//! Notes panel — multiple editable scratchpads, grouped by scope.
//!
//! Notes are organised into colour-coded **scope groups**:
//!
//! - **Global** (blue) — notes shared across everything, not tied to a folder.
//! - **Host** (purple) — for remote SSH sessions, notes for the whole host.
//! - **Folder** (green) — notes for the current git repo root (local) or the
//!   launch directory (remote).
//!
//! Each scope can hold any number of notes; every note is a tab labelled by its
//! filename, with a small coloured dot marking its scope. A `+` button creates
//! a new auto-named note (rename by double-clicking its tab). Empty notes are
//! deleted from disk and only persisted once they have content.
//!
//! All notes are stored client-side under `<data>/jmux/notes/` in a mirror tree
//! (`global/`, `local/<abs-path>/`, `remote/<dest>/[<abs-path>/]`), so remote
//! notes never require I/O over the SSH bridge.
//!
//! When a panel is opened against a *specific* file (`notes.open --file X`),
//! that single file is shown directly with no tab bar.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4::prelude::*;

use crate::app::{lock_or_recover, AppState};

/// Debounce before writing edits to disk.
const SAVE_DEBOUNCE_MS: u64 = 800;

/// A note's scope group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    Global,
    Host,
    Folder,
}

impl Scope {
    fn dot_class(self) -> &'static str {
        match self {
            Scope::Global => "notes-dot-global",
            Scope::Host => "notes-dot-host",
            Scope::Folder => "notes-dot-folder",
        }
    }

    fn tab_class(self) -> &'static str {
        match self {
            Scope::Global => "notes-tab-global",
            Scope::Host => "notes-tab-host",
            Scope::Folder => "notes-tab-folder",
        }
    }

    fn new_label(self) -> &'static str {
        match self {
            Scope::Global => "New global note",
            Scope::Host => "New host note",
            Scope::Folder => "New folder note",
        }
    }
}

/// A scope group plus the directory its notes live in.
#[derive(Debug, Clone)]
struct ScopeInfo {
    scope: Scope,
    dir: PathBuf,
}

/// Context resolved from the owning workspace, used to pick scope groups.
#[derive(Debug, Default, Clone)]
pub struct NotesContext {
    /// Launch directory (local absolute path, or the remote pwd for SSH).
    pub cwd: Option<String>,
    /// Remote SSH destination when this is a remote session.
    pub remote: Option<String>,
}

/// Look up the workspace owning `panel_id` and extract its notes context.
///
/// Safe to call during widget construction: the UI clones workspace data out of
/// the tab-manager lock before building, so the lock is free here.
pub fn resolve_context(panel_id: uuid::Uuid, state: &Rc<AppState>) -> NotesContext {
    let tm = lock_or_recover(&state.shared.tab_manager);
    match tm.find_workspace_with_panel(panel_id) {
        Some(ws) => NotesContext {
            cwd: Some(ws.current_directory.clone()).filter(|d| !d.is_empty()),
            remote: ws
                .remote_config
                .as_ref()
                .map(|c| c.destination.clone())
                .filter(|d| !d.is_empty()),
        },
        None => NotesContext::default(),
    }
}

/// Default (global) notes file: a configured path, else `<data>/jmux/notes.md`.
/// Used for the legacy single-file `notes.open` default.
pub fn default_notes_path() -> String {
    let configured = crate::settings::load().notes_path;
    let configured = configured.trim();
    if !configured.is_empty() {
        let expanded = if let Some(rest) = configured.strip_prefix("~/") {
            dirs::home_dir()
                .map(|h| h.join(rest))
                .unwrap_or_else(|| PathBuf::from(configured))
        } else {
            PathBuf::from(configured)
        };
        if let Some(parent) = expanded.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        return expanded.to_string_lossy().into_owned();
    }

    let dir = data_root();
    let _ = std::fs::create_dir_all(&dir);
    dir.join("notes.md").to_string_lossy().into_owned()
}

/// `<data>/jmux` — the base directory for jmux client-side data.
fn data_root() -> PathBuf {
    dirs::data_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".local/share")))
        .unwrap_or_else(std::env::temp_dir)
        .join("jmux")
}

/// Root of the per-context notes mirror tree: `<data>/jmux/notes/`.
fn notes_tree_root() -> PathBuf {
    data_root().join("notes")
}

/// Strip a leading `/` so an absolute path can be joined under a root.
fn mirror_rel(abs: &str) -> PathBuf {
    let p = Path::new(abs);
    p.strip_prefix("/").unwrap_or(p).to_path_buf()
}

/// Sanitize an SSH destination for use as a single path component.
fn sanitize_dest(dest: &str) -> String {
    dest.replace(['/', '\\'], "_")
}

/// Walk up from `start` to the nearest ancestor containing `.git`.
fn git_root(start: &str) -> Option<PathBuf> {
    let mut p = PathBuf::from(start);
    if !p.is_absolute() {
        return None;
    }
    loop {
        if p.join(".git").exists() {
            return Some(p);
        }
        if !p.pop() {
            return None;
        }
    }
}

/// Compute the scope groups (and their directories) for a context.
fn scopes(ctx: &NotesContext) -> Vec<ScopeInfo> {
    let mut v = vec![ScopeInfo {
        scope: Scope::Global,
        dir: notes_tree_root().join("global"),
    }];

    match &ctx.remote {
        Some(dest) => {
            let host_dir = notes_tree_root().join("remote").join(sanitize_dest(dest));
            if let Some(cwd) = &ctx.cwd {
                // Remote git-root detection would require I/O over the bridge;
                // key the folder scope to the remote launch directory instead.
                v.push(ScopeInfo {
                    scope: Scope::Folder,
                    dir: host_dir.join(mirror_rel(cwd)),
                });
            }
            v.push(ScopeInfo {
                scope: Scope::Host,
                dir: host_dir,
            });
        }
        None => {
            if let Some(cwd) = &ctx.cwd {
                let root = git_root(cwd).unwrap_or_else(|| PathBuf::from(cwd));
                v.push(ScopeInfo {
                    scope: Scope::Folder,
                    dir: notes_tree_root()
                        .join("local")
                        .join(mirror_rel(&root.to_string_lossy())),
                });
            }
        }
    }

    // Order tabs Global → Host → Folder regardless of construction order.
    v.sort_by_key(|s| match s.scope {
        Scope::Global => 0,
        Scope::Host => 1,
        Scope::Folder => 2,
    });
    v
}

/// One-time migration: seed the Global scope with the legacy single notes file.
fn migrate_global_note() {
    let global = notes_tree_root().join("global");
    if global.exists() {
        return;
    }
    let legacy = PathBuf::from(default_notes_path());
    if let Ok(content) = std::fs::read_to_string(&legacy) {
        if !content.trim().is_empty() {
            if std::fs::create_dir_all(&global).is_ok() {
                let name = legacy
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("notes.md");
                let _ = std::fs::write(global.join(name), content);
            }
        }
    }
}

/// List the `.md` notes in a directory, sorted by filename.
fn list_notes(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "md"))
        .collect();
    out.sort();
    out
}

/// Pick the next free `note.md` / `note-N.md` name in `dir`.
fn next_note_name(dir: &Path, reserved: &HashSet<PathBuf>) -> PathBuf {
    for i in 1..10_000 {
        let name = if i == 1 {
            "note.md".to_string()
        } else {
            format!("note-{i}.md")
        };
        let p = dir.join(&name);
        if !p.exists() && !reserved.contains(&p) {
            return p;
        }
    }
    dir.join("note.md")
}

/// Normalise a typed name into a `.md` filename, or `None` if blank.
fn sanitize_filename(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    let cleaned = trimmed.replace(['/', '\\'], "_");
    Some(if cleaned.ends_with(".md") {
        cleaned
    } else {
        format!("{cleaned}.md")
    })
}

/// Create a notes panel widget.
///
/// When `file` names a specific note (not the global default), a single editor
/// is shown for it. Otherwise scope-grouped tabs are built from `ctx`.
pub fn create_notes_widget(
    panel_id: uuid::Uuid,
    file: Option<&str>,
    ctx: NotesContext,
    is_attention_source: bool,
) -> gtk4::Widget {
    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    container.set_hexpand(true);
    container.set_vexpand(true);
    container.add_css_class("panel-shell");
    if is_attention_source {
        container.add_css_class("attention-panel");
    }
    container.set_widget_name(&panel_id.to_string());

    let global = default_notes_path();
    if let Some(path) = file.filter(|p| !p.is_empty() && *p != global) {
        // Specific file requested — single editor, no tabs.
        let reserved = Rc::new(RefCell::new(HashSet::new()));
        let notebook = gtk4::Notebook::new();
        notebook.set_show_tabs(false);
        let editor = build_note_editor(&notebook, Scope::Global, PathBuf::from(path), &reserved);
        notebook.append_page(&editor, gtk4::Widget::NONE);
        notebook.set_hexpand(true);
        notebook.set_vexpand(true);
        container.append(&notebook);
        return container.upcast();
    }

    migrate_global_note();
    let scope_infos = scopes(&ctx);

    let notebook = gtk4::Notebook::new();
    notebook.set_hexpand(true);
    notebook.set_vexpand(true);
    notebook.set_scrollable(true);

    let reserved: Rc<RefCell<HashSet<PathBuf>>> = Rc::new(RefCell::new(HashSet::new()));

    for si in &scope_infos {
        for note in list_notes(&si.dir) {
            add_note_page(&notebook, si.scope, note, &reserved);
        }
    }
    // No notes on disk yet — seed an empty, editable note in the most-specific
    // scope so the panel is usable immediately (and the tab bar + "+" show).
    if notebook.n_pages() == 0 {
        if let Some(si) = scope_infos.last() {
            new_note_in(&notebook, si, &reserved);
        }
    }
    // Default to the most specific scope's last note.
    let pages = notebook.n_pages();
    if pages > 0 {
        notebook.set_current_page(Some(pages - 1));
    }

    // ── "+" action button (create a new note) ──
    let plus = gtk4::Button::from_icon_name("list-add-symbolic");
    plus.add_css_class("flat");
    plus.set_tooltip_text(Some("New note"));
    {
        // WEAK: the "+" button is the notebook's action widget — strong
        // captures here (and in the per-item closures, which live inside a
        // popover parented to this button) cycle back to the notebook and
        // leaked the whole notes subtree per rebuild.
        let notebook = notebook.downgrade();
        let reserved = reserved.clone();
        let scope_infos = scope_infos.clone();
        plus.connect_clicked(move |btn| {
            let Some(strong_notebook) = notebook.upgrade() else {
                return;
            };
            if scope_infos.len() == 1 {
                new_note_in(&strong_notebook, &scope_infos[0], &reserved);
                return;
            }
            // Multiple scopes — let the user pick which group to add to.
            let popover = gtk4::Popover::new();
            let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
            for si in &scope_infos {
                let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
                let dot = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
                dot.add_css_class("notes-dot");
                dot.add_css_class(si.scope.dot_class());
                dot.set_valign(gtk4::Align::Center);
                row.append(&dot);
                row.append(&gtk4::Label::new(Some(si.scope.new_label())));
                let item = gtk4::Button::new();
                item.set_child(Some(&row));
                item.add_css_class("flat");
                let notebook = strong_notebook.downgrade();
                let reserved = reserved.clone();
                let si = si.clone();
                let popover_w = popover.downgrade();
                item.connect_clicked(move |_| {
                    if let Some(nb) = notebook.upgrade() {
                        new_note_in(&nb, &si, &reserved);
                    }
                    if let Some(p) = popover_w.upgrade() {
                        p.popdown();
                    }
                });
                vbox.append(&item);
            }
            popover.set_child(Some(&vbox));
            popover.set_parent(btn);
            // The popover is parented to the button and would otherwise
            // accumulate one instance per click for the button's lifetime.
            popover.connect_closed(|p| {
                let weak = p.downgrade();
                glib::idle_add_local_once(move || {
                    if let Some(p) = weak.upgrade() {
                        p.unparent();
                    }
                });
            });
            popover.popup();
        });
    }
    notebook.set_action_widget(&plus, gtk4::PackType::End);

    container.append(&notebook);
    container.upcast()
}

/// Create a new auto-named note in `si`'s scope and select it.
fn new_note_in(
    notebook: &gtk4::Notebook,
    si: &ScopeInfo,
    reserved: &Rc<RefCell<HashSet<PathBuf>>>,
) {
    let path = next_note_name(&si.dir, &reserved.borrow());
    let editor = add_note_page(notebook, si.scope, path, reserved);
    if let Some(n) = notebook.page_num(&editor) {
        notebook.set_current_page(Some(n));
    }
}

/// Build an editor page for `path`, append it to `notebook`, and return it.
fn add_note_page(
    notebook: &gtk4::Notebook,
    scope: Scope,
    path: PathBuf,
    reserved: &Rc<RefCell<HashSet<PathBuf>>>,
) -> gtk4::Widget {
    let path_ref = Rc::new(RefCell::new(path.clone()));
    reserved.borrow_mut().insert(path);

    let editor = build_note_editor_inner(notebook, &path_ref, reserved);
    let tab = build_tab_label(scope, &path_ref, reserved);
    notebook.append_page(&editor, Some(&tab));
    notebook.set_tab_reorderable(&editor, true);
    editor
}

/// Build a colour-coded tab label (dot + filename) with double-click rename.
fn build_tab_label(
    scope: Scope,
    path_ref: &Rc<RefCell<PathBuf>>,
    reserved: &Rc<RefCell<HashSet<PathBuf>>>,
) -> gtk4::Widget {
    let tab = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    tab.add_css_class(scope.tab_class());

    let dot = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    dot.add_css_class("notes-dot");
    dot.add_css_class(scope.dot_class());
    dot.set_valign(gtk4::Align::Center);
    tab.append(&dot);

    let filename = path_ref
        .borrow()
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("note.md")
        .to_string();
    let label = gtk4::Label::new(Some(&filename));
    // No ellipsize: tabs size to the full filename (the notebook is scrollable
    // for overflow). Enabling ellipsize collapses a narrow tab to just "…".
    label.set_tooltip_text(path_ref.borrow().to_str());
    tab.append(&label);

    let entry = gtk4::Entry::new();
    entry.set_visible(false);
    entry.set_width_chars(12);
    tab.append(&entry);

    // Double-click → inline rename.
    {
        let label = label.clone();
        let entry = entry.clone();
        let path_ref = path_ref.clone();
        let click = gtk4::GestureClick::new();
        click.connect_pressed(move |_, n_press, _, _| {
            if n_press != 2 {
                return;
            }
            let stem = path_ref
                .borrow()
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("note")
                .to_string();
            entry.set_text(&stem);
            label.set_visible(false);
            entry.set_visible(true);
            entry.grab_focus();
        });
        tab.add_controller(click);
    }

    let apply_rename = {
        // WEAK: this Rc is held by signal handlers ON the entry itself —
        // strong entry/label captures pinned them (and the tab) forever.
        let label = label.downgrade();
        let entry = entry.downgrade();
        let path_ref = path_ref.clone();
        let reserved = reserved.clone();
        Rc::new(move || {
            let (Some(label), Some(entry)) = (label.upgrade(), entry.upgrade()) else {
                return;
            };
            entry.set_visible(false);
            label.set_visible(true);
            let Some(name) = sanitize_filename(&entry.text()) else {
                return;
            };
            let old = path_ref.borrow().clone();
            let new = old
                .parent()
                .map(|d| d.join(&name))
                .unwrap_or_else(|| PathBuf::from(&name));
            if new == old || new.exists() || reserved.borrow().contains(&new) {
                return;
            }
            if old.exists() {
                let _ = std::fs::rename(&old, &new);
            }
            reserved.borrow_mut().remove(&old);
            reserved.borrow_mut().insert(new.clone());
            *path_ref.borrow_mut() = new.clone();
            label.set_text(&name);
            label.set_tooltip_text(new.to_str());
        })
    };
    {
        let apply_rename = apply_rename.clone();
        entry.connect_activate(move |_| apply_rename());
    }
    {
        let focus = gtk4::EventControllerFocus::new();
        focus.connect_leave(move |_| apply_rename());
        entry.add_controller(focus);
    }

    tab.upcast()
}

/// Editor page bound to a mutable path, with empty-note cleanup that also
/// removes its own tab when the note is left empty.
fn build_note_editor_inner(
    notebook: &gtk4::Notebook,
    path_ref: &Rc<RefCell<PathBuf>>,
    reserved: &Rc<RefCell<HashSet<PathBuf>>>,
) -> gtk4::Widget {
    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    container.set_hexpand(true);
    container.set_vexpand(true);

    let status = gtk4::Label::new(Some("saved"));
    let text_view = gtk4::TextView::new();
    text_view.set_editable(true);
    text_view.set_monospace(true);
    text_view.set_wrap_mode(if crate::settings::load().editor_word_wrap {
        gtk4::WrapMode::WordChar
    } else {
        gtk4::WrapMode::None
    });
    text_view.set_left_margin(8);
    text_view.set_right_margin(8);
    text_view.set_top_margin(4);
    text_view.set_bottom_margin(4);

    let buffer = text_view.buffer();
    if let Ok(content) = std::fs::read_to_string(&*path_ref.borrow()) {
        buffer.set_text(&content);
    }

    let scrolled = gtk4::ScrolledWindow::new();
    scrolled.set_policy(gtk4::PolicyType::Automatic, gtk4::PolicyType::Automatic);
    scrolled.set_hexpand(true);
    scrolled.set_vexpand(true);
    scrolled.set_child(Some(&text_view));
    container.append(&scrolled);
    // Slim status line at the bottom.
    status.add_css_class("dim-label");
    status.add_css_class("caption");
    status.set_halign(gtk4::Align::End);
    status.set_margin_end(6);
    container.append(&status);

    // Debounced auto-save: write when there's content, delete when emptied.
    let pending = Rc::new(Cell::new(false));
    {
        let path_ref = path_ref.clone();
        let status = status.clone();
        let pending = pending.clone();
        // The buffer arrives as the signal argument — capturing it strongly
        // would be a buffer → signal → closure → buffer cycle (every note's
        // text immortal per rebuild).
        buffer.connect_changed(move |buf| {
            status.set_text("editing…");
            if pending.replace(true) {
                return;
            }
            let buffer_w = buf.clone();
            let path_ref = path_ref.clone();
            let status = status.clone();
            let pending = pending.clone();
            glib::timeout_add_local_once(
                std::time::Duration::from_millis(SAVE_DEBOUNCE_MS),
                move || {
                    pending.set(false);
                    let text = buffer_text(&buffer_w);
                    let path = path_ref.borrow().clone();
                    match save_or_delete(&path, &text) {
                        Ok(true) => status.set_text("saved"),
                        Ok(false) => status.set_text("empty"),
                        Err(e) => status.set_text(&format!("save failed: {e}")),
                    }
                },
            );
        });
    }

    // On focus loss: persist content, or (when empty) delete the file and drop
    // the abandoned tab — but never the last one, so the panel stays usable.
    {
        let buffer_w = buffer.clone();
        let path_ref = path_ref.clone();
        let reserved = reserved.clone();
        // WEAK: this controller sits on the text view, a descendant of both
        // the editor container and the notebook — strong captures pinned the
        // whole notebook subtree per rebuild.
        let notebook = notebook.downgrade();
        let editor = container.downgrade();
        let focus = gtk4::EventControllerFocus::new();
        focus.connect_leave(move |_| {
            let text = buffer_text(&buffer_w);
            let path = path_ref.borrow().clone();
            if text.trim().is_empty() {
                if path.exists() {
                    let _ = std::fs::remove_file(&path);
                }
                reserved.borrow_mut().remove(&path);
                // Defer page removal so we don't destroy the widget mid-signal;
                // keep the last tab so the panel never goes blank.
                let notebook = notebook.clone();
                let editor = editor.clone();
                glib::idle_add_local_once(move || {
                    let (Some(notebook), Some(editor)) = (notebook.upgrade(), editor.upgrade())
                    else {
                        return;
                    };
                    if notebook.n_pages() > 1 {
                        if let Some(n) = notebook.page_num(&editor) {
                            notebook.remove_page(Some(n));
                        }
                    }
                });
            } else {
                let _ = save_or_delete(&path, &text);
            }
        });
        text_view.add_controller(focus);
    }

    container.upcast()
}

/// Thin wrapper for `create_notes_widget`'s single-file path.
fn build_note_editor(
    notebook: &gtk4::Notebook,
    _scope: Scope,
    path: PathBuf,
    reserved: &Rc<RefCell<HashSet<PathBuf>>>,
) -> gtk4::Widget {
    let path_ref = Rc::new(RefCell::new(path));
    build_note_editor_inner(notebook, &path_ref, reserved)
}

/// Full buffer text.
fn buffer_text(buffer: &gtk4::TextBuffer) -> String {
    buffer
        .text(&buffer.start_iter(), &buffer.end_iter(), false)
        .to_string()
}

/// Persist `text` to `path`, deleting the file when empty. Returns `Ok(true)`
/// when content was written, `Ok(false)` when the note was empty.
fn save_or_delete(path: &Path, text: &str) -> std::io::Result<bool> {
    if text.trim().is_empty() {
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirror_rel_strips_leading_slash() {
        assert_eq!(mirror_rel("/home/u/src/dog"), PathBuf::from("home/u/src/dog"));
        assert_eq!(mirror_rel("rel/path"), PathBuf::from("rel/path"));
    }

    #[test]
    fn sanitize_dest_replaces_separators() {
        assert_eq!(sanitize_dest("user@host"), "user@host");
        assert_eq!(sanitize_dest("a/b\\c"), "a_b_c");
    }

    #[test]
    fn sanitize_filename_adds_md_and_rejects_blank() {
        assert_eq!(sanitize_filename("todo"), Some("todo.md".to_string()));
        assert_eq!(sanitize_filename("todo.md"), Some("todo.md".to_string()));
        assert_eq!(sanitize_filename("a/b"), Some("a_b.md".to_string()));
        assert_eq!(sanitize_filename("   "), None);
    }

    #[test]
    fn next_note_name_skips_used() {
        let dir = PathBuf::from("/x");
        let mut reserved = HashSet::new();
        assert_eq!(next_note_name(&dir, &reserved), dir.join("note.md"));
        reserved.insert(dir.join("note.md"));
        assert_eq!(next_note_name(&dir, &reserved), dir.join("note-2.md"));
    }

    #[test]
    fn git_root_walks_up_to_dot_git() {
        let tmp = std::env::temp_dir().join(format!("jmux-notes-test-{}", std::process::id()));
        let nested = tmp.join("repo/src/components");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(tmp.join("repo/.git")).unwrap();
        let found = git_root(&nested.to_string_lossy());
        assert_eq!(found, Some(tmp.join("repo")));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn remote_scopes_are_global_host_folder() {
        let ctx = NotesContext {
            cwd: Some("/srv/app".to_string()),
            remote: Some("user@bigboy".to_string()),
        };
        let s = scopes(&ctx);
        let kinds: Vec<Scope> = s.iter().map(|i| i.scope).collect();
        assert_eq!(kinds, vec![Scope::Global, Scope::Host, Scope::Folder]);
        let root = notes_tree_root();
        assert_eq!(s[1].dir, root.join("remote/user@bigboy"));
        assert_eq!(s[2].dir, root.join("remote/user@bigboy/srv/app"));
    }

    #[test]
    fn local_scopes_are_global_folder() {
        let ctx = NotesContext {
            cwd: Some("/tmp/not-a-repo-xyz".to_string()),
            remote: None,
        };
        let s = scopes(&ctx);
        let kinds: Vec<Scope> = s.iter().map(|i| i.scope).collect();
        assert_eq!(kinds, vec![Scope::Global, Scope::Folder]);
    }

    #[test]
    fn save_or_delete_writes_then_removes() {
        let tmp = std::env::temp_dir().join(format!("jmux-note-sd-{}.md", std::process::id()));
        assert_eq!(save_or_delete(&tmp, "hello").unwrap(), true);
        assert!(tmp.exists());
        assert_eq!(save_or_delete(&tmp, "   ").unwrap(), false);
        assert!(!tmp.exists());
    }
}
