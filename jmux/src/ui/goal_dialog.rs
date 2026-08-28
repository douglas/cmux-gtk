//! "Run goal" — the dialog that starts a goal or a graph from inside the app.
//!
//! Typing a goal at the CLI still runs it straight away. This dialog is the
//! other door, for when you want to see or change three things first:
//!
//! 1. **Permission** — ask first, accept edits (the default), or auto. Auto
//!    sends `bypassPermissions` and stays per-run: it is never written back
//!    to the settings file, which refuses it as a default on purpose.
//! 2. **Roles** — who does the work, on which model, at what effort. A role
//!    is a runner (`settings.goal.runners`) under the name people use for it.
//! 3. **Instructions** — the text jmux actually sends the agent, editable,
//!    with the shipped default one button away (`goal::Guidance`).
//!
//! Opened by the command palette, by `jmux goal --configure`, and before
//! every goal when `goal.confirm_before_run` is on.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::app::{lock_or_recover, AppState, UiEvent};
use crate::goal::Guidance;
use crate::settings::GoalRunner;

/// The three permission choices, in the order they appear, with the value
/// each sends and the line shown underneath.
const MODES: [(&str, &str, &str); 3] = [
    (
        "Ask first",
        "supervised",
        "Asks before it writes a file or runs a command.",
    ),
    (
        "Accept edits",
        "acceptEdits",
        "Writes files without asking. Asks before commands outside the allowed list.",
    ),
    (
        "Auto",
        "bypassPermissions",
        "No prompts at all. This run only — never saved as your default.",
    ),
];

/// Runner name edited when the settings file names no default. Written to
/// `goal.runners.default` so the change is visible and reversible in the
/// config file like any other.
const IMPLIED_RUNNER: &str = "default";

/// The role being edited and the runner behind it, held while the dialog is
/// open and written to the settings file on Run.
struct Draft {
    /// Name of the runner each role uses.
    runner_name: String,
    runner: GoalRunner,
}

impl Draft {
    fn load(settings: &crate::settings::GoalSettings) -> Self {
        let (runner_name, runner) = crate::goal::resolve_runner_by_name("", settings)
            .unwrap_or_else(|_| (IMPLIED_RUNNER.to_string(), GoalRunner::default()));
        let runner_name = if settings.default_runner.is_empty() {
            IMPLIED_RUNNER.to_string()
        } else {
            runner_name
        };
        Self {
            runner_name,
            runner,
        }
    }

    /// "sonnet · high", or "default model" when nothing is pinned.
    fn summary(&self) -> String {
        let model = if self.runner.model.is_empty() {
            "default model".to_string()
        } else {
            self.runner.model.clone()
        };
        if self.runner.effort.is_empty() {
            model
        } else {
            format!("{model} · {}", self.runner.effort)
        }
    }

    /// Persist the role so the next goal uses it too. The permission mode is
    /// deliberately not written: auto stays a per-run choice.
    fn save(&self) {
        let mut settings = crate::settings::load();
        settings
            .goal
            .runners
            .insert(self.runner_name.clone(), self.runner.clone());
        if settings.goal.default_runner.is_empty() {
            settings.goal.default_runner = self.runner_name.clone();
        }
        let _ = crate::settings::save(&settings);
    }
}

/// Show the dialog. `prefill` seeds the goal box (from `jmux goal
/// --configure "..."`); an empty box just waits for typing.
pub fn show_goal_dialog(
    window: &adw::ApplicationWindow,
    state: &Rc<AppState>,
    prefill: Option<String>,
) {
    let settings = crate::settings::load();
    let draft = Rc::new(RefCell::new(Draft::load(&settings.goal)));

    let dialog = adw::Dialog::new();
    dialog.set_title("Run goal");
    dialog.set_content_width(640);
    dialog.set_content_height(680);

    let nav = adw::NavigationView::new();

    // ---- main page ----------------------------------------------------
    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.set_show_end_title_buttons(false);

    let cancel = gtk4::Button::with_label("Cancel");
    let run = gtk4::Button::with_label("Run");
    run.add_css_class("suggested-action");
    header.pack_start(&cancel);
    header.pack_end(&run);
    toolbar.add_top_bar(&header);

    let page = adw::PreferencesPage::new();

    // Goal text.
    let goal_group = adw::PreferencesGroup::new();
    goal_group.set_title("Goal");
    goal_group.set_description(Some("What you want done, in your own words."));
    let goal_view = gtk4::TextView::new();
    goal_view.set_wrap_mode(gtk4::WrapMode::WordChar);
    goal_view.set_top_margin(8);
    goal_view.set_bottom_margin(8);
    goal_view.set_left_margin(10);
    goal_view.set_right_margin(10);
    goal_view.buffer().set_text(prefill.as_deref().unwrap_or(""));
    let goal_frame = gtk4::Frame::new(None);
    goal_frame.set_height_request(96);
    goal_frame.set_child(Some(&goal_view));
    goal_group.add(&goal_frame);
    page.add(&goal_group);

    // Permission.
    let perm_group = adw::PreferencesGroup::new();
    perm_group.set_title("Permission");
    let perm_row = adw::ComboRow::new();
    perm_row.set_title("How much it may do on its own");
    let perm_model = gtk4::StringList::new(&MODES.map(|(label, _, _)| label));
    perm_row.set_model(Some(&perm_model));
    let default_mode = settings.goal.permission_mode.clone();
    let selected = MODES
        .iter()
        .position(|(_, value, _)| *value == default_mode)
        .unwrap_or(1);
    perm_row.set_selected(selected as u32);
    perm_row.set_subtitle(MODES[selected].2);
    perm_row.connect_selected_notify(|row| {
        let i = (row.selected() as usize).min(MODES.len() - 1);
        row.set_subtitle(MODES[i].2);
    });
    perm_group.add(&perm_row);
    page.add(&perm_group);

    // Roles.
    let roles_group = adw::PreferencesGroup::new();
    roles_group.set_title("Roles");
    roles_group.set_description(Some("Click a role to change its model or read its instructions."));

    let worker_row = adw::ActionRow::new();
    worker_row.set_activatable(true);
    worker_row.set_title(Guidance::Worker.label());
    worker_row.set_subtitle(Guidance::Worker.blurb());
    let worker_value = gtk4::Label::new(Some(&draft.borrow().summary()));
    worker_value.add_css_class("dim-label");
    worker_row.add_suffix(&worker_value);
    worker_row.add_suffix(&gtk4::Image::from_icon_name("go-next-symbolic"));
    roles_group.add(&worker_row);

    // Only meaningful for a graph run, so it follows the "plan first" switch.
    let orch_row = adw::ActionRow::new();
    orch_row.set_activatable(true);
    orch_row.set_title(Guidance::Orchestrator.label());
    orch_row.set_subtitle(Guidance::Orchestrator.blurb());
    orch_row.add_suffix(&gtk4::Image::from_icon_name("go-next-symbolic"));
    orch_row.set_visible(false);
    roles_group.add(&orch_row);
    page.add(&roles_group);

    // How it runs.
    let run_group = adw::PreferencesGroup::new();
    run_group.set_title("How it runs");

    let plan_row = adw::SwitchRow::new();
    plan_row.set_title("Split into a plan first");
    plan_row.set_subtitle("Runs a graph: one agent writes a plan, you approve it, then the work starts");
    plan_row.set_active(false);
    run_group.add(&plan_row);

    let iter_row = adw::SpinRow::new(
        Some(&gtk4::Adjustment::new(
            settings.goal.max_iterations.clamp(1, 50) as f64,
            1.0,
            50.0,
            1.0,
            1.0,
            0.0,
        )),
        1.0,
        0,
    );
    iter_row.set_title("Tries before it gives up");
    iter_row.set_subtitle("It starts again while it keeps reporting that it is stuck");
    run_group.add(&iter_row);

    let background_row = adw::SwitchRow::new();
    background_row.set_title("Run in the background");
    background_row.set_subtitle("Starts without taking you away from what you are doing");
    background_row.set_active(false);
    run_group.add(&background_row);
    page.add(&run_group);

    // The answer to "straight to running, or this dialog?".
    let pref_group = adw::PreferencesGroup::new();
    let confirm_row = adw::SwitchRow::new();
    confirm_row.set_title("Show this before every goal");
    confirm_row.set_subtitle("Off: typing a goal runs it, and --configure opens this");
    confirm_row.set_active(settings.goal.confirm_before_run);
    pref_group.add(&confirm_row);
    page.add(&pref_group);

    toolbar.set_content(Some(&page));
    let main_page = adw::NavigationPage::new(&toolbar, "Run goal");
    nav.add(&main_page);
    dialog.set_child(Some(&nav));

    // Showing the orchestrator only when it has something to do.
    let orch_row_c = orch_row.clone();
    let run_btn = run.clone();
    plan_row.connect_active_notify(move |row| {
        orch_row_c.set_visible(row.is_active());
        // The button says what pressing it does: a graph stops at the plan.
        run_btn.set_label(if row.is_active() { "Plan" } else { "Run" });
    });

    // ---- role subpages ------------------------------------------------
    {
        let nav_c = nav.clone();
        let draft_c = draft.clone();
        let value_label = worker_value.clone();
        worker_row.connect_activated(move |_| {
            let page = role_page(Guidance::Worker, Some(&draft_c), Some(&value_label));
            nav_c.push(&page);
        });
    }
    {
        let nav_c = nav.clone();
        orch_row.connect_activated(move |_| {
            let page = role_page(Guidance::Orchestrator, None, None);
            nav_c.push(&page);
        });
    }

    // ---- responses ----------------------------------------------------
    {
        let dialog_c = dialog.clone();
        cancel.connect_clicked(move |_| {
            dialog_c.close();
        });
    }
    {
        let dialog_c = dialog.clone();
        let state = state.clone();
        let goal_buffer = goal_view.buffer();
        let draft_c = draft.clone();
        run.connect_clicked(move |_| {
            let (start, end) = goal_buffer.bounds();
            let goal_text = goal_buffer.text(&start, &end, false).to_string();
            let goal_text = goal_text.trim().to_string();
            if goal_text.is_empty() {
                state
                    .shared
                    .send_ui_event(UiEvent::ShowToast("Type a goal first".into()));
                return;
            }

            // Persist what the user changed, minus the permission mode.
            draft_c.borrow().save();
            let mut settings = crate::settings::load();
            if settings.goal.confirm_before_run != confirm_row.is_active() {
                settings.goal.confirm_before_run = confirm_row.is_active();
                let _ = crate::settings::save(&settings);
            }

            let mode = MODES[(perm_row.selected() as usize).min(MODES.len() - 1)].1;
            let cwd = current_directory(&state);
            let mut params = serde_json::json!({
                "goal_text": goal_text,
                "permission_mode": mode,
                "max_iterations": iter_row.value() as u32,
                "runner": draft_c.borrow().runner_name,
            });
            if let Some(cwd) = cwd {
                params["cwd"] = serde_json::json!(cwd);
            }

            let plan_first = plan_row.is_active();
            if !plan_first {
                params["select"] = serde_json::json!(!background_row.is_active());
            }

            let outcome = if plan_first {
                crate::goal::resolve_goal_source(&params)
                    .map_err(|(_, msg)| msg)
                    .and_then(|source| {
                        let name = source.name.clone();
                        crate::goal::graph::create_graph(&state.shared, &name, &source, &params)
                    })
            } else {
                crate::socket::v2::goal::create_goal(&state.shared, &params)
                    .map_err(|(_, msg)| msg)
            };

            match outcome {
                Ok(_) => {
                    let _ = dialog_c.close();
                    state.shared.notify_ui_refresh();
                }
                Err(e) => {
                    state
                        .shared
                        .send_ui_event(UiEvent::ShowToast(format!("Could not start: {e}")));
                }
            }
        });
    }

    dialog.present(Some(window));
    goal_view.grab_focus();
}

/// The subpage behind a role: what it runs on, and the instructions it is
/// sent. `draft` is None for the orchestrator, whose model comes from the
/// graph's own runner — only its instructions are edited here.
fn role_page(
    role: Guidance,
    draft: Option<&Rc<RefCell<Draft>>>,
    value_label: Option<&gtk4::Label>,
) -> adw::NavigationPage {
    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.set_show_end_title_buttons(false);
    toolbar.add_top_bar(&header);

    let page = adw::PreferencesPage::new();

    if let Some(draft) = draft {
        let group = adw::PreferencesGroup::new();
        group.set_title("Runs on");

        let model_row = adw::EntryRow::new();
        model_row.set_title("Model");
        model_row.set_text(&draft.borrow().runner.model);
        group.add(&model_row);

        let effort_row = adw::EntryRow::new();
        effort_row.set_title("Effort");
        effort_row.set_text(&draft.borrow().runner.effort);
        group.add(&effort_row);

        let tools_row = adw::EntryRow::new();
        tools_row.set_title("Tools it may use without asking");
        tools_row.set_text(&draft.borrow().runner.allowed_tools.join(", "));
        group.add(&tools_row);

        // Write back on every keystroke: the dialog has no Save button of its
        // own, and Run reads the draft.
        for (row, field) in [
            (&model_row, "model"),
            (&effort_row, "effort"),
            (&tools_row, "tools"),
        ] {
            let draft = draft.clone();
            let value_label = value_label.cloned();
            let field = field.to_string();
            row.connect_changed(move |row| {
                {
                    let mut d = draft.borrow_mut();
                    let text = row.text().to_string();
                    match field.as_str() {
                        "model" => d.runner.model = text.trim().to_string(),
                        "effort" => d.runner.effort = text.trim().to_string(),
                        _ => {
                            d.runner.allowed_tools = text
                                .split(',')
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                                .map(str::to_string)
                                .collect()
                        }
                    }
                }
                if let Some(label) = &value_label {
                    label.set_text(&draft.borrow().summary());
                }
            });
        }
        page.add(&group);
    }

    // Instructions.
    let group = adw::PreferencesGroup::new();
    group.set_title("Instructions sent to this agent");
    group.set_description(Some(&instructions_status(role)));

    let view = gtk4::TextView::new();
    view.set_monospace(true);
    view.set_wrap_mode(gtk4::WrapMode::WordChar);
    view.set_top_margin(8);
    view.set_bottom_margin(8);
    view.set_left_margin(10);
    view.set_right_margin(10);
    view.buffer().set_text(&role.text());

    let scroller = gtk4::ScrolledWindow::new();
    scroller.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroller.set_height_request(320);
    scroller.set_child(Some(&view));
    let frame = gtk4::Frame::new(None);
    frame.set_child(Some(&scroller));
    group.add(&frame);

    let buttons = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    buttons.set_halign(gtk4::Align::End);
    buttons.set_margin_top(8);
    let reset = gtk4::Button::with_label("Reset to default");
    let save = gtk4::Button::with_label("Save instructions");
    save.add_css_class("suggested-action");
    buttons.append(&reset);
    buttons.append(&save);
    group.add(&buttons);
    page.add(&group);

    {
        let buffer = view.buffer();
        let group_c = group.clone();
        reset.connect_clicked(move |_| {
            let _ = role.reset();
            buffer.set_text(role.default_text());
            group_c.set_description(Some(&instructions_status(role)));
        });
    }
    {
        let buffer = view.buffer();
        let group_c = group.clone();
        save.connect_clicked(move |_| {
            let (start, end) = buffer.bounds();
            let text = buffer.text(&start, &end, false).to_string();
            let _ = role.save(&text);
            group_c.set_description(Some(&instructions_status(role)));
        });
    }

    toolbar.set_content(Some(&page));
    adw::NavigationPage::new(&toolbar, role.label())
}

/// One line saying whether these instructions are the shipped ones, and
/// where an edit is kept.
fn instructions_status(role: Guidance) -> String {
    let path = role
        .override_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(no config directory)".into());
    if role.is_edited() {
        format!("Edited. Kept in {path} and used by every run until you reset it.")
    } else {
        format!("The shipped default. Saving an edit writes {path}.")
    }
}

/// The directory the goal runs in: the selected workspace's, if it has one.
fn current_directory(state: &Rc<AppState>) -> Option<String> {
    let tm = lock_or_recover(&state.shared.tab_manager);
    tm.selected()
        .map(|ws| ws.current_directory.clone())
        .filter(|d| !d.is_empty())
}
