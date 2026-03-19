//! Notification panel — list of all notifications, toggled from sidebar.

use std::rc::Rc;

use gtk4::prelude::*;

use crate::app::{lock_or_recover, AppState};

/// Notification panel widgets.
#[derive(Clone)]
pub struct NotificationsPanel {
    pub root: gtk4::Box,
    list_box: gtk4::ListBox,
}

/// Create the notification panel widget.
pub fn create_notifications_panel(state: &Rc<AppState>) -> NotificationsPanel {
    let root = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    root.add_css_class("sidebar");

    // Header with title and clear-all button
    let header = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    header.set_margin_start(12);
    header.set_margin_end(12);
    header.set_margin_top(8);
    header.set_margin_bottom(4);

    let title = gtk4::Label::new(Some("Notifications"));
    title.add_css_class("title-4");
    title.set_hexpand(true);
    title.set_halign(gtk4::Align::Start);
    header.append(&title);

    let clear_btn = gtk4::Button::with_label("Clear All");
    clear_btn.add_css_class("flat");
    clear_btn.add_css_class("caption");
    {
        let state = state.clone();
        clear_btn.connect_clicked(move |_| {
            lock_or_recover(&state.shared.notifications).clear();
            state.shared.notify_ui_refresh();
        });
    }
    header.append(&clear_btn);
    root.append(&header);

    let scrolled = gtk4::ScrolledWindow::new();
    scrolled.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scrolled.set_vexpand(true);

    let list_box = gtk4::ListBox::new();
    list_box.set_selection_mode(gtk4::SelectionMode::None);
    list_box.add_css_class("navigation-sidebar");
    scrolled.set_child(Some(&list_box));
    root.append(&scrolled);

    let panel = NotificationsPanel { root, list_box };
    panel.refresh(state);
    panel
}

impl NotificationsPanel {
    /// Refresh the notification list from the store.
    pub fn refresh(&self, state: &Rc<AppState>) {
        while let Some(child) = self.list_box.first_child() {
            self.list_box.remove(&child);
        }

        let notifications: Vec<_> = {
            let store = lock_or_recover(&state.shared.notifications);
            store.all().iter().rev().cloned().collect()
        };

        if notifications.is_empty() {
            let empty_label = gtk4::Label::new(Some("No notifications"));
            empty_label.add_css_class("dim-label");
            empty_label.set_margin_top(24);
            self.list_box.append(&empty_label);
            return;
        }

        for notification in &notifications {
            let row = gtk4::ListBoxRow::new();
            if notification.is_read {
                row.add_css_class("notification-row");
            } else {
                row.add_css_class("notification-row");
                row.add_css_class("notification-row-unread");
            }

            let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
            vbox.set_margin_start(12);
            vbox.set_margin_end(12);
            vbox.set_margin_top(6);
            vbox.set_margin_bottom(6);

            // Title row with timestamp
            let title_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
            let title_label = gtk4::Label::new(Some(&notification.title));
            title_label.add_css_class("notification-title");
            title_label.add_css_class("caption");
            title_label.set_hexpand(true);
            title_label.set_halign(gtk4::Align::Start);
            title_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            title_row.append(&title_label);

            let time_label = gtk4::Label::new(Some(&format_timestamp(notification.timestamp)));
            time_label.add_css_class("notification-timestamp");
            time_label.add_css_class("caption");
            title_row.append(&time_label);

            vbox.append(&title_row);

            // Body
            if !notification.body.is_empty() {
                let body_label = gtk4::Label::new(Some(&notification.body));
                body_label.set_halign(gtk4::Align::Start);
                body_label.set_wrap(true);
                body_label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
                body_label.set_max_width_chars(40);
                body_label.add_css_class("caption");
                body_label.add_css_class("dim-label");
                vbox.append(&body_label);
            }

            // Unread indicator dot
            if !notification.is_read {
                let dot = gtk4::Label::new(Some("●"));
                dot.add_css_class("accent");
                dot.set_halign(gtk4::Align::Start);
                vbox.append(&dot);
            }

            row.set_child(Some(&vbox));

            // Click to jump to source workspace
            if let Some(workspace_id) = notification.source_workspace_id {
                let notif_id = notification.id;
                let state = state.clone();
                let gesture = gtk4::GestureClick::new();
                gesture.connect_released(move |_gesture, _n, _x, _y| {
                    // Mark this notification as read
                    lock_or_recover(&state.shared.notifications).mark_read(notif_id);
                    // Select the source workspace
                    {
                        let mut tm = lock_or_recover(&state.shared.tab_manager);
                        let idx = tm
                            .iter()
                            .enumerate()
                            .find(|(_, ws)| ws.id == workspace_id)
                            .map(|(i, _)| i);
                        if let Some(idx) = idx {
                            tm.select(idx);
                        }
                    }
                    state.shared.notify_ui_refresh();
                });
                row.add_controller(gesture);
            }

            self.list_box.append(&row);
        }
    }
}

/// Format a UNIX timestamp as a relative time string.
fn format_timestamp(timestamp: f64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();
    let delta = (now - timestamp).max(0.0) as u64;

    if delta < 60 {
        "just now".to_string()
    } else if delta < 3600 {
        format!("{}m ago", delta / 60)
    } else if delta < 86400 {
        format!("{}h ago", delta / 3600)
    } else {
        format!("{}d ago", delta / 86400)
    }
}
