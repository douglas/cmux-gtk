//! Remote session controller — manages the lifecycle of a remote daemon connection.
//!
//! Orchestrates: bootstrap → RPC connect → proxy tunnel → state tracking.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use super::proxy::ProxyTunnel;
use super::rpc::RemoteRpcClient;

/// Remote workspace configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteConfig {
    pub destination: String,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub identity: Option<String>,
    #[serde(default)]
    pub ssh_options: Vec<String>,
    /// Path to the daemon binary on the remote host.
    #[serde(default)]
    pub remote_daemon_path: Option<String>,
}

impl RemoteConfig {
    /// Build SSH arguments from this config.
    pub fn ssh_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(port) = self.port {
            args.push("-p".to_string());
            args.push(port.to_string());
        }
        if let Some(ref identity) = self.identity {
            args.push("-i".to_string());
            args.push(identity.clone());
        }
        // Only pass ssh_options that look like valid Key=Value pairs
        // to prevent injection of arbitrary SSH flags from tampered session files.
        for opt in &self.ssh_options {
            if opt.contains('=') && !opt.starts_with('-') && opt.len() < 256 {
                args.push("-o".to_string());
                args.push(opt.clone());
            } else {
                tracing::warn!(opt, "Skipping invalid SSH option from session config");
            }
        }
        args.push(self.destination.clone());
        args
    }

    /// The effective daemon path on the remote host.
    pub fn daemon_path(&self) -> &str {
        self.remote_daemon_path
            .as_deref()
            .unwrap_or("~/.cmux/bin/cmuxd-remote")
    }
}

/// Remote connection state.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RemoteState {
    #[default]
    Disconnected,
    Connecting,
    Connected {
        /// Local proxy port for browser panels.
        proxy_port: u16,
        /// Daemon version from hello response.
        daemon_version: String,
    },
    Error(String),
}

/// Manages the lifecycle of a single remote daemon connection.
pub struct RemoteSessionController {
    pub config: RemoteConfig,
    pub state: RemoteState,
    rpc: Option<Arc<RemoteRpcClient>>,
    proxy: Option<ProxyTunnel>,
}

impl RemoteSessionController {
    pub fn new(config: RemoteConfig) -> Self {
        Self {
            config,
            state: RemoteState::Disconnected,
            rpc: None,
            proxy: None,
        }
    }

    /// Attempt to connect to the remote daemon and start the proxy tunnel.
    ///
    /// If `auto_bootstrap` is true, probes the remote platform and uploads
    /// the daemon binary if missing before connecting.
    pub fn start(&mut self) -> Result<(), String> {
        self.state = RemoteState::Connecting;

        let ssh_args = self.config.ssh_args();

        // Bootstrap: probe platform, upload daemon if needed
        let daemon_path = if self.config.remote_daemon_path.is_some() {
            self.config.daemon_path().to_string()
        } else {
            match super::bootstrap::bootstrap_daemon(&ssh_args) {
                Ok(path) => {
                    self.config.remote_daemon_path = Some(path.clone());
                    path
                }
                Err(e) => {
                    self.state = RemoteState::Error(format!("Bootstrap failed: {}", e));
                    return Err(e);
                }
            }
        };

        tracing::info!(
            destination = %self.config.destination,
            daemon_path = %daemon_path,
            "Connecting to remote daemon"
        );

        // Connect RPC client
        let rpc = RemoteRpcClient::new(&ssh_args, &daemon_path)?;

        // Hello handshake
        let hello = rpc.hello().inspect_err(|e| {
            self.state = RemoteState::Error(e.clone());
        })?;

        tracing::info!(
            name = %hello.name,
            version = %hello.version,
            capabilities = ?hello.capabilities,
            "Remote daemon connected"
        );

        let rpc = Arc::new(rpc);

        // Start proxy tunnel
        let proxy = ProxyTunnel::start(Arc::clone(&rpc)).inspect_err(|e| {
            self.state = RemoteState::Error(e.clone());
        })?;

        let proxy_port = proxy.port();
        tracing::info!(proxy_port, "Proxy tunnel started");

        self.state = RemoteState::Connected {
            proxy_port,
            daemon_version: hello.version,
        };
        self.rpc = Some(rpc);
        self.proxy = Some(proxy);

        Ok(())
    }

    /// Disconnect from the remote daemon and stop the proxy.
    pub fn stop(&mut self) {
        if let Some(proxy) = self.proxy.take() {
            proxy.stop();
        }
        if let Some(rpc) = self.rpc.take() {
            rpc.shutdown();
        }
        self.state = RemoteState::Disconnected;
        tracing::info!(destination = %self.config.destination, "Remote session stopped");
    }

    /// Disconnect and reconnect.
    pub fn reconnect(&mut self) -> Result<(), String> {
        self.stop();
        self.start()
    }

    /// The local proxy port, if connected.
    pub fn proxy_port(&self) -> Option<u16> {
        match &self.state {
            RemoteState::Connected { proxy_port, .. } => Some(*proxy_port),
            _ => None,
        }
    }
}

impl Drop for RemoteSessionController {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Thread-safe wrapper for RemoteSessionController.
pub type SharedRemoteSession = Arc<Mutex<RemoteSessionController>>;
