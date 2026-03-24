//! Socket authentication using SO_PEERCRED.

use std::io;

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
    // SAFETY: getuid() is always safe — no failure mode, no arguments.
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
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    type HmacSha256 = Hmac<Sha256>;
    let Ok(mut mac) = HmacSha256::new_from_slice(password.as_bytes()) else {
        return false;
    };
    mac.update(challenge);
    let expected = mac.finalize().into_bytes();
    let expected_hex = hex_encode(&expected);

    // Constant-time comparison via fixed-length XOR
    if expected_hex.len() != response_hex.len() {
        return false;
    }
    let diff = expected_hex
        .bytes()
        .zip(response_hex.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b));
    diff == 0
}

/// Compute HMAC-SHA256 and return the raw bytes.
pub fn compute_hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(key)
        .expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

pub fn hex_encode(data: &[u8]) -> String {
    data.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Decode a hex string to bytes.  Returns `None` for odd-length or non-hex input.
pub fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let mut bytes = Vec::with_capacity(s.len() / 2);
    let mut chars = s.chars();
    while let (Some(a), Some(b)) = (chars.next(), chars.next()) {
        let hi = a.to_digit(16)?;
        let lo = b.to_digit(16)?;
        bytes.push(((hi << 4) | lo) as u8);
    }
    Some(bytes)
}

/// Constant-time HMAC-SHA256 verification using raw key bytes.
///
/// Use this instead of `verify_hmac` when the key is already raw bytes
/// (e.g., decoded from hex).
pub fn verify_hmac_raw(key: &[u8], message: &[u8], response_hex: &str) -> bool {
    let expected = compute_hmac_sha256(key, message);
    let expected_hex = hex_encode(&expected);
    if expected_hex.len() != response_hex.len() {
        return false;
    }
    expected_hex
        .bytes()
        .zip(response_hex.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_known_vector() {
        // RFC 4231 Test Case 2: HMAC-SHA256 with "Jefe" key and "what do ya want for nothing?"
        let key = b"Jefe";
        let data = b"what do ya want for nothing?";
        let result = hex_encode(&compute_hmac_sha256(key, data));
        assert_eq!(
            result,
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn verify_hmac_valid() {
        let password = "test-password";
        let challenge = b"random-challenge-bytes";
        let expected = compute_hmac_sha256(password.as_bytes(), challenge);
        let expected_hex = hex_encode(&expected);
        assert!(verify_hmac(password, challenge, &expected_hex));
    }

    #[test]
    fn verify_hmac_invalid() {
        let password = "test-password";
        let challenge = b"random-challenge-bytes";
        assert!(!verify_hmac(password, challenge, "deadbeef00112233"));
    }

    #[test]
    fn verify_hmac_wrong_password() {
        let challenge = b"challenge";
        let expected = compute_hmac_sha256(b"correct-password", challenge);
        let expected_hex = hex_encode(&expected);
        assert!(!verify_hmac("wrong-password", challenge, &expected_hex));
    }

    #[test]
    fn verify_hmac_empty_inputs() {
        // Empty password and empty challenge should still produce a valid HMAC
        let expected = compute_hmac_sha256(b"", b"");
        let expected_hex = hex_encode(&expected);
        assert!(verify_hmac("", b"", &expected_hex));
        // But should NOT match an empty response
        assert!(!verify_hmac("", b"", ""));
    }

    #[test]
    fn hex_encode_roundtrip() {
        assert_eq!(hex_encode(&[0x00, 0xff, 0xab, 0x12]), "00ffab12");
        assert_eq!(hex_encode(&[]), "");
    }

    #[test]
    fn is_descendant_self() {
        let pid = std::process::id();
        assert!(is_descendant(pid, pid));
    }

    #[test]
    fn is_descendant_of_init() {
        // Current process should be a descendant of PID 1
        let pid = std::process::id();
        assert!(is_descendant(pid, 1));
    }

    #[test]
    fn is_descendant_zero_pid() {
        assert!(!is_descendant(0, 1));
    }
}
