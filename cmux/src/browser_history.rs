//! Browser history store — persistent browsing history with frecency ranking.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// A single browsing history entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub url: String,
    pub title: String,
    pub visit_count: u32,
    pub last_visited: u64,
    pub frecency_score: f64,
}

/// Persistent browser history with frecency-ranked search.
///
/// Data stored at `~/.local/share/cmux/browser-history.json`.
/// Thread-safe — all access goes through a global `Mutex`.
pub struct BrowserHistoryStore {
    entries: HashMap<String, HistoryEntry>,
    dirty: bool,
}

static STORE: Mutex<Option<BrowserHistoryStore>> = Mutex::new(None);

fn data_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("cmux/browser-history.json")
}

/// Canonical form: lowercase host, strip trailing slash, strip fragment.
fn canonical_url(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed.is_empty() || trimmed == "about:blank" {
        return trimmed.to_string();
    }

    // Parse into scheme + rest
    let (scheme, rest) = if let Some(idx) = trimmed.find("://") {
        (&trimmed[..idx], &trimmed[idx + 3..])
    } else {
        return trimmed.to_lowercase();
    };

    // Split host from path
    let (host_port, path) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, "/"),
    };

    // Strip fragment
    let path = match path.find('#') {
        Some(idx) => &path[..idx],
        None => path,
    };

    // Strip trailing slash (but keep bare "/")
    let path = if path.len() > 1 && path.ends_with('/') {
        &path[..path.len() - 1]
    } else {
        path
    };

    format!("{}://{}{}", scheme.to_lowercase(), host_port.to_lowercase(), path)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Compute frecency: visits weighted by recency (exponential decay, half-life ~7 days).
fn compute_frecency(visit_count: u32, last_visited: u64) -> f64 {
    let age_days = (now_secs().saturating_sub(last_visited)) as f64 / 86400.0;
    let recency_weight = (-age_days / 7.0).exp();
    visit_count as f64 * recency_weight
}

impl BrowserHistoryStore {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            dirty: false,
        }
    }

    fn load_from_disk() -> Self {
        let path = data_path();
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                let entries: Vec<HistoryEntry> =
                    serde_json::from_str(&content).unwrap_or_else(|err| {
                        tracing::warn!("Failed to parse browser history: {err}");
                        Vec::new()
                    });
                let map: HashMap<String, HistoryEntry> = entries
                    .into_iter()
                    .map(|e| (canonical_url(&e.url), e))
                    .collect();
                Self {
                    entries: map,
                    dirty: false,
                }
            }
            Err(_) => Self::new(),
        }
    }

    fn flush(&mut self) {
        if !self.dirty {
            return;
        }
        let path = data_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let entries: Vec<&HistoryEntry> = self.entries.values().collect();
        if let Ok(json) = serde_json::to_string_pretty(&entries) {
            let _ = std::fs::write(&path, json);
        }
        self.dirty = false;
    }
}

/// Initialize the global store (loads from disk). Call once at startup.
pub fn init() {
    let mut guard = STORE.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        *guard = Some(BrowserHistoryStore::load_from_disk());
    }
}

/// Record a page visit. Updates frecency and visit count.
pub fn record_visit(url: &str, title: &str) {
    let canonical = canonical_url(url);
    if canonical.is_empty() || canonical == "about:blank" {
        return;
    }

    let mut guard = STORE.lock().unwrap_or_else(|e| e.into_inner());
    let store = guard.get_or_insert_with(BrowserHistoryStore::load_from_disk);

    let now = now_secs();
    let entry = store.entries.entry(canonical.clone()).or_insert_with(|| HistoryEntry {
        url: url.to_string(),
        title: String::new(),
        visit_count: 0,
        last_visited: now,
        frecency_score: 0.0,
    });

    entry.visit_count += 1;
    entry.last_visited = now;
    if !title.is_empty() {
        entry.title = title.to_string();
    }
    // Keep the original URL if it looks better (has proper casing)
    if entry.url != url && canonical_url(url) == canonical {
        entry.url = url.to_string();
    }
    entry.frecency_score = compute_frecency(entry.visit_count, entry.last_visited);
    store.dirty = true;
}

/// Search history entries matching a query string. Returns results sorted by frecency (descending).
pub fn search(query: &str, limit: usize) -> Vec<HistoryEntry> {
    let guard = STORE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(store) = guard.as_ref() else {
        return Vec::new();
    };

    let query_lower = query.to_lowercase();
    let terms: Vec<&str> = query_lower.split_whitespace().collect();
    if terms.is_empty() {
        // Return top frecency entries
        let mut results: Vec<HistoryEntry> = store.entries.values().cloned().collect();
        results.sort_by(|a, b| b.frecency_score.partial_cmp(&a.frecency_score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit);
        return results;
    }

    let mut results: Vec<HistoryEntry> = store
        .entries
        .values()
        .filter(|entry| {
            let url_lower = entry.url.to_lowercase();
            let title_lower = entry.title.to_lowercase();
            terms.iter().all(|term| url_lower.contains(term) || title_lower.contains(term))
        })
        .cloned()
        .collect();

    // Boost exact URL prefix matches
    for entry in &mut results {
        let url_lower = entry.url.to_lowercase();
        if url_lower.contains(&query_lower) {
            entry.frecency_score *= 2.0;
        }
    }

    results.sort_by(|a, b| {
        b.frecency_score
            .partial_cmp(&a.frecency_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(limit);
    results
}

/// Flush dirty entries to disk. Call periodically or on shutdown.
pub fn flush() {
    let mut guard = STORE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(store) = guard.as_mut() {
        store.flush();
    }
}

/// Total number of history entries.
#[allow(dead_code)]
pub fn entry_count() -> usize {
    let guard = STORE.lock().unwrap_or_else(|e| e.into_inner());
    guard.as_ref().map(|s| s.entries.len()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canonical_url() {
        assert_eq!(
            canonical_url("https://Example.COM/path/"),
            "https://example.com/path"
        );
        assert_eq!(
            canonical_url("https://example.com/path#section"),
            "https://example.com/path"
        );
        assert_eq!(canonical_url("https://example.com/"), "https://example.com/");
        assert_eq!(canonical_url("about:blank"), "about:blank");
    }

    #[test]
    fn test_frecency_recent_is_higher() {
        let recent = compute_frecency(1, now_secs());
        let old = compute_frecency(1, now_secs() - 30 * 86400);
        assert!(recent > old);
    }
}
