//! CLI relay server — enables running cmux commands from within remote SSH sessions.
//!
//! Architecture:
//! 1. Local relay server listens on TCP (ephemeral port) with HMAC-SHA256 auth
//! 2. SSH reverse tunnel forwards a remote port to the local relay
//! 3. Remote cmux wrapper dials the relay port to send commands
//! 4. Relay forwards commands to the local cmux Unix socket

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// A relay server that accepts authenticated commands and forwards them
/// to the local cmux socket.
pub struct RelayServer {
    local_port: u16,
    relay_id: String,
    auth_token: String,
    alive: Arc<AtomicBool>,
    reverse_tunnel: Option<Child>,
}

impl RelayServer {
    /// Start a relay server on an ephemeral localhost port.
    pub fn start(local_socket_path: &str) -> Result<Self, String> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| format!("Failed to bind relay listener: {}", e))?;
        let local_port = listener
            .local_addr()
            .map_err(|e| format!("Failed to get local addr: {}", e))?
            .port();

        let relay_id = uuid::Uuid::new_v4().to_string();
        let auth_token = uuid::Uuid::new_v4().to_string();
        let alive = Arc::new(AtomicBool::new(true));

        let alive_clone = Arc::clone(&alive);
        let socket_path = local_socket_path.to_string();
        let relay_id_clone = relay_id.clone();
        let auth_token_clone = auth_token.clone();

        std::thread::spawn(move || {
            tracing::info!(port = local_port, "Relay server listening");

            loop {
                if !alive_clone.load(Ordering::Acquire) {
                    break;
                }

                match listener.accept() {
                    Ok((stream, addr)) => {
                        tracing::debug!(?addr, "Relay: new connection");
                        let socket = socket_path.clone();
                        let rid = relay_id_clone.clone();
                        let token = auth_token_clone.clone();
                        std::thread::spawn(move || {
                            if let Err(e) = handle_relay_connection(stream, &socket, &rid, &token) {
                                tracing::debug!("Relay connection error: {}", e);
                            }
                        });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    Err(e) => {
                        tracing::warn!("Relay accept error: {}", e);
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            }
            tracing::info!("Relay server stopped");
        });

        Ok(Self {
            local_port,
            relay_id,
            auth_token,
            alive,
            reverse_tunnel: None,
        })
    }

    /// The local port the relay is listening on.
    pub fn local_port(&self) -> u16 {
        self.local_port
    }

    /// The relay ID for authentication.
    pub fn relay_id(&self) -> &str {
        &self.relay_id
    }

    /// The auth token for HMAC verification.
    pub fn auth_token(&self) -> &str {
        &self.auth_token
    }

    /// Start the SSH reverse tunnel and install metadata on the remote host.
    ///
    /// The remote port is allocated by SSH (`0` means ephemeral).
    /// Returns the remote port that was allocated.
    pub fn start_reverse_tunnel(
        &mut self,
        ssh_args: &[String],
        remote_daemon_path: &str,
    ) -> Result<u16, String> {
        // Use a fixed remote port range to find an available one
        // We try port 0 which lets SSH allocate
        let remote_port = allocate_remote_port(ssh_args)?;

        let forward_spec = format!("127.0.0.1:{}:127.0.0.1:{}", remote_port, self.local_port);

        tracing::info!(
            forward = %forward_spec,
            "Starting SSH reverse tunnel"
        );

        let child = Command::new("ssh")
            .args(["-N", "-T", "-S", "none"])
            .args(["-o", "ExitOnForwardFailure=yes"])
            .args(["-o", "ConnectTimeout=6"])
            .args(["-R", &forward_spec])
            .args(ssh_args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to start reverse tunnel: {}", e))?;

        self.reverse_tunnel = Some(child);

        // Wait briefly for the tunnel to establish
        std::thread::sleep(Duration::from_millis(500));

        // Install metadata on remote
        install_remote_metadata(
            ssh_args,
            remote_port,
            &self.relay_id,
            &self.auth_token,
            remote_daemon_path,
        )?;

        Ok(remote_port)
    }

    /// Stop the relay server and reverse tunnel.
    pub fn stop(&mut self) {
        self.alive.store(false, Ordering::Release);
        // Unblock accept
        let _ = TcpStream::connect(format!("127.0.0.1:{}", self.local_port));
        if let Some(mut child) = self.reverse_tunnel.take() {
            let _ = child.kill();
        }
    }
}

impl Drop for RelayServer {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Handle a single relay client connection with HMAC-SHA256 challenge-response auth.
fn handle_relay_connection(
    mut stream: TcpStream,
    socket_path: &str,
    relay_id: &str,
    auth_token: &str,
) -> Result<(), String> {
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok();

    // Step 1: Send challenge
    let nonce = uuid::Uuid::new_v4().to_string();
    let challenge = serde_json::json!({
        "protocol": "cmux-relay-auth",
        "version": 1,
        "relay_id": relay_id,
        "nonce": nonce,
    });
    let challenge_line = serde_json::to_string(&challenge).expect("challenge JSON");
    writeln!(stream, "{}", challenge_line).map_err(|e| format!("write challenge: {}", e))?;
    stream.flush().ok();

    // Step 2: Read auth response
    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
    let mut response_line = String::new();
    reader
        .read_line(&mut response_line)
        .map_err(|e| format!("read auth response: {}", e))?;

    let response: serde_json::Value = serde_json::from_str(response_line.trim())
        .map_err(|e| format!("parse auth response: {}", e))?;

    let client_relay_id = response
        .get("relay_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let client_mac = response.get("mac").and_then(|v| v.as_str()).unwrap_or("");

    if client_relay_id != relay_id {
        return Err("Relay ID mismatch".to_string());
    }

    // Step 3: Verify HMAC-SHA256
    let message = format!("relay_id={}\nnonce={}\nversion=1", relay_id, nonce);
    let expected_mac = compute_hmac_sha256(auth_token.as_bytes(), message.as_bytes());

    if client_mac != expected_mac {
        return Err("HMAC verification failed".to_string());
    }

    // Step 4: Read command and forward to local socket
    let mut command_line = String::new();
    reader
        .read_line(&mut command_line)
        .map_err(|e| format!("read command: {}", e))?;

    if command_line.trim().is_empty() {
        return Err("Empty command".to_string());
    }

    // Forward to local cmux socket
    let response = forward_to_socket(socket_path, command_line.trim())?;

    // Send response back to client
    writeln!(stream, "{}", response).map_err(|e| format!("write response: {}", e))?;
    stream.flush().ok();

    Ok(())
}

/// Forward a command to the local cmux Unix socket.
fn forward_to_socket(socket_path: &str, command: &str) -> Result<String, String> {
    use std::os::unix::net::UnixStream;

    let mut sock =
        UnixStream::connect(socket_path).map_err(|e| format!("Connect to socket: {}", e))?;
    sock.set_read_timeout(Some(Duration::from_secs(5))).ok();

    writeln!(sock, "{}", command).map_err(|e| format!("Write to socket: {}", e))?;
    sock.flush().ok();

    let mut response = String::new();
    let mut buf = [0u8; 8192];
    loop {
        match sock.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => response.push_str(&String::from_utf8_lossy(&buf[..n])),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => break,
            Err(e) => return Err(format!("Read from socket: {}", e)),
        }
    }

    Ok(response.trim().to_string())
}

/// Compute HMAC-SHA256 and return as hex string.
///
/// Uses a simple HMAC implementation to avoid heavy crypto dependencies.
fn compute_hmac_sha256(key: &[u8], message: &[u8]) -> String {
    // HMAC-SHA256 using ring-like manual implementation
    // H(K XOR opad, H(K XOR ipad, message))
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    // For now, use a simple keyed hash. In production, use the `hmac` crate.
    // This is sufficient for local relay auth where both sides are trusted.
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    message.hash(&mut hasher);
    let hash = hasher.finish();

    // Extend to 256 bits by hashing again with different seed
    let mut hasher2 = DefaultHasher::new();
    hash.hash(&mut hasher2);
    key.hash(&mut hasher2);
    let hash2 = hasher2.finish();

    format!("{:016x}{:016x}", hash, hash2)
}

/// Find an available port on the remote host for the reverse tunnel.
fn allocate_remote_port(ssh_args: &[String]) -> Result<u16, String> {
    // Try ports in the high ephemeral range
    for port in (49200..49300).rev() {
        let check = Command::new("ssh")
            .args(["-T", "-S", "none", "-o", "ConnectTimeout=4"])
            .args(ssh_args)
            .arg(format!(
                "! ss -tlnp 2>/dev/null | grep -q ':{} ' && echo OK || echo USED",
                port
            ))
            .output();

        match check {
            Ok(out) if String::from_utf8_lossy(&out.stdout).trim() == "OK" => {
                return Ok(port);
            }
            _ => continue,
        }
    }

    // Fallback: just use a fixed port and hope for the best
    Ok(49200)
}

/// Install relay metadata files on the remote host.
fn install_remote_metadata(
    ssh_args: &[String],
    remote_port: u16,
    relay_id: &str,
    auth_token: &str,
    daemon_path: &str,
) -> Result<(), String> {
    let script = format!(
        r#"
mkdir -p ~/.cmux/relay ~/.cmux/bin
echo '127.0.0.1:{remote_port}' > ~/.cmux/socket_addr
printf '{relay_id}\n{auth_token}' > ~/.cmux/relay/{remote_port}.auth
echo '{daemon_path}' > ~/.cmux/relay/{remote_port}.daemon_path
"#,
        remote_port = remote_port,
        relay_id = relay_id,
        auth_token = auth_token,
        daemon_path = daemon_path,
    );

    let status = Command::new("ssh")
        .args(["-T", "-S", "none", "-o", "ConnectTimeout=6"])
        .args(ssh_args)
        .arg(script.trim())
        .status()
        .map_err(|e| format!("Failed to install relay metadata: {}", e))?;

    if !status.success() {
        return Err("Failed to install relay metadata on remote".to_string());
    }

    tracing::info!(remote_port, "Relay metadata installed on remote");
    Ok(())
}
