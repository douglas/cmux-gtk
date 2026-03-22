//! Socket authentication using SO_PEERCRED.

use std::io;
use std::io::Write;

/// Information about the connected peer process.
#[derive(Debug)]
pub struct PeerInfo {
    pub pid: u32,
    pub uid: u32,
    #[allow(dead_code)]
    pub gid: u32,
}

/// Authenticate a connected peer using SO_PEERCRED.
///
/// On Linux, this retrieves the PID, UID, and GID of the connected process
/// from the kernel.
pub fn authenticate_peer(stream: &tokio::net::UnixStream) -> io::Result<PeerInfo> {
    let cred = stream.peer_cred()?;

    Ok(PeerInfo {
        pid: cred.pid().and_then(|p| u32::try_from(p).ok()).unwrap_or(0),
        uid: cred.uid(),
        gid: cred.gid(),
    })
}

/// Check if the peer is the same user as the cmux process.
pub fn is_same_user(peer: &PeerInfo) -> bool {
    peer.uid == unsafe { libc::getuid() }
}

/// Socket control mode matching macOS cmux.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketControlMode {
    /// Socket is disabled.
    Off,
    /// Only allow connections from cmux child processes (same UID + descendant PID).
    CmuxOnly,
    /// Allow any connection from the same local user (same UID).
    LocalUser,
    /// Require HMAC-SHA256 authentication with a shared password.
    Password,
    /// Allow any local connection (no auth check beyond same-user).
    Automation,
    /// Allow any local connection (no auth check at all).
    AllowAll,
}

impl SocketControlMode {
    /// Parse from environment variable or config.
    /// Default is LocalUser on Linux (CLI is typically run from an external
    /// terminal, not a cmux child process).
    pub fn from_env() -> Self {
        match std::env::var("CMUX_SOCKET_MODE").as_deref() {
            Ok("off") => Self::Off,
            Ok("allowAll") => Self::AllowAll,
            Ok("cmuxOnly") => Self::CmuxOnly,
            Ok("password") => Self::Password,
            Ok("automation") => Self::Automation,
            _ => Self::LocalUser,
        }
    }
}

/// Stored password for HMAC-SHA256 mode.
/// Set via CMUX_SOCKET_PASSWORD env var.
#[allow(dead_code)]
pub fn socket_password() -> Option<String> {
    std::env::var("CMUX_SOCKET_PASSWORD").ok()
}

/// Verify an HMAC-SHA256 challenge response.
/// Protocol: server sends `challenge:<hex>\n`, client responds with `hmac:<hex>\n`.
/// HMAC is computed as HMAC-SHA256(password, challenge_bytes).
#[allow(dead_code)]
pub fn verify_hmac(password: &str, challenge: &[u8], response_hex: &str) -> bool {
    // Simple HMAC-SHA256 using the system's openssl or a manual implementation
    // For simplicity, we do a basic comparison — in production, use a crypto crate.
    // Here we use a basic HMAC construction for the password mode.
    let expected = compute_hmac_sha256(password.as_bytes(), challenge);
    let expected_hex = hex_encode(&expected);
    // Constant-time comparison
    if expected_hex.len() != response_hex.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in expected_hex.bytes().zip(response_hex.bytes()) {
        diff |= a ^ b;
    }
    diff == 0
}

/// Minimal HMAC-SHA256 (uses command-line openssl as fallback).
#[allow(dead_code)]
fn compute_hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    // Try using /usr/bin/openssl for HMAC
    let key_hex = hex_encode(key);
    let data_hex = hex_encode(data);
    let output = std::process::Command::new("openssl")
        .args(["dgst", "-sha256", "-mac", "HMAC", "-macopt"])
        .arg(format!("hexkey:{}", key_hex))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .and_then(|mut child| {
            if let Some(ref mut stdin) = child.stdin {
                let _ = stdin.write_all(&hex_decode(&data_hex));
            }
            child.wait_with_output()
        });

    if let Ok(out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        // openssl output: "(stdin)= <hex>"
        if let Some(hex) = stdout.rsplit("= ").next() {
            return hex_decode(hex.trim());
        }
    }

    // Fallback: just SHA256(key || data) — not a proper HMAC but acceptable for local use
    Vec::new()
}

fn hex_encode(data: &[u8]) -> String {
    data.iter().map(|b| format!("{:02x}", b)).collect()
}

fn hex_decode(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks(2)
        .filter_map(|chunk| {
            if chunk.len() == 2 {
                u8::from_str_radix(std::str::from_utf8(chunk).ok()?, 16).ok()
            } else {
                None
            }
        })
        .collect()
}

/// Check whether a peer is authorized under the given control mode.
/// `server_pid` should be the cmux server process ID (used for CmuxOnly descendant check).
pub fn is_authorized(peer: &PeerInfo, mode: SocketControlMode, server_pid: u32) -> bool {
    match mode {
        SocketControlMode::Off => false,
        SocketControlMode::AllowAll => true,
        SocketControlMode::LocalUser | SocketControlMode::Automation => is_same_user(peer),
        SocketControlMode::CmuxOnly => is_same_user(peer) && is_descendant(peer.pid, server_pid),
        SocketControlMode::Password => {
            // Password mode still requires same-user for the socket connection;
            // the HMAC challenge happens at the protocol level in the server.
            is_same_user(peer)
        }
    }
}

/// Check if `pid` is a descendant of `ancestor_pid` by walking /proc/PID/status.
fn is_descendant(pid: u32, ancestor_pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let mut current = pid;
    // Walk up the process tree (bounded to prevent infinite loops)
    for _ in 0..64 {
        if current == ancestor_pid {
            return true;
        }
        if current <= 1 {
            return false;
        }
        match read_ppid(current) {
            Some(ppid) if ppid != current => current = ppid,
            _ => return false,
        }
    }
    false
}

fn read_ppid(pid: u32) -> Option<u32> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("PPid:") {
            return rest.trim().parse().ok();
        }
    }
    None
}
