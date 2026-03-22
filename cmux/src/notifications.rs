//! Notification store and desktop notification integration.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A notification from a terminal or agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: Uuid,
    pub title: String,
    pub body: String,
    pub source_workspace_id: Option<Uuid>,
    pub source_panel_id: Option<Uuid>,
    pub timestamp: f64,
    pub is_read: bool,
}

/// Notification store — keeps track of all notifications.
#[derive(Debug, Default)]
pub struct NotificationStore {
    notifications: Vec<Notification>,
}

const MAX_NOTIFICATIONS: usize = 500;

impl NotificationStore {
    pub fn new() -> Self {
        Self {
            notifications: Vec::new(),
        }
    }

    /// Add a notification and optionally send a desktop notification.
    pub fn add(
        &mut self,
        title: &str,
        body: &str,
        workspace_id: Option<Uuid>,
        panel_id: Option<Uuid>,
        send_desktop: bool,
    ) -> Uuid {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        let title = crate::model::workspace::truncate_str(title, 1024);
        let body = crate::model::workspace::truncate_str(body, 8192);

        let notification = Notification {
            id: Uuid::new_v4(),
            title: title.to_string(),
            body: body.to_string(),
            source_workspace_id: workspace_id,
            source_panel_id: panel_id,
            timestamp: now,
            is_read: false,
        };

        let id = notification.id;

        if send_desktop {
            send_desktop_notification(title, body);
        }

        if self.notifications.len() >= MAX_NOTIFICATIONS {
            self.notifications.drain(..self.notifications.len() / 4);
        }

        self.notifications.push(notification);
        id
    }

    /// Get all notifications.
    pub fn all(&self) -> &[Notification] {
        &self.notifications
    }

    /// Get unread count.
    #[allow(dead_code)]
    pub fn unread_count(&self) -> usize {
        self.notifications.iter().filter(|n| !n.is_read).count()
    }

    /// Get unread count for a specific workspace.
    #[allow(dead_code)]
    pub fn unread_count_for_workspace(&self, workspace_id: Uuid) -> usize {
        self.notifications
            .iter()
            .filter(|n| !n.is_read && n.source_workspace_id == Some(workspace_id))
            .count()
    }

    /// Mark a notification as read.
    pub fn mark_read(&mut self, id: Uuid) {
        if let Some(n) = self.notifications.iter_mut().find(|n| n.id == id) {
            n.is_read = true;
        }
    }

    /// Mark all notifications for a workspace as read.
    pub fn mark_workspace_read(&mut self, workspace_id: Uuid) {
        for notification in &mut self.notifications {
            if notification.source_workspace_id == Some(workspace_id) {
                notification.is_read = true;
            }
        }
    }

    /// Mark all notifications as read.
    #[allow(dead_code)]
    pub fn mark_all_read(&mut self) {
        for n in &mut self.notifications {
            n.is_read = true;
        }
    }

    /// Clear all notifications.
    pub fn clear(&mut self) {
        self.notifications.clear();
    }
}

/// Send a desktop notification using gio::Notification, optionally playing a sound.
fn send_desktop_notification(title: &str, body: &str) {
    let title = title.to_string();
    let body = body.to_string();
    let settings = crate::settings::load();

    glib::MainContext::default().invoke(move || {
        let notification = gio::Notification::new(&title);
        notification.set_body(Some(&body));

        if let Some(app) = gio::Application::default() {
            use gio::prelude::ApplicationExt;
            app.send_notification(None, &notification);
        } else {
            tracing::debug!(
                title = %title,
                "Desktop notification unavailable; body omitted"
            );
        }

        // Play notification sound if enabled
        if settings.notifications.sound_enabled {
            play_notification_sound(&settings.notifications.sound_name);
        }
    });
}

/// Play a notification sound based on the configured sound name.
fn play_notification_sound(sound: &crate::settings::NotificationSound) {
    use crate::settings::NotificationSound;

    match sound {
        NotificationSound::Default => {
            // Use the desktop bell (simplest, always available)
            use gtk4::prelude::DisplayExt;
            if let Some(display) = gdk4::Display::default() {
                display.beep();
            }
        }
        NotificationSound::None => {}
        NotificationSound::Theme(name) => {
            play_theme_sound(name);
        }
        NotificationSound::File(path) => {
            play_sound_file(path);
        }
    }
}

/// Play a sound from the freedesktop sound theme using canberra-gtk-play or paplay.
fn play_theme_sound(name: &str) {
    // Sanitize the name: only allow alphanumeric, dashes, underscores
    let safe_name: String = name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .take(64)
        .collect();
    if safe_name.is_empty() {
        return;
    }

    // Try canberra-gtk-play first (standard on GNOME/GTK desktops)
    std::thread::spawn(move || {
        let result = std::process::Command::new("canberra-gtk-play")
            .arg("-i")
            .arg(&safe_name)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();

        if result.is_err() || result.is_ok_and(|s| !s.success()) {
            // Fallback: try paplay with the theme sound
            // XDG sound themes store files under /usr/share/sounds/
            let _ = std::process::Command::new("paplay")
                .arg(format!(
                    "/usr/share/sounds/freedesktop/stereo/{safe_name}.oga"
                ))
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    });
}

/// Play a custom sound file (WAV, OGG, OGA).
fn play_sound_file(path: &str) {
    let path = path.to_string();
    std::thread::spawn(move || {
        // Try paplay (PulseAudio), then pw-play (PipeWire), then aplay (ALSA)
        let players = ["paplay", "pw-play", "aplay"];
        for player in &players {
            if std::process::Command::new(player)
                .arg(&path)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
            {
                return;
            }
        }
        tracing::warn!(path = %path, "No audio player found for notification sound");
    });
}
