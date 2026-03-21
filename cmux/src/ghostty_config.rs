//! Read ghostty config values for cmux's own UI decisions.
//!
//! Ghostty loads `~/.config/ghostty/config` internally and applies it to
//! terminal rendering (fonts, themes, colors). This module reads values
//! back via `ghostty_config_get` so cmux can match its chrome (initial
//! background, split opacity, divider color) to the terminal theme.

/// Cached ghostty config values used by cmux's UI.
#[derive(Debug, Clone)]
pub struct GhosttyUiConfig {
    /// Terminal background color (r, g, b) as 0.0-1.0 floats.
    pub background: Option<(f32, f32, f32)>,
    /// Terminal background opacity (0.0-1.0).
    pub background_opacity: Option<f64>,
    /// Opacity for unfocused split panes (0.0-1.0).
    pub unfocused_split_opacity: Option<f64>,
    /// Fill color for unfocused split panes.
    pub unfocused_split_fill: Option<(f32, f32, f32)>,
    /// Split divider color.
    pub split_divider_color: Option<(f32, f32, f32)>,
}

impl GhosttyUiConfig {
    /// Read config values from a GhosttyApp's loaded config.
    pub fn from_app(app: &ghostty_gtk::app::GhosttyApp) -> Self {
        Self {
            background: app
                .get_config_color("background")
                .map(|(r, g, b)| (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)),
            background_opacity: app.get_config_f64("background-opacity"),
            unfocused_split_opacity: app.get_config_f64("unfocused-split-opacity"),
            unfocused_split_fill: app
                .get_config_color("unfocused-split-fill")
                .map(|(r, g, b)| (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)),
            split_divider_color: app
                .get_config_color("split-divider-color")
                .map(|(r, g, b)| (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)),
        }
    }

    /// Background color as a CSS hex string (e.g., "#1a1a2e").
    pub fn background_hex(&self) -> Option<String> {
        self.background.map(|(r, g, b)| {
            format!(
                "#{:02x}{:02x}{:02x}",
                (r * 255.0) as u8,
                (g * 255.0) as u8,
                (b * 255.0) as u8,
            )
        })
    }
}

impl Default for GhosttyUiConfig {
    fn default() -> Self {
        Self {
            background: None,
            background_opacity: None,
            unfocused_split_opacity: None,
            unfocused_split_fill: None,
            split_divider_color: None,
        }
    }
}
