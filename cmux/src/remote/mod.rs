//! Remote SSH workspace support.
//!
//! Manages the lifecycle of remote daemon connections, proxy tunnels,
//! and CLI relay servers for SSH-based workspaces.
//!
//! This module is scaffolded and ready to be wired into the UI.

#[allow(dead_code)]
pub mod bootstrap;
#[allow(dead_code)]
pub mod proxy;
#[allow(dead_code)]
pub mod relay;
#[allow(dead_code)]
pub mod rpc;
#[allow(dead_code)]
pub mod session;
