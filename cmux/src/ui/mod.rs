//! GTK4/libadwaita UI layer — window, sidebar, panels, dialogs, and settings.

pub mod all_surfaces_search;
#[cfg(feature = "webkit")]
pub mod browser_panel;
pub mod command_palette;
#[cfg(feature = "webkit")]
pub mod markdown_panel;
pub mod notifications_panel;
pub mod omnibar;
pub mod search_overlay;
pub mod settings;
pub mod sidebar;
pub mod split_view;
pub mod terminal_panel;
pub mod welcome;
pub mod window;
