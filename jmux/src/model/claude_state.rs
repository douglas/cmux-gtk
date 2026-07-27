//! Claude pane state detection — a Rust port of deck's classifier
//! (~/src/deck/deck/sessions.py + questions.py), so the sidebar shows the same
//! states the deck does.
//!
//! State, derived from an agent pane's title + visible terminal text:
//!
//!   NeedsInput  a selectable question/permission menu is on screen, or the
//!               last response asks the user something
//!   Working     the main agent's turn is running ("esc to interrupt"/spinner)
//!   Waiting     back at the prompt, but a backgrounded shell/agent is running
//!   None (idle) a prompt is shown, nothing running, no question

/// Most-urgent-last so `Ord`/`max()` picks the state that matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ClaudeState {
    Waiting,
    Working,
    NeedsInput,
}

/// Classify an agent pane's state from its visible text + raw (unstripped)
/// title. Precedence mirrors deck's `classify()`:
///
///   1. a real selection menu           → NeedsInput (a true block)
///   2. the main turn is running        → Working
///   3. a backgrounded task is running  → Waiting
///   4. the last response is a question → NeedsInput (free-text)
///   5. otherwise                       → None (idle)
///
/// Waiting sits *above* the soft "ends in ?" rule on purpose: if a background
/// shell is still running, any trailing "?" is stale (already answered).
pub fn classify(text: &str, raw_title: &str) -> Option<ClaudeState> {
    if has_pointer_menu(text) || has_menu_footer(text) {
        return Some(ClaudeState::NeedsInput);
    }
    let live = tail(text, 12);
    if is_active_turn(&live, raw_title) {
        return Some(ClaudeState::Working);
    }
    if is_background_running(&live) {
        return Some(ClaudeState::Waiting);
    }
    if last_response_is_question(text) {
        return Some(ClaudeState::NeedsInput);
    }
    None
}

/// A hard selection menu is on screen (pointer menu or "enter to select"
/// footer). This is the STRICT NeedsInput test used by the goal auto-driver:
/// the soft "last response ends in ?" heuristic in [`classify`] is fine for a
/// sidebar badge but far too twitchy to pause an autonomous run on.
pub fn has_selection_menu(text: &str) -> bool {
    has_pointer_menu(text) || has_menu_footer(text)
}

/// Whether a terminal pane's title looks like a plain shell rather than an
/// agent — "user@host:~/path" or a bare shell name. Such panes never carry a
/// Claude state.
pub fn is_shell_title(title: &str) -> bool {
    let t = title.trim();
    if matches!(
        t.to_ascii_lowercase().as_str(),
        "bash" | "zsh" | "fish" | "sh" | "-bash" | "-zsh"
    ) {
        return true;
    }
    // ^[\w.\-]+@[\w.\-]+:
    let mut seen_at = false;
    let mut before = 0usize;
    let mut after = 0usize;
    for c in t.chars() {
        match c {
            ':' => return seen_at && after > 0,
            '@' => {
                if seen_at || before == 0 {
                    return false;
                }
                seen_at = true;
            }
            c if c.is_alphanumeric() || c == '_' || c == '.' || c == '-' => {
                if seen_at {
                    after += 1;
                } else {
                    before += 1;
                }
            }
            _ => return false,
        }
    }
    false
}

/// The live status region — only the bottom of the pane. Scanning the whole
/// buffer matches stale status lines left in scrollback.
fn tail(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

/// Braille Patterns block — Claude animates its working spinner from these and
/// prepends a frame to the pane title while a turn runs.
fn is_braille(c: char) -> bool {
    ('\u{2800}'..='\u{28FF}').contains(&c)
}

/// Whether the *main* agent's turn is running (interruptible). The cheapest
/// tell is the animated braille spinner in the title; otherwise the footer's
/// "esc to interrupt" or a live parenthesised timer ("✢ Working… (1m 8s ·").
/// The paren anchor matters — past-tense summaries ("Crunched for 2m 15s")
/// must NOT count as active.
fn is_active_turn(live: &str, raw_title: &str) -> bool {
    if raw_title
        .trim_start()
        .chars()
        .next()
        .is_some_and(is_braille)
    {
        return true;
    }
    if live.to_ascii_lowercase().contains("esc to interrupt") {
        return true;
    }
    has_live_timer(live)
}

/// `\(\d+m \d+s` or `\(\d+s\b`
fn has_live_timer(s: &str) -> bool {
    let b: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < b.len() {
        if b[i] != '(' {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        let d0 = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > d0 {
            match b.get(j) {
                // (\d+m \d+s
                Some('m') if b.get(j + 1) == Some(&' ') => {
                    let mut k = j + 2;
                    let d1 = k;
                    while k < b.len() && b[k].is_ascii_digit() {
                        k += 1;
                    }
                    if k > d1 && b.get(k) == Some(&'s') {
                        return true;
                    }
                }
                // (\d+s\b
                Some('s') => {
                    let boundary = b
                        .get(j + 1)
                        .map_or(true, |c| !c.is_alphanumeric() && *c != '_');
                    if boundary {
                        return true;
                    }
                }
                _ => {}
            }
        }
        i = j.max(i + 1);
    }
    false
}

/// Whether a backgrounded shell/subagent is running while the main agent sits
/// back at the prompt — the "waiting on something to finish" state.
fn is_background_running(live: &str) -> bool {
    let lower = live.to_ascii_lowercase();
    if lower.contains("shell still running") || lower.contains("shells still running") {
        return true;
    }
    if live.contains("↓ to manage") {
        return true;
    }
    if has_shell_count(&lower) || waiting_for_background_agents(&lower) {
        return true;
    }
    has_subagent_row(live)
}

/// `\b\d+ shells?\b` — a backgrounded-command count ("· 1 shell ·").
fn has_shell_count(lower: &str) -> bool {
    for (pos, _) in lower.match_indices("shell") {
        // preceding: " " preceded by ≥1 digit with a word boundary before it
        let before = &lower[..pos];
        let Some(before) = before.strip_suffix(' ') else {
            continue;
        };
        let digits = before.chars().rev().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 {
            continue;
        }
        let pre = before.chars().rev().nth(digits);
        if pre.is_some_and(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        // following: optional 's', then a word boundary
        let rest = &lower[pos + "shell".len()..];
        let rest = rest.strip_prefix('s').unwrap_or(rest);
        if rest.chars().next().map_or(true, |c| !c.is_alphanumeric() && c != '_') {
            return true;
        }
    }
    false
}

/// `waiting for \d+ background agent`
fn waiting_for_background_agents(lower: &str) -> bool {
    for (pos, _) in lower.match_indices("waiting for ") {
        let rest = &lower[pos + "waiting for ".len()..];
        let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits > 0 && rest[digits..].starts_with(" background agent") {
            return true;
        }
    }
    false
}

/// `◯\s+\S` — a subagent monitor row ("◯ general-purpose … 5m 26s").
fn has_subagent_row(live: &str) -> bool {
    for (pos, m) in live.match_indices('◯') {
        let rest = &live[pos + m.len()..];
        let trimmed = rest.trim_start();
        if trimmed.len() < rest.len() && !trimmed.is_empty() && !trimmed.starts_with('\n') {
            return true;
        }
    }
    false
}

// ── Selection menus ──────────────────────────────────────────────────────────

/// A line that looks like a numbered menu option:
/// leading indent, optional ❯ pointer, "N." or "N)", then a non-empty label.
/// Returns (pointed, number).
fn parse_option(line: &str) -> Option<(bool, u32)> {
    let mut s = line.trim_start();
    let pointed = s.starts_with('❯');
    if pointed {
        s = s['❯'.len_utf8()..].trim_start();
    }
    let digits = s.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    let rest = &s[digits..];
    let mut chars = rest.chars();
    if !matches!(chars.next(), Some('.') | Some(')')) {
        return None;
    }
    let after = chars.as_str();
    if !after.starts_with(char::is_whitespace) || after.trim().is_empty() {
        return None;
    }
    s[..digits].parse().ok().map(|n| (pointed, n))
}

/// The classic ❯ menu: two or more contiguous lines numbered sequentially from
/// 1, with at least one line bearing the ❯ pointer. Strict, because scrollback
/// is full of things that loosely look like options.
fn has_pointer_menu(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let mut j = i;
        let mut expected = 1u32;
        let mut pointed = false;
        while j < lines.len() {
            match parse_option(lines[j]) {
                Some((p, n)) if n == expected => {
                    pointed |= p;
                    expected += 1;
                    j += 1;
                }
                _ => break,
            }
        }
        if j - i >= 2 && pointed {
            return true;
        }
        i = if j > i { j } else { i + 1 };
    }
    false
}

/// The newer boxed AskUserQuestion / plan picker has no ❯ pointer; its
/// unmistakable tell is the nav footer near the bottom.
fn has_menu_footer(text: &str) -> bool {
    let lower = tail(text, 8).to_ascii_lowercase();
    lower.contains("enter to select") || lower.contains("↑/↓ to navigate")
}

// ── "Ends in a question" ─────────────────────────────────────────────────────

/// Chrome lines that are not part of Claude's response (footers, separators,
/// status/recap/spinner lines, task-panel rows, the input box).
const CHROME_GLYPHS: &[char] = &[
    '✻', '✶', '✳', '✢', '✣', '※', '⏵', '◯', '●', '◻', '◼', '✔', '⎿', '│',
];
const CHROME_TEXT: &[&str] = &[
    "for shortcuts",
    "esc to interrupt",
    "auto mode on",
    "ctrl+t",
    "for agents",
    "tokens",
    "completed",
];

/// Whether Claude's most recent response asks the user something: the tail of
/// the actual response (chrome removed) has a sentence ending in "?" — not a
/// "?:" ternary or a "foo?bar" url.
fn last_response_is_question(text: &str) -> bool {
    let mut recent: [Option<String>; 4] = Default::default();
    let mut idx = 0;
    for raw in text.lines() {
        let owned = raw.replace('\u{a0}', " ");
        let s = owned.trim();
        if s.is_empty() || matches!(s, "❯" | "›" | "▶") {
            continue;
        }
        if s.chars().all(|c| "─—-=_ │".contains(c)) {
            continue;
        }
        let lower = s.to_ascii_lowercase();
        if s.starts_with(CHROME_GLYPHS) || CHROME_TEXT.iter().any(|t| lower.contains(t)) {
            continue;
        }
        recent[idx % 4] = Some(s.to_string());
        idx += 1;
    }
    recent.iter().flatten().any(|line| {
        let chars: Vec<char> = line.chars().collect();
        chars.iter().enumerate().any(|(i, c)| {
            *c == '?' && chars.get(i + 1).map_or(true, |n| n.is_whitespace())
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_titles() {
        assert!(is_shell_title("jbriggs@bigboy:~/src"));
        assert!(is_shell_title("bash"));
        assert!(is_shell_title("  zsh  "));
        assert!(!is_shell_title("Create local Whisper replacement"));
        assert!(!is_shell_title("claude"));
        assert!(!is_shell_title("fix the sidebar @ home: part 2"));
    }

    #[test]
    fn braille_title_is_working() {
        assert_eq!(classify("", "⠋ Fixing the bug"), Some(ClaudeState::Working));
        assert_eq!(classify("", "✳ Fixing the bug"), None);
    }

    #[test]
    fn esc_to_interrupt_is_working() {
        let text = "some output\n✢ Accomplishing… (esc to interrupt)\n";
        assert_eq!(classify(text, ""), Some(ClaudeState::Working));
    }

    #[test]
    fn live_timer_is_working() {
        assert_eq!(
            classify("✢ Accomplishing… (1m 8s · ↓ 2.4k tokens)", ""),
            Some(ClaudeState::Working)
        );
        assert_eq!(
            classify("✢ Pondering… (42s · esc)", ""),
            Some(ClaudeState::Working)
        );
        // Past-tense summary must NOT count as active.
        assert_eq!(classify("✳ Crunched for 2m 15s\n❯", ""), None);
    }

    #[test]
    fn background_shell_is_waiting() {
        assert_eq!(
            classify("· 1 shell · ↓ to manage\n❯", ""),
            Some(ClaudeState::Waiting)
        );
        assert_eq!(
            classify("2 shells still running\n❯", ""),
            Some(ClaudeState::Waiting)
        );
        assert_eq!(
            classify("◯ general-purpose … 5m 26s · ↓ 48.4k tokens\n❯", ""),
            Some(ClaudeState::Waiting)
        );
        assert_eq!(
            classify("waiting for 2 background agents to finish\n❯", ""),
            Some(ClaudeState::Waiting)
        );
    }

    #[test]
    fn pointer_menu_is_needs_input() {
        let text = "Do you want to proceed?\n❯ 1. Yes\n  2. Yes, and don't ask again\n  3. No\n";
        assert_eq!(classify(text, ""), Some(ClaudeState::NeedsInput));
    }

    #[test]
    fn menu_footer_is_needs_input() {
        let text = "Which approach?\n  1. Fast\n  2. Thorough\nenter to select · esc to cancel\n";
        assert_eq!(classify(text, ""), Some(ClaudeState::NeedsInput));
    }

    #[test]
    fn numbered_list_without_pointer_is_not_a_menu() {
        let text = "Here are the steps:\n  1. Build\n  2. Test\n  3. Ship\n❯";
        assert_eq!(classify(text, ""), None);
    }

    #[test]
    fn trailing_question_is_needs_input() {
        let text = "I fixed the bug in main.rs.\nWant me to also add a test?\n❯";
        assert_eq!(classify(text, ""), Some(ClaudeState::NeedsInput));
    }

    #[test]
    fn stale_question_with_background_shell_is_waiting() {
        // The user already answered; the shell is the live signal.
        let text = "Want me to also add a test?\nrunning…\n· 1 shell · ↓ to manage\n❯";
        assert_eq!(classify(text, ""), Some(ClaudeState::Waiting));
    }

    #[test]
    fn ternary_and_url_question_marks_ignored() {
        // "?" not followed by whitespace/EOL: ?: ternaries, ?. chaining, urls.
        assert_eq!(classify("let x = cond ?: b;\n❯", ""), None);
        assert_eq!(classify("let y = value?.method();\n❯", ""), None);
        assert_eq!(classify("see https://example.com?foo=bar\n❯", ""), None);
    }

    #[test]
    fn idle_prompt_is_none() {
        assert_eq!(classify("$ ls\nfile.txt\n❯", ""), None);
        assert_eq!(classify("", ""), None);
    }

    #[test]
    fn urgency_ordering() {
        assert!(ClaudeState::NeedsInput > ClaudeState::Working);
        assert!(ClaudeState::Working > ClaudeState::Waiting);
    }

    #[test]
    fn stale_scrollback_outside_tail_ignored() {
        // "esc to interrupt" 20 lines up must not count.
        let mut text = String::from("✢ old (esc to interrupt)\n");
        text.push_str(&"line\n".repeat(20));
        text.push('❯');
        assert_eq!(classify(&text, ""), None);
    }
}
