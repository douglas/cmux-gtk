//! Data models — Panel, Workspace, TabManager, and layout tree types.

pub mod panel;
pub mod tab_manager;
pub mod workspace;

pub use panel::{Panel, PanelType};
pub use tab_manager::TabManager;
pub use workspace::Workspace;
