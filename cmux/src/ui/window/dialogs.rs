//! Rename dialogs for workspaces and tabs.

use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::app::{lock_or_recover, AppState};

/// Show a dialog to rename the currently selected workspace.
pub(super) fn show_rename_dialog(
    window: &adw::ApplicationWindow,
    state: &Rc<AppState>,
    list_box: &gtk4::ListBox,
    content_box: &gtk4::Box,
    current_title: &str,
) {
    let dialog = adw::MessageDialog::new(Some(window), Some("Rename Workspace"), None);
    dialog.set_body("Enter a new name for this workspace:");

    let entry = gtk4::Entry::new();
    entry.set_text(current_title);
    entry.set_activates_default(true);
    dialog.set_extra_child(Some(&entry));

    dialog.add_response("cancel", "Cancel");
    dialog.add_response("rename", "Rename");
    dialog.set_default_response(Some("rename"));
    dialog.set_response_appearance("rename", adw::ResponseAppearance::Suggested);

    let state = state.clone();
    let list_box = list_box.clone();
    let content_box = content_box.clone();
    dialog.connect_response(None, move |dialog, response| {
        if response == "rename" {
            let entry = dialog
                .extra_child()
                .and_then(|w| w.downcast::<gtk4::Entry>().ok());
            if let Some(entry) = entry {
                let new_name = entry.text().to_string();
                if !new_name.is_empty() {
                    let mut tm = lock_or_recover(&state.shared.tab_manager);
                    if let Some(ws) = tm.selected_mut() {
                        ws.custom_title = Some(new_name);
                    }
                    drop(tm);
                    super::refresh_ui(&list_box, &content_box, &state);
                }
            }
        }
    });

    dialog.present();
}

/// Show a dialog to rename a specific panel tab.
pub fn show_rename_tab_dialog(
    window: &adw::ApplicationWindow,
    state: &Rc<AppState>,
    panel_id: uuid::Uuid,
) {
    let current_title = {
        let tm = lock_or_recover(&state.shared.tab_manager);
        tm.find_workspace_with_panel(panel_id)
            .and_then(|ws| ws.panels.get(&panel_id))
            .map(|p| p.display_title().to_string())
            .unwrap_or_default()
    };

    let dialog = gtk4::Window::builder()
        .transient_for(window)
        .modal(true)
        .title("Rename Tab")
        .default_width(320)
        .build();

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    vbox.set_margin_start(16);
    vbox.set_margin_end(16);
    vbox.set_margin_top(16);
    vbox.set_margin_bottom(16);

    let entry = gtk4::Entry::new();
    entry.set_text(&current_title);
    entry.set_activates_default(true);
    vbox.append(&entry);

    let btn_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    btn_box.set_halign(gtk4::Align::End);

    let cancel_btn = gtk4::Button::with_label("Cancel");
    let ok_btn = gtk4::Button::with_label("Rename");
    ok_btn.add_css_class("suggested-action");
    btn_box.append(&cancel_btn);
    btn_box.append(&ok_btn);
    vbox.append(&btn_box);

    dialog.set_child(Some(&vbox));

    {
        let dialog = dialog.clone();
        cancel_btn.connect_clicked(move |_| dialog.close());
    }

    {
        let state = state.clone();
        let dialog = dialog.clone();
        let entry = entry.clone();
        ok_btn.connect_clicked(move |_| {
            let new_title = entry.text().to_string();
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            if let Some(ws) = tm.find_workspace_with_panel_mut(panel_id) {
                if let Some(panel) = ws.panels.get_mut(&panel_id) {
                    if new_title.is_empty() {
                        panel.custom_title = None;
                    } else {
                        panel.custom_title = Some(new_title);
                    }
                }
            }
            drop(tm);
            state.shared.notify_ui_refresh();
            dialog.close();
        });
    }

    // Enter activates OK
    {
        let ok_btn = ok_btn.clone();
        entry.connect_activate(move |_| {
            ok_btn.emit_clicked();
        });
    }

    // Escape closes
    let key_controller = gtk4::EventControllerKey::new();
    {
        let dialog = dialog.clone();
        key_controller.connect_key_pressed(move |_, keyval, _, _| {
            if keyval == gdk4::Key::Escape {
                dialog.close();
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
    }
    dialog.add_controller(key_controller);

    dialog.present();
    entry.grab_focus();
    entry.select_region(0, -1);
}

/// Show a dialog to create a new SSH workspace.
pub fn show_ssh_dialog(window: &adw::ApplicationWindow, state: &Rc<AppState>) {
    let dialog = adw::MessageDialog::new(
        Some(window),
        Some("New SSH Workspace"),
        Some("Connect to a remote host via SSH"),
    );

    let group = adw::PreferencesGroup::new();

    let dest_row = adw::EntryRow::new();
    dest_row.set_title("Destination");
    dest_row.set_text("user@host");
    group.add(&dest_row);

    let port_row = adw::EntryRow::new();
    port_row.set_title("Port (optional)");
    group.add(&port_row);

    let identity_row = adw::EntryRow::new();
    identity_row.set_title("Identity file (optional)");
    identity_row.set_text("");
    group.add(&identity_row);

    dialog.set_extra_child(Some(&group));

    dialog.add_response("cancel", "Cancel");
    dialog.add_response("connect", "Connect");
    dialog.set_default_response(Some("connect"));
    dialog.set_response_appearance("connect", adw::ResponseAppearance::Suggested);

    let state = state.clone();
    dialog.connect_response(None, move |_dialog, response| {
        if response != "connect" {
            return;
        }
        let destination = dest_row.text().to_string();
        if destination.is_empty() {
            return;
        }
        let port: Option<u16> = port_row
            .text()
            .to_string()
            .parse()
            .ok()
            .filter(|&p: &u16| p > 0);
        let identity = {
            let text = identity_row.text().to_string();
            if text.is_empty() { None } else { Some(text) }
        };

        let remote_config = crate::remote::session::RemoteConfig {
            destination: destination.clone(),
            port,
            identity,
            ssh_options: Vec::new(),
            remote_daemon_path: None,
        };

        let mut ws = crate::model::Workspace::new();
        ws.custom_title = Some(destination.clone());
        ws.remote_config = Some(remote_config);
        let ws_id = ws.id;

        {
            let placement = crate::settings::load().new_workspace_placement;
            let mut tm = lock_or_recover(&state.shared.tab_manager);
            tm.add_workspace_with_placement(ws, placement);
            tm.select_by_id(ws_id);
        }

        state
            .shared
            .send_ui_event(crate::app::UiEvent::RemoteConnect {
                workspace_id: ws_id,
            });
        state.shared.notify_ui_refresh();
    });

    dialog.present();
}
