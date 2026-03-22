//! SOCKS5/HTTP CONNECT proxy tunnel.
//!
//! Listens on a local ephemeral port and tunnels connections through
//! the remote daemon's proxy.open/write/close RPC.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use super::rpc::{RemoteRpcClient, StreamEvent};

/// A local proxy that tunnels TCP connections through the remote daemon.
pub struct ProxyTunnel {
    port: u16,
    alive: Arc<std::sync::atomic::AtomicBool>,
}

impl ProxyTunnel {
    /// Start a SOCKS5/HTTP CONNECT proxy on an ephemeral localhost port.
    /// Returns the tunnel handle with the bound port.
    pub fn start(rpc: Arc<RemoteRpcClient>) -> Result<Self, String> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| format!("Failed to bind proxy listener: {}", e))?;
        let port = listener
            .local_addr()
            .map_err(|e| format!("Failed to get local addr: {}", e))?
            .port();

        let alive = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let alive_clone = Arc::clone(&alive);

        // Set a short accept timeout so the thread can check `alive`
        listener.set_nonblocking(false).ok();

        std::thread::spawn(move || {
            tracing::info!(port, "Proxy tunnel listening");
            // Set short timeout for accept so we can check alive flag
            let _ = listener.set_nonblocking(false);

            loop {
                if !alive_clone.load(std::sync::atomic::Ordering::Acquire) {
                    break;
                }

                // Use a short SO_RCVTIMEO to periodically check alive
                // We can't use set_nonblocking easily, so just accept and handle
                match listener.accept() {
                    Ok((stream, addr)) => {
                        tracing::debug!(?addr, "Proxy: new connection");
                        let rpc = Arc::clone(&rpc);
                        std::thread::spawn(move || {
                            if let Err(e) = handle_proxy_connection(stream, &rpc) {
                                tracing::debug!("Proxy connection error: {}", e);
                            }
                        });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    Err(e) => {
                        tracing::warn!("Proxy accept error: {}", e);
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            }
            tracing::info!("Proxy tunnel stopped");
        });

        Ok(Self { port, alive })
    }

    /// The local port the proxy is listening on.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Stop the proxy tunnel.
    pub fn stop(&self) {
        self.alive
            .store(false, std::sync::atomic::Ordering::Release);
        // Connect to ourselves to unblock the accept() call
        let _ = TcpStream::connect(format!("127.0.0.1:{}", self.port));
    }
}

impl Drop for ProxyTunnel {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Handle a single proxy client connection.
fn handle_proxy_connection(mut stream: TcpStream, rpc: &RemoteRpcClient) -> Result<(), String> {
    stream.set_read_timeout(Some(Duration::from_secs(30))).ok();

    let mut buf = [0u8; 4096];

    // Read first bytes to detect SOCKS5 vs HTTP CONNECT
    let n = stream.read(&mut buf).map_err(|e| format!("read: {}", e))?;
    if n == 0 {
        return Err("empty connection".to_string());
    }

    if buf[0] == 0x05 {
        // SOCKS5
        handle_socks5(stream, rpc, &buf[..n])
    } else if buf[..n].starts_with(b"CONNECT ") {
        // HTTP CONNECT
        handle_http_connect(stream, rpc, &buf[..n])
    } else {
        Err(format!(
            "Unknown proxy protocol: first byte 0x{:02x}",
            buf[0]
        ))
    }
}

/// Handle a SOCKS5 connection.
fn handle_socks5(
    mut stream: TcpStream,
    rpc: &RemoteRpcClient,
    initial: &[u8],
) -> Result<(), String> {
    // Parse greeting: version(1) + nmethods(1) + methods(nmethods)
    if initial.len() < 2 || initial[0] != 0x05 {
        return Err("Invalid SOCKS5 greeting".to_string());
    }
    let nmethods = initial[1] as usize;
    let greeting_len = 2 + nmethods;

    // We may need to read more if the greeting wasn't fully in the initial read
    let mut greeting = initial.to_vec();
    while greeting.len() < greeting_len {
        let mut more = [0u8; 256];
        let n = stream.read(&mut more).map_err(|e| format!("read: {}", e))?;
        if n == 0 {
            return Err("Connection closed during greeting".to_string());
        }
        greeting.extend_from_slice(&more[..n]);
    }

    // Send auth reply: no authentication required
    stream
        .write_all(&[0x05, 0x00])
        .map_err(|e| format!("write auth: {}", e))?;

    // Pipelined data after the greeting (may contain the CONNECT request)
    let pipelined = if greeting.len() > greeting_len {
        greeting[greeting_len..].to_vec()
    } else {
        Vec::new()
    };

    // Read CONNECT request: version(1) + cmd(1) + rsv(1) + atyp(1) + dst.addr + dst.port(2)
    let mut connect_buf = pipelined;
    while connect_buf.len() < 4 {
        let mut more = [0u8; 1024];
        let n = stream.read(&mut more).map_err(|e| format!("read: {}", e))?;
        if n == 0 {
            return Err("Connection closed during CONNECT".to_string());
        }
        connect_buf.extend_from_slice(&more[..n]);
    }

    if connect_buf[0] != 0x05 || connect_buf[1] != 0x01 {
        // Not a CONNECT command
        let reply = [0x05, 0x07, 0x00, 0x01, 0, 0, 0, 0, 0, 0]; // command not supported
        let _ = stream.write_all(&reply);
        return Err(format!(
            "Unsupported SOCKS5 command: 0x{:02x}",
            connect_buf[1]
        ));
    }

    let atyp = connect_buf[3];
    let (host, port, consumed) = match atyp {
        0x01 => {
            // IPv4
            while connect_buf.len() < 10 {
                let mut more = [0u8; 256];
                let n = stream.read(&mut more).map_err(|e| format!("read: {}", e))?;
                connect_buf.extend_from_slice(&more[..n]);
            }
            let ip = format!(
                "{}.{}.{}.{}",
                connect_buf[4], connect_buf[5], connect_buf[6], connect_buf[7]
            );
            let port = u16::from_be_bytes([connect_buf[8], connect_buf[9]]);
            (ip, port, 10)
        }
        0x03 => {
            // Domain name
            let domain_len = connect_buf[4] as usize;
            let needed = 5 + domain_len + 2;
            while connect_buf.len() < needed {
                let mut more = [0u8; 1024];
                let n = stream.read(&mut more).map_err(|e| format!("read: {}", e))?;
                connect_buf.extend_from_slice(&more[..n]);
            }
            let host = String::from_utf8_lossy(&connect_buf[5..5 + domain_len]).to_string();
            let port_offset = 5 + domain_len;
            let port = u16::from_be_bytes([connect_buf[port_offset], connect_buf[port_offset + 1]]);
            (host, port, needed)
        }
        0x04 => {
            // IPv6
            while connect_buf.len() < 22 {
                let mut more = [0u8; 256];
                let n = stream.read(&mut more).map_err(|e| format!("read: {}", e))?;
                connect_buf.extend_from_slice(&more[..n]);
            }
            let mut octets = [0u8; 16];
            octets.copy_from_slice(&connect_buf[4..20]);
            let ip = std::net::Ipv6Addr::from(octets).to_string();
            let port = u16::from_be_bytes([connect_buf[20], connect_buf[21]]);
            (ip, port, 22)
        }
        _ => {
            let reply = [0x05, 0x08, 0x00, 0x01, 0, 0, 0, 0, 0, 0];
            let _ = stream.write_all(&reply);
            return Err(format!("Unsupported SOCKS5 address type: 0x{:02x}", atyp));
        }
    };

    // Preserve pipelined data after CONNECT request
    let pipelined_payload = if connect_buf.len() > consumed {
        Some(connect_buf[consumed..].to_vec())
    } else {
        None
    };

    // Open remote proxy stream
    let stream_id = rpc
        .proxy_open(&host, port)
        .map_err(|e| format!("proxy.open({}:{}): {}", host, port, e))?;

    // Subscribe to push events
    let events = rpc
        .proxy_subscribe(stream_id)
        .map_err(|e| format!("proxy.subscribe: {}", e))?;

    // Send SOCKS5 success reply
    let reply = [0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0];
    stream
        .write_all(&reply)
        .map_err(|e| format!("write reply: {}", e))?;

    // Forward pipelined payload
    if let Some(data) = pipelined_payload {
        if !data.is_empty() {
            rpc.proxy_write(stream_id, &data)
                .map_err(|e| format!("proxy.write pipelined: {}", e))?;
        }
    }

    // Bidirectional relay
    relay_streams(&mut stream, rpc, stream_id, events)
}

/// Handle an HTTP CONNECT request.
fn handle_http_connect(
    mut stream: TcpStream,
    rpc: &RemoteRpcClient,
    initial: &[u8],
) -> Result<(), String> {
    // Read until we have the full HTTP headers
    let mut buf = initial.to_vec();
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        if buf.len() > 65536 {
            return Err("HTTP CONNECT headers too large".to_string());
        }
        let mut more = [0u8; 4096];
        let n = stream.read(&mut more).map_err(|e| format!("read: {}", e))?;
        if n == 0 {
            return Err("Connection closed during HTTP CONNECT".to_string());
        }
        buf.extend_from_slice(&more[..n]);
    }

    // Parse CONNECT host:port from first line
    let header_end = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("Malformed HTTP CONNECT header")?
        + 4;
    let first_line = buf
        .iter()
        .take_while(|&&b| b != b'\r')
        .copied()
        .collect::<Vec<_>>();
    let first_line = String::from_utf8_lossy(&first_line);

    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 || parts[0] != "CONNECT" {
        return Err(format!("Invalid HTTP CONNECT: {}", first_line));
    }

    let target = parts[1];
    let (host, port) = if let Some(colon) = target.rfind(':') {
        let host = &target[..colon];
        let port: u16 = target[colon + 1..]
            .parse()
            .map_err(|_| format!("Invalid port in CONNECT target: {}", target))?;
        (host.to_string(), port)
    } else {
        (target.to_string(), 80)
    };

    // Pipelined data after headers
    let pipelined = if buf.len() > header_end {
        Some(buf[header_end..].to_vec())
    } else {
        None
    };

    // Open remote proxy stream
    let stream_id = rpc
        .proxy_open(&host, port)
        .map_err(|e| format!("proxy.open({}:{}): {}", host, port, e))?;

    let events = rpc
        .proxy_subscribe(stream_id)
        .map_err(|e| format!("proxy.subscribe: {}", e))?;

    // Send HTTP 200 success
    stream
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .map_err(|e| format!("write HTTP reply: {}", e))?;

    // Forward pipelined data
    if let Some(data) = pipelined {
        if !data.is_empty() {
            rpc.proxy_write(stream_id, &data)
                .map_err(|e| format!("proxy.write pipelined: {}", e))?;
        }
    }

    relay_streams(&mut stream, rpc, stream_id, events)
}

/// Bidirectional relay between a local TCP stream and a remote proxy stream.
fn relay_streams(
    stream: &mut TcpStream,
    rpc: &RemoteRpcClient,
    stream_id: u64,
    events: std::sync::mpsc::Receiver<StreamEvent>,
) -> Result<(), String> {
    // Set stream to non-blocking for the relay loop
    stream.set_nonblocking(true).ok();

    let mut local_buf = [0u8; 32768];

    loop {
        // Read from local → write to remote
        match stream.read(&mut local_buf) {
            Ok(0) => {
                // Local EOF
                let _ = rpc.proxy_close(stream_id);
                return Ok(());
            }
            Ok(n) => {
                if let Err(e) = rpc.proxy_write(stream_id, &local_buf[..n]) {
                    tracing::debug!("proxy.write failed: {}", e);
                    return Ok(());
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No data available, continue to check remote
            }
            Err(e) => {
                let _ = rpc.proxy_close(stream_id);
                return Err(format!("Local read error: {}", e));
            }
        }

        // Read from remote → write to local
        match events.try_recv() {
            Ok(StreamEvent::Data(data)) => {
                stream.set_nonblocking(false).ok();
                if let Err(e) = stream.write_all(&data) {
                    tracing::debug!("Local write failed: {}", e);
                    let _ = rpc.proxy_close(stream_id);
                    return Ok(());
                }
                stream.set_nonblocking(true).ok();
            }
            Ok(StreamEvent::Eof(final_data)) => {
                stream.set_nonblocking(false).ok();
                if let Some(data) = final_data {
                    let _ = stream.write_all(&data);
                }
                // Shut down the write side — remote is done sending
                let _ = stream.shutdown(std::net::Shutdown::Write);
                return Ok(());
            }
            Ok(StreamEvent::Error(msg)) => {
                tracing::debug!(stream_id, "Remote stream error: {}", msg);
                let _ = rpc.proxy_close(stream_id);
                return Ok(());
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                // No remote data, brief sleep to avoid busy loop
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                return Ok(());
            }
        }
    }
}
