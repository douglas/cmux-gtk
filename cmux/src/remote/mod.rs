//! Remote SSH workspace support.
//!
//! Manages the lifecycle of remote daemon connections, proxy tunnels,
//! and CLI relay servers for SSH-based workspaces.

pub mod proxy;
pub mod rpc;
pub mod session;
