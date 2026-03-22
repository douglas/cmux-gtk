//! JSON-line RPC client for communicating with cmuxd-remote over SSH stdio.
//!
//! The remote daemon accepts `serve --stdio` and communicates via
//! newline-delimited JSON on stdin/stdout:
//! - Request:  `{"id": N, "method": "...", "params": {...}}\n`
//! - Response: `{"id": N, "ok": true, "result": {...}}\n`
//! - Push:     `{"method": "proxy.stream.push", "params": {...}}\n`

use base64::Engine;
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

/// Events pushed by the remote daemon for proxy streams.
#[derive(Debug)]
pub enum StreamEvent {
    /// Binary data received from remote connection.
    Data(Vec<u8>),
    /// End of stream, possibly with final data.
    Eof(Option<Vec<u8>>),
    /// Error on the stream.
    Error(String),
}

/// Response from the `hello` RPC call.
#[derive(Debug)]
pub struct HelloResponse {
    pub name: String,
    pub version: String,
    pub capabilities: Vec<String>,
    pub remote_path: String,
}

type PendingCall = std::sync::mpsc::Sender<Result<Value, RpcError>>;

/// RPC error type.
#[derive(Debug, Clone)]
pub struct RpcError {
    pub code: String,
    pub message: String,
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for RpcError {}

/// JSON-line RPC client over an SSH stdio connection to cmuxd-remote.
pub struct RemoteRpcClient {
    stdin: Mutex<std::process::ChildStdin>,
    next_id: AtomicU64,
    pending: Arc<Mutex<HashMap<u64, PendingCall>>>,
    stream_subs: Arc<Mutex<HashMap<u64, mpsc::Sender<StreamEvent>>>>,
    alive: Arc<AtomicBool>,
    _reader_thread: std::thread::JoinHandle<()>,
    child: Mutex<Child>,
}

impl RemoteRpcClient {
    /// Connect to a remote daemon via SSH.
    ///
    /// `ssh_args` should contain the SSH destination and any options
    /// (e.g., `["-p", "22", "user@host"]`).
    /// `remote_daemon_path` is the path to cmuxd-remote on the remote host.
    pub fn new(ssh_args: &[String], remote_daemon_path: &str) -> Result<Self, String> {
        let mut cmd = Command::new("ssh");
        cmd.args(["-T", "-S", "none"])
            .args([
                "-o",
                "ConnectTimeout=6",
                "-o",
                "ServerAliveInterval=20",
                "-o",
                "ServerAliveCountMax=2",
                "-o",
                "StrictHostKeyChecking=accept-new",
            ])
            .args(ssh_args)
            .arg(format!("sh -c 'exec {} serve --stdio'", remote_daemon_path))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn SSH: {}", e))?;

        let stdin = child.stdin.take().ok_or("No stdin on SSH process")?;
        let stdout = child.stdout.take().ok_or("No stdout on SSH process")?;

        let pending: Arc<Mutex<HashMap<u64, PendingCall>>> = Arc::new(Mutex::new(HashMap::new()));
        let stream_subs: Arc<Mutex<HashMap<u64, mpsc::Sender<StreamEvent>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let alive = Arc::new(AtomicBool::new(true));

        // Reader thread: parse JSON lines from stdout, route to pending calls or stream subs
        let pending_clone = Arc::clone(&pending);
        let subs_clone = Arc::clone(&stream_subs);
        let alive_clone = Arc::clone(&alive);
        let reader_thread = std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                let line = match line {
                    Ok(l) => l,
                    Err(e) => {
                        tracing::debug!("RPC reader: stdout closed: {}", e);
                        break;
                    }
                };

                if line.is_empty() {
                    continue;
                }

                let msg: Value = match serde_json::from_str(&line) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!("RPC reader: invalid JSON: {} — {}", e, line);
                        continue;
                    }
                };

                // Check if this is a response (has "id") or a push event
                if let Some(id) = msg.get("id").and_then(|v| v.as_u64()) {
                    // Response to a pending call
                    let sender = pending_clone.lock().expect("mutex poisoned").remove(&id);
                    if let Some(tx) = sender {
                        let result = if msg.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
                            Ok(msg.get("result").cloned().unwrap_or(Value::Null))
                        } else {
                            let err = msg.get("error").cloned().unwrap_or(Value::Null);
                            Err(RpcError {
                                code: err
                                    .get("code")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown")
                                    .to_string(),
                                message: err
                                    .get("message")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown error")
                                    .to_string(),
                            })
                        };
                        let _ = tx.send(result);
                    }
                } else if let Some(method) = msg.get("method").and_then(|v| v.as_str()) {
                    // Push event
                    if method == "proxy.stream.push" {
                        if let Some(params) = msg.get("params") {
                            let stream_id = params
                                .get("stream_id")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0);
                            let event_type =
                                params.get("event").and_then(|v| v.as_str()).unwrap_or("");

                            let data =
                                params
                                    .get("data_base64")
                                    .and_then(|v| v.as_str())
                                    .and_then(|s| {
                                        base64::engine::general_purpose::STANDARD.decode(s).ok()
                                    });

                            let event = match event_type {
                                "proxy.stream.data" => StreamEvent::Data(data.unwrap_or_default()),
                                "proxy.stream.eof" => StreamEvent::Eof(data),
                                "proxy.stream.error" => StreamEvent::Error(
                                    params
                                        .get("message")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("stream error")
                                        .to_string(),
                                ),
                                _ => continue,
                            };

                            let subs = subs_clone.lock().expect("mutex poisoned");
                            if let Some(tx) = subs.get(&stream_id) {
                                let _ = tx.send(event);
                            }
                        }
                    }
                }
            }

            alive_clone.store(false, Ordering::Release);
            tracing::debug!("RPC reader thread exiting");
        });

        Ok(Self {
            stdin: Mutex::new(stdin),
            next_id: AtomicU64::new(1),
            pending,
            stream_subs,
            alive,
            _reader_thread: reader_thread,
            child: Mutex::new(child),
        })
    }

    /// Check if the SSH process is still alive.
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }

    /// Send an RPC request and wait for the response.
    pub fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        if !self.is_alive() {
            return Err("RPC client is not connected".to_string());
        }

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = serde_json::json!({
            "id": id,
            "method": method,
            "params": params,
        });

        let (tx, rx) = std::sync::mpsc::channel();
        self.pending.lock().expect("mutex poisoned").insert(id, tx);

        {
            let mut stdin = self.stdin.lock().expect("mutex poisoned");
            let line = serde_json::to_string(&request).expect("RPC request JSON");
            if let Err(e) = writeln!(stdin, "{}", line) {
                self.pending.lock().expect("mutex poisoned").remove(&id);
                return Err(format!("Failed to write RPC request: {}", e));
            }
            let _ = stdin.flush();
        }

        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(rpc_err)) => Err(format!("RPC error: {}", rpc_err)),
            Err(_) => {
                self.pending.lock().expect("mutex poisoned").remove(&id);
                Err("RPC call timed out".to_string())
            }
        }
    }

    /// Perform the hello handshake and verify capabilities.
    pub fn hello(&self) -> Result<HelloResponse, String> {
        let result = self.call("hello", serde_json::json!({}))?;

        let name = result
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let version = result
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let capabilities: Vec<String> = result
            .get("capabilities")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let remote_path = result
            .get("remote_path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if !capabilities.contains(&"proxy.stream.push".to_string()) {
            return Err("Remote daemon missing required capability: proxy.stream.push".to_string());
        }

        Ok(HelloResponse {
            name,
            version,
            capabilities,
            remote_path,
        })
    }

    /// Open a proxy stream to a remote host:port. Returns the stream ID.
    pub fn proxy_open(&self, host: &str, port: u16) -> Result<u64, String> {
        let result = self.call(
            "proxy.open",
            serde_json::json!({"host": host, "port": port}),
        )?;
        result
            .get("stream_id")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| "proxy.open: missing stream_id in response".to_string())
    }

    /// Write data to a proxy stream.
    pub fn proxy_write(&self, stream_id: u64, data: &[u8]) -> Result<(), String> {
        let encoded = base64::engine::general_purpose::STANDARD.encode(data);
        self.call(
            "proxy.write",
            serde_json::json!({"stream_id": stream_id, "data_base64": encoded}),
        )?;
        Ok(())
    }

    /// Close a proxy stream.
    pub fn proxy_close(&self, stream_id: u64) -> Result<(), String> {
        let _ = self.call("proxy.close", serde_json::json!({"stream_id": stream_id}));
        self.stream_subs
            .lock()
            .expect("mutex poisoned")
            .remove(&stream_id);
        Ok(())
    }

    /// Subscribe to push events for a proxy stream.
    /// Returns a receiver that will get StreamEvent messages.
    pub fn proxy_subscribe(&self, stream_id: u64) -> Result<mpsc::Receiver<StreamEvent>, String> {
        // First, tell the daemon we want push events
        self.call(
            "proxy.stream.subscribe",
            serde_json::json!({"stream_id": stream_id}),
        )?;

        let (tx, rx) = mpsc::channel();
        self.stream_subs
            .lock()
            .expect("mutex poisoned")
            .insert(stream_id, tx);
        Ok(rx)
    }

    /// Shut down the SSH process.
    pub fn shutdown(&self) {
        self.alive.store(false, Ordering::Release);
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
        }
    }
}

impl Drop for RemoteRpcClient {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rpc_error_display() {
        let err = RpcError {
            code: "not_found".to_string(),
            message: "stream not found".to_string(),
        };
        assert_eq!(format!("{}", err), "not_found: stream not found");
    }
}
