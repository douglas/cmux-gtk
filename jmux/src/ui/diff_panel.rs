//! Diff panel — a git "CodeView" diff viewer.
//!
//! Renders `git diff` (optionally `--staged`) for a directory into a
//! monospace TextView with colored add/remove/hunk lines. Uses plain GTK
//! widgets (no WebKit), so it works in every build configuration.

use std::cell::Cell;
use std::process::Command;
use std::rc::Rc;

use gtk4::prelude::*;

/// Create a diff panel widget rendering `git diff` for `dir`.
///
/// Layout:
/// ```text
/// VBox:
///   ├─ toolbar (HBox): [icon] [label] [spacer] [Staged toggle] [reload]
///   └─ ScrolledWindow → TextView (monospace, colored diff)
/// ```
/// What the diff panel shows. Encoded in the panel's `command` field:
/// absent/empty = working tree, "staged" = index, "branch:<ref>" = vs <ref>.
enum DiffSpec {
    Working,
    Staged,
    Branch(String),
}

fn parse_spec(source: Option<&str>) -> DiffSpec {
    let s = source.unwrap_or("");
    if s == "staged" {
        DiffSpec::Staged
    } else if let Some(r) = s.strip_prefix("branch:") {
        DiffSpec::Branch(r.to_string())
    } else {
        DiffSpec::Working
    }
}

pub fn create_diff_widget(
    panel_id: uuid::Uuid,
    dir: Option<&str>,
    source: Option<&str>,
    is_attention_source: bool,
) -> gtk4::Widget {
    let initial_spec = parse_spec(source);
    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    container.set_hexpand(true);
    container.set_vexpand(true);
    container.add_css_class("panel-shell");
    if is_attention_source {
        container.add_css_class("attention-panel");
    }
    container.set_widget_name(&panel_id.to_string());

    let dir = dir
        .map(String::from)
        .unwrap_or_else(|| std::env::var("HOME").unwrap_or_else(|_| ".".to_string()));

    // ── Toolbar ──
    let toolbar = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    toolbar.add_css_class("browser-nav-bar");
    toolbar.set_margin_start(6);
    toolbar.set_margin_end(6);
    toolbar.set_margin_top(2);
    toolbar.set_margin_bottom(2);

    let icon = gtk4::Image::from_icon_name("media-flash-symbolic");
    icon.set_pixel_size(16);
    toolbar.append(&icon);

    let label = gtk4::Label::new(Some("git diff"));
    label.add_css_class("dim-label");
    label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    label.set_tooltip_text(Some(&dir));
    toolbar.append(&label);

    let spacer = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    toolbar.append(&spacer);

    let staged_toggle = gtk4::ToggleButton::with_label("Staged");
    staged_toggle.add_css_class("flat");
    staged_toggle.set_tooltip_text(Some("Show staged changes (git diff --staged)"));
    toolbar.append(&staged_toggle);

    let reload_btn = gtk4::Button::from_icon_name("view-refresh-symbolic");
    reload_btn.add_css_class("flat");
    reload_btn.set_tooltip_text(Some("Reload"));
    toolbar.append(&reload_btn);

    container.append(&toolbar);

    // ── Diff view ──
    let text_view = gtk4::TextView::new();
    text_view.set_editable(false);
    text_view.set_cursor_visible(false);
    text_view.set_monospace(true);
    text_view.set_wrap_mode(gtk4::WrapMode::None);
    text_view.add_css_class("diff-view");
    text_view.set_left_margin(8);
    text_view.set_right_margin(8);
    text_view.set_top_margin(4);

    let buffer = text_view.buffer();
    install_diff_tags(&buffer);

    let scrolled = gtk4::ScrolledWindow::new();
    scrolled.set_policy(gtk4::PolicyType::Automatic, gtk4::PolicyType::Automatic);
    scrolled.set_hexpand(true);
    scrolled.set_vexpand(true);
    scrolled.set_child(Some(&text_view));
    container.append(&scrolled);

    // Branch mode renders a fixed `git diff <ref>` and hides the staged toggle.
    if let DiffSpec::Branch(ref r) = initial_spec {
        let r = r.clone();
        staged_toggle.set_visible(false);
        label.set_text(&format!("git diff {r}"));
        render_diff(&buffer, &dir, &DiffSpec::Branch(r.clone()));
        {
            let buffer = buffer.clone();
            let dir = dir.clone();
            let r = r.clone();
            attach_comment_handler(&text_view, &dir.clone(), move || {
                render_diff(&buffer, &dir, &DiffSpec::Branch(r.clone()));
            });
        }
        let buffer2 = buffer.clone();
        let dir2 = dir.clone();
        reload_btn.connect_clicked(move |_| {
            render_diff(&buffer2, &dir2, &DiffSpec::Branch(r.clone()));
        });
        return container.upcast();
    }

    // Working-tree / staged mode with the toggle.
    let staged = Rc::new(Cell::new(matches!(initial_spec, DiffSpec::Staged)));
    staged_toggle.set_active(staged.get());

    let spec_of = |s: bool| if s { DiffSpec::Staged } else { DiffSpec::Working };
    render_diff(&buffer, &dir, &spec_of(staged.get()));

    {
        let buffer = buffer.clone();
        let dir = dir.clone();
        let staged = staged.clone();
        attach_comment_handler(&text_view, &dir.clone(), move || {
            let spec = if staged.get() {
                DiffSpec::Staged
            } else {
                DiffSpec::Working
            };
            render_diff(&buffer, &dir, &spec);
        });
    }

    {
        let buffer = buffer.clone();
        let dir = dir.clone();
        let staged = staged.clone();
        staged_toggle.connect_toggled(move |btn| {
            staged.set(btn.is_active());
            render_diff(&buffer, &dir, &spec_of(staged.get()));
        });
    }
    {
        let buffer = buffer.clone();
        let dir = dir.clone();
        let staged = staged.clone();
        reload_btn.connect_clicked(move |_| {
            render_diff(&buffer, &dir, &spec_of(staged.get()));
        });
    }

    container.upcast()
}

/// Lazily-loaded syntect syntax and theme sets (default assets, embedded).
fn syntax_assets() -> &'static (syntect::parsing::SyntaxSet, syntect::highlighting::ThemeSet) {
    use std::sync::OnceLock;
    static ASSETS: OnceLock<(syntect::parsing::SyntaxSet, syntect::highlighting::ThemeSet)> =
        OnceLock::new();
    ASSETS.get_or_init(|| {
        (
            syntect::parsing::SyntaxSet::load_defaults_newlines(),
            syntect::highlighting::ThemeSet::load_defaults(),
        )
    })
}

/// Whether the app is currently in dark mode (picks the diff theme + tints).
fn is_dark() -> bool {
    libadwaita::StyleManager::default().is_dark()
}

/// Install the text tags used to color diff lines.
fn install_diff_tags(buffer: &gtk4::TextBuffer) {
    let table = buffer.tag_table();
    let dark = is_dark();
    // Foreground tags for the +/- prefix and hunk/meta lines.
    let add = gtk4::TextTag::builder()
        .name("diff-add")
        .foreground("#2ea043")
        .build();
    let remove = gtk4::TextTag::builder()
        .name("diff-remove")
        .foreground("#f85149")
        .build();
    let hunk = gtk4::TextTag::builder()
        .name("diff-hunk")
        .foreground("#58a6ff")
        .build();
    let meta = gtk4::TextTag::builder()
        .name("diff-meta")
        .weight(700)
        .build();
    // Subtle line-background tints so highlighted code keeps its add/remove cue.
    let add_bg = gtk4::TextTag::builder()
        .name("diff-add-bg")
        .background(if dark { "#10261b" } else { "#e6ffec" })
        .build();
    let remove_bg = gtk4::TextTag::builder()
        .name("diff-remove-bg")
        .background(if dark { "#2a1518" } else { "#ffebe9" })
        .build();
    let comment = gtk4::TextTag::builder()
        .name("diff-comment")
        .foreground(if dark { "#e3b341" } else { "#9a6700" })
        .style(gtk4::pango::Style::Italic)
        .build();
    table.add(&add);
    table.add(&remove);
    table.add(&hunk);
    table.add(&meta);
    table.add(&add_bg);
    table.add(&remove_bg);
    table.add(&comment);
}

/// Path to the per-directory review-comments store.
fn comments_path(dir: &str) -> std::path::PathBuf {
    std::path::Path::new(dir).join(".jmux").join("diff-comments.json")
}

/// Load review comments (line-code → comment) for `dir`.
fn load_comments(dir: &str) -> std::collections::HashMap<String, String> {
    std::fs::read_to_string(comments_path(dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Persist review comments for `dir` (creates `.jmux/` as needed).
fn save_comments(dir: &str, comments: &std::collections::HashMap<String, String>) {
    let path = comments_path(dir);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(comments) {
        let _ = std::fs::write(path, json);
    }
}

/// If `code` (a diff line's content, sans prefix) has a saved comment, render it
/// inline below the line.
fn insert_inline_comment(
    buffer: &gtk4::TextBuffer,
    comments: &std::collections::HashMap<String, String>,
    code: &str,
) {
    let key = code.trim();
    if key.is_empty() {
        return;
    }
    if let Some(comment) = comments.get(key) {
        let mut end = buffer.end_iter();
        buffer.insert_with_tags_by_name(&mut end, &format!("    💬 {comment}\n"), &["diff-comment"]);
    }
}

/// Right-click a diff line to add/edit a review comment (keyed by line content,
/// persisted under `.jmux/diff-comments.json`). `rerender` redraws on save.
fn attach_comment_handler(text_view: &gtk4::TextView, dir: &str, rerender: impl Fn() + 'static) {
    let gesture = gtk4::GestureClick::new();
    gesture.set_button(3);
    let dir = dir.to_string();
    // WEAK: the gesture is attached to the text view — a strong capture is a
    // textview → gesture → closure → textview cycle that leaked the whole
    // rendered diff buffer per rebuild.
    let tv = text_view.downgrade();
    let rerender = Rc::new(rerender);
    gesture.connect_pressed(move |gesture, _n, x, y| {
        let Some(tv) = tv.upgrade() else {
            return;
        };
        let (bx, by) =
            tv.window_to_buffer_coords(gtk4::TextWindowType::Widget, x as i32, y as i32);
        let Some(iter) = tv.iter_at_location(bx, by) else {
            return;
        };
        let mut start = iter.clone();
        start.set_line_offset(0);
        let mut line_end = start.clone();
        if !line_end.ends_line() {
            line_end.forward_to_line_end();
        }
        let line_text = tv.buffer().text(&start, &line_end, false).to_string();
        let code_key = line_text
            .trim_start_matches([' ', '+', '-'])
            .trim()
            .to_string();
        if code_key.is_empty() {
            return;
        }
        gesture.set_state(gtk4::EventSequenceState::Claimed);

        let existing = load_comments(&dir).get(&code_key).cloned().unwrap_or_default();
        let popover = gtk4::Popover::new();
        popover.set_parent(&tv);
        popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
        vbox.set_margin_top(8);
        vbox.set_margin_bottom(8);
        vbox.set_margin_start(8);
        vbox.set_margin_end(8);
        let entry = gtk4::Entry::new();
        entry.set_placeholder_text(Some("Review comment (empty to remove)"));
        entry.set_text(&existing);
        entry.set_width_chars(40);
        let save = gtk4::Button::with_label("Save comment");
        save.add_css_class("suggested-action");
        vbox.append(&entry);
        vbox.append(&save);
        popover.set_child(Some(&vbox));

        let commit = {
            let dir = dir.clone();
            let code_key = code_key.clone();
            let entry = entry.clone();
            let popover = popover.clone();
            let rerender = rerender.clone();
            move || {
                let text = entry.text().to_string();
                let mut comments = load_comments(&dir);
                if text.trim().is_empty() {
                    comments.remove(&code_key);
                } else {
                    comments.insert(code_key.clone(), text.trim().to_string());
                }
                save_comments(&dir, &comments);
                popover.popdown();
                rerender();
            }
        };
        {
            let commit = commit.clone();
            save.connect_clicked(move |_| commit());
        }
        entry.connect_activate(move |_| commit());
        popover.popup();
    });
    text_view.add_controller(gesture);
}

/// Return the name of a foreground tag for `color`, creating it on first use.
fn fg_tag_name(buffer: &gtk4::TextBuffer, color: syntect::highlighting::Color) -> String {
    let name = format!("synfg-{:02x}{:02x}{:02x}", color.r, color.g, color.b);
    let table = buffer.tag_table();
    if table.lookup(&name).is_none() {
        let hex = format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b);
        let tag = gtk4::TextTag::builder()
            .name(&name)
            .foreground(&hex)
            .build();
        table.add(&tag);
    }
    name
}

/// Insert a code line's `content` (no prefix) syntax-highlighted, applying
/// `bg_tag` (if any) to the whole inserted range. Falls back to plain text.
fn insert_highlighted(
    buffer: &gtk4::TextBuffer,
    hl: &mut syntect::easy::HighlightLines,
    ss: &syntect::parsing::SyntaxSet,
    content: &str,
    bg_tag: Option<&str>,
) {
    match hl.highlight_line(content, ss) {
        Ok(ranges) => {
            for (style, text) in ranges {
                if text.is_empty() {
                    continue;
                }
                let fg = fg_tag_name(buffer, style.foreground);
                let mut names: Vec<&str> = Vec::with_capacity(2);
                if let Some(bg) = bg_tag {
                    names.push(bg);
                }
                names.push(&fg);
                let mut end = buffer.end_iter();
                buffer.insert_with_tags_by_name(&mut end, text, &names);
            }
        }
        Err(_) => {
            let mut end = buffer.end_iter();
            match bg_tag {
                Some(bg) => buffer.insert_with_tags_by_name(&mut end, content, &[bg]),
                None => buffer.insert(&mut end, content),
            }
        }
    }
}

/// Run `git diff` (optionally `--staged`) in `dir` and render it into `buffer`
/// with per-line coloring.
fn render_diff(buffer: &gtk4::TextBuffer, dir: &str, spec: &DiffSpec) {
    buffer.set_text("");

    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(dir).arg("--no-pager").arg("diff");
    match spec {
        DiffSpec::Working => {}
        DiffSpec::Staged => {
            cmd.arg("--staged");
        }
        DiffSpec::Branch(r) => {
            cmd.arg(r);
        }
    }
    let output = cmd.output();

    let empty_msg = match spec {
        DiffSpec::Staged => "No staged changes.",
        DiffSpec::Branch(_) => "No differences from that ref.",
        DiffSpec::Working => "No changes in the working tree.",
    };

    let text = match output {
        Ok(out) if out.status.success() => {
            let s = String::from_utf8_lossy(&out.stdout).to_string();
            if s.trim().is_empty() {
                buffer.set_text(empty_msg);
                return;
            }
            s
        }
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr);
            buffer.set_text(&format!("git diff failed:\n{err}"));
            return;
        }
        Err(e) => {
            buffer.set_text(&format!("Failed to run git: {e}"));
            return;
        }
    };

    // Syntax-highlight code lines while keeping the add/remove cue. Two
    // highlighters reconstruct the new (context+add) and old (context+remove)
    // file states so each side colors correctly.
    use syntect::easy::HighlightLines;
    let (ss, ts) = syntax_assets();
    let theme = &ts.themes[if is_dark() {
        "base16-ocean.dark"
    } else {
        "InspiredGitHub"
    }];
    let plain = ss.find_syntax_plain_text();
    let mut hl_new = HighlightLines::new(plain, theme);
    let mut hl_old = HighlightLines::new(plain, theme);
    let comments = load_comments(dir);

    let meta = |buffer: &gtk4::TextBuffer, line: &str| {
        let mut end = buffer.end_iter();
        buffer.insert_with_tags_by_name(&mut end, line, &["diff-meta"]);
    };

    for line in text.split_inclusive('\n') {
        // File header: switch syntax + reset both highlighters.
        if line.starts_with("+++") {
            if let Some(path) = line.strip_prefix("+++ b/") {
                let syntax = ss
                    .find_syntax_for_file(path.trim_end())
                    .ok()
                    .flatten()
                    .unwrap_or(plain);
                hl_new = HighlightLines::new(syntax, theme);
                hl_old = HighlightLines::new(syntax, theme);
            }
            meta(buffer, line);
        } else if line.starts_with("---")
            || line.starts_with("@@")
            || line.starts_with("diff ")
            || line.starts_with("index ")
            || line.starts_with("new file")
            || line.starts_with("deleted file")
            || line.starts_with("rename ")
            || line.starts_with("similarity ")
            || line.starts_with("old mode")
            || line.starts_with("new mode")
            || line.starts_with('\\')
        {
            let tag = if line.starts_with("@@") {
                "diff-hunk"
            } else {
                "diff-meta"
            };
            let mut end = buffer.end_iter();
            buffer.insert_with_tags_by_name(&mut end, line, &[tag]);
        } else if let Some(rest) = line.strip_prefix('+') {
            let mut end = buffer.end_iter();
            buffer.insert_with_tags_by_name(&mut end, "+", &["diff-add", "diff-add-bg"]);
            insert_highlighted(buffer, &mut hl_new, ss, rest, Some("diff-add-bg"));
            insert_inline_comment(buffer, &comments, rest);
        } else if let Some(rest) = line.strip_prefix('-') {
            let mut end = buffer.end_iter();
            buffer.insert_with_tags_by_name(&mut end, "-", &["diff-remove", "diff-remove-bg"]);
            insert_highlighted(buffer, &mut hl_old, ss, rest, Some("diff-remove-bg"));
            insert_inline_comment(buffer, &comments, rest);
        } else {
            // Context line: feed both sides (advance old state), display new.
            let rest = line.strip_prefix(' ').unwrap_or(line);
            if line.starts_with(' ') {
                let mut end = buffer.end_iter();
                buffer.insert(&mut end, " ");
            }
            let _ = hl_old.highlight_line(rest, ss);
            insert_highlighted(buffer, &mut hl_new, ss, rest, None);
            insert_inline_comment(buffer, &comments, rest);
        }
    }
}
