//! Keyboard shortcut configuration — persistent keybindings.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A keyboard shortcut binding.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Keybinding {
    /// GTK key name (e.g., "t", "d", "f", "1")
    pub key: String,
    /// Whether Ctrl is required.
    pub ctrl: bool,
    /// Whether Shift is required.
    pub shift: bool,
    /// Whether Alt is required.
    pub alt: bool,
}

impl Keybinding {
    pub fn ctrl_shift(key: &str) -> Self {
        Self {
            key: key.to_string(),
            ctrl: true,
            shift: true,
            alt: false,
        }
    }

    pub fn ctrl(key: &str) -> Self {
        Self {
            key: key.to_string(),
            ctrl: true,
            shift: false,
            alt: false,
        }
    }

    /// Format as a human-readable string for display.
    pub fn display(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.alt {
            parts.push("Alt");
        }
        parts.push(&self.key);
        parts.join("+")
    }
}

/// All configurable shortcut actions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ShortcutConfig {
    /// Action name → keybinding map.
    pub bindings: HashMap<String, Keybinding>,
}

impl Default for ShortcutConfig {
    fn default() -> Self {
        let mut bindings = HashMap::new();

        // Workspace management
        bindings.insert(
            "workspace.new".into(),
            Keybinding::ctrl_shift("T"),
        );
        bindings.insert(
            "workspace.close".into(),
            Keybinding::ctrl_shift("W"),
        );
        bindings.insert(
            "workspace.latest_unread".into(),
            Keybinding::ctrl_shift("U"),
        );
        bindings.insert(
            "workspace.rename".into(),
            Keybinding::ctrl_shift("R"),
        );
        bindings.insert(
            "workspace.move_up".into(),
            Keybinding::ctrl_shift("Page_Up"),
        );
        bindings.insert(
            "workspace.move_down".into(),
            Keybinding::ctrl_shift("Page_Down"),
        );

        // Pane management
        bindings.insert(
            "pane.close".into(),
            Keybinding::ctrl_shift("Q"),
        );
        bindings.insert(
            "pane.split_horizontal".into(),
            Keybinding::ctrl_shift("D"),
        );
        bindings.insert(
            "pane.split_vertical".into(),
            Keybinding::ctrl_shift("E"),
        );
        bindings.insert(
            "pane.focus_prev".into(),
            Keybinding::ctrl_shift("bracketleft"),
        );
        bindings.insert(
            "pane.focus_next".into(),
            Keybinding::ctrl_shift("bracketright"),
        );

        // UI toggles
        bindings.insert(
            "find".into(),
            Keybinding::ctrl("f"),
        );
        bindings.insert(
            "find.next".into(),
            Keybinding::ctrl("g"),
        );
        bindings.insert(
            "find.previous".into(),
            Keybinding::ctrl_shift("G"),
        );
        bindings.insert(
            "notifications.toggle".into(),
            Keybinding::ctrl_shift("I"),
        );
        bindings.insert(
            "settings".into(),
            Keybinding::ctrl("comma"),
        );

        // Terminal font size
        bindings.insert(
            "font.increase".into(),
            Keybinding::ctrl("equal"),
        );
        bindings.insert(
            "font.decrease".into(),
            Keybinding::ctrl("minus"),
        );
        bindings.insert(
            "font.reset".into(),
            Keybinding::ctrl("0"),
        );

        // Clear scrollback
        bindings.insert(
            "surface.clear".into(),
            Keybinding::ctrl("k"),
        );

        // Browser-specific splits
        bindings.insert(
            "browser.split_horizontal".into(),
            Keybinding {
                key: "d".to_string(),
                ctrl: true,
                shift: false,
                alt: true,
            },
        );
        bindings.insert(
            "browser.split_vertical".into(),
            Keybinding {
                key: "e".to_string(),
                ctrl: true,
                shift: false,
                alt: true,
            },
        );

        // Close other pane tabs
        bindings.insert(
            "tab.close_others".into(),
            Keybinding {
                key: "W".to_string(),
                ctrl: true,
                shift: true,
                alt: true,
            },
        );

        // Browser console toggle
        bindings.insert(
            "browser.console_toggle".into(),
            Keybinding {
                key: "c".to_string(),
                ctrl: true,
                shift: false,
                alt: true,
            },
        );

        Self { bindings }
    }
}

impl ShortcutConfig {
    /// Get the keybinding for an action.
    pub fn get(&self, action: &str) -> Option<&Keybinding> {
        self.bindings.get(action)
    }

    /// Check if a key event matches any shortcut. Returns the action name.
    pub fn match_event(&self, key_name: &str, ctrl: bool, shift: bool, alt: bool) -> Option<&str> {
        self.bindings.iter().find_map(|(action, binding)| {
            if binding.key == key_name
                && binding.ctrl == ctrl
                && binding.shift == shift
                && binding.alt == alt
            {
                Some(action.as_str())
            } else {
                None
            }
        })
    }
}

/// Load shortcut config from disk.
pub fn load() -> ShortcutConfig {
    let path = super::config_dir().join("shortcuts.json");
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => ShortcutConfig::default(),
    }
}

/// Save shortcut config to disk.
pub fn save(config: &ShortcutConfig) -> Result<(), std::io::Error> {
    let dir = super::config_dir();
    std::fs::create_dir_all(&dir)?;

    let path = dir.join("shortcuts.json");
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| std::io::Error::other(e))?;
    std::fs::write(path, json)
}
