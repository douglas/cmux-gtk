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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_store_with(n: usize) -> NotificationStore {
        let mut store = NotificationStore::new();
        for i in 0..n {
            store.add(
                &format!("title-{i}"),
                &format!("body-{i}"),
                None,
                None,
                false,
            );
        }
        store
    }

    #[test]
    fn test_add_and_all() {
        let mut store = NotificationStore::new();
        let id = store.add("hello", "world", None, None, false);
        assert_eq!(store.all().len(), 1);
        assert_eq!(store.all()[0].id, id);
        assert_eq!(store.all()[0].title, "hello");
        assert!(!store.all()[0].is_read);
    }

    #[test]
    fn test_unread_count() {
        let mut store = make_store_with(3);
        assert_eq!(store.unread_count(), 3);

        let id = store.all()[0].id;
        store.mark_read(id);
        assert_eq!(store.unread_count(), 2);
    }

    #[test]
    fn test_unread_count_for_workspace() {
        let ws1 = Uuid::new_v4();
        let ws2 = Uuid::new_v4();
        let mut store = NotificationStore::new();
        store.add("a", "b", Some(ws1), None, false);
        store.add("c", "d", Some(ws1), None, false);
        store.add("e", "f", Some(ws2), None, false);

        assert_eq!(store.unread_count_for_workspace(ws1), 2);
        assert_eq!(store.unread_count_for_workspace(ws2), 1);
    }

    #[test]
    fn test_mark_workspace_read() {
        let ws = Uuid::new_v4();
        let mut store = NotificationStore::new();
        store.add("a", "b", Some(ws), None, false);
        store.add("c", "d", Some(ws), None, false);
        store.add("e", "f", None, None, false);

        store.mark_workspace_read(ws);
        assert_eq!(store.unread_count(), 1); // only the None-workspace one
    }

    #[test]
    fn test_mark_all_read() {
        let mut store = make_store_with(5);
        assert_eq!(store.unread_count(), 5);
        store.mark_all_read();
        assert_eq!(store.unread_count(), 0);
    }

    #[test]
    fn test_clear() {
        let mut store = make_store_with(3);
        store.clear();
        assert!(store.all().is_empty());
    }

    #[test]
    fn test_eviction_on_overflow() {
        let mut store = make_store_with(MAX_NOTIFICATIONS);
        assert_eq!(store.all().len(), MAX_NOTIFICATIONS);

        // Adding one more triggers eviction of the oldest 25%
        store.add("overflow", "test", None, None, false);
        assert!(store.all().len() < MAX_NOTIFICATIONS);
        assert_eq!(store.all().last().unwrap().title, "overflow");
    }
}

/// Play a custom sound file (WAV, OGG, OGA).
fn play_sound_file(path: &str) {
    // Validate: must be a regular file with a known audio extension.
    let p = std::path::Path::new(path);
    let valid_ext = p
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e, "wav" | "ogg" | "oga" | "mp3" | "flac" | "opus"));
    if !valid_ext || !p.is_file() {
        tracing::warn!(path, "Notification sound: invalid file or extension");
        return;
    }
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
