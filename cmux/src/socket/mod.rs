//! Socket API — V1 text protocol, V2 JSON-RPC, browser automation, and auth.

pub mod auth;
#[cfg(feature = "webkit")]
pub mod browser;
pub mod server;
pub mod v1;
pub mod v2;
