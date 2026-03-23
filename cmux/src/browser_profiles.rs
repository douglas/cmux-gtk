//! Browser profiles — isolated WebKit sessions per profile.
//!
//! Each profile gets its own `NetworkSession` with separate cookies, storage,
//! and cache directories under `~/.local/share/cmux/webkit-profiles/<name>/`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

/// A browser profile with isolated storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserProfile {
    pub name: String,
    pub is_default: bool,
}

/// Persistent profile list.
#[derive(Debug, Default, Serialize, Deserialize)]
struct ProfileStore {
    profiles: Vec<BrowserProfile>,
}

static STORE: Mutex<Option<ProfileStore>> = Mutex::new(None);

fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("cmux/browser-profiles.json")
}

fn profiles_data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("cmux/webkit-profiles")
}

/// Initialize the profile store. Creates a "Default" profile if none exist.
pub fn init() {
    let mut guard = crate::app::lock_or_recover(&STORE);
    if guard.is_some() {
        return;
    }

    let store = match std::fs::read_to_string(config_path()) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => ProfileStore::default(),
    };

    let mut store: ProfileStore = store;
    if store.profiles.is_empty() {
        store.profiles.push(BrowserProfile {
            name: "Default".to_string(),
            is_default: true,
        });
        save_store(&store);
    }

    *guard = Some(store);
}

fn save_store(store: &ProfileStore) {
    let path = config_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(store) {
        let _ = std::fs::write(path, json);
    }
}

/// List all profiles.
pub fn list() -> Vec<BrowserProfile> {
    let guard = crate::app::lock_or_recover(&STORE);
    guard
        .as_ref()
        .map(|s| s.profiles.clone())
        .unwrap_or_default()
}

/// Get the default profile name.
pub fn default_profile_name() -> String {
    let guard = crate::app::lock_or_recover(&STORE);
    guard
        .as_ref()
        .and_then(|s| s.profiles.iter().find(|p| p.is_default))
        .map(|p| p.name.clone())
        .unwrap_or_else(|| "Default".to_string())
}

/// Validate a profile name: must be non-empty, at most 64 chars, and free
/// of path traversal characters (`/`, `\`, `..`).
fn is_valid_profile_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains("..")
}

/// Create a new profile. Returns false if name already exists or is invalid.
#[allow(dead_code)]
pub fn create(name: &str) -> bool {
    if !is_valid_profile_name(name) {
        return false;
    }

    let mut guard = crate::app::lock_or_recover(&STORE);
    let store = guard.get_or_insert_with(ProfileStore::default);

    if store.profiles.iter().any(|p| p.name == name) {
        return false;
    }

    store.profiles.push(BrowserProfile {
        name: name.to_string(),
        is_default: false,
    });
    save_store(store);
    true
}

/// Rename a profile. Returns false if old name not found, new name already
/// exists, or new name is invalid.
#[allow(dead_code)]
pub fn rename(old_name: &str, new_name: &str) -> bool {
    if !is_valid_profile_name(new_name) {
        return false;
    }

    let mut guard = crate::app::lock_or_recover(&STORE);
    let store = guard.get_or_insert_with(ProfileStore::default);

    if store.profiles.iter().any(|p| p.name == new_name) {
        return false;
    }

    if let Some(profile) = store.profiles.iter_mut().find(|p| p.name == old_name) {
        // Rename data directories
        let old_dir = profiles_data_dir().join(old_name);
        let new_dir = profiles_data_dir().join(new_name);
        if old_dir.exists() {
            let _ = std::fs::rename(&old_dir, &new_dir);
        }

        profile.name = new_name.to_string();
        save_store(store);
        true
    } else {
        false
    }
}

/// Delete a profile. Cannot delete the default profile.
#[allow(dead_code)]
pub fn delete(name: &str) -> bool {
    let mut guard = crate::app::lock_or_recover(&STORE);
    let store = guard.get_or_insert_with(ProfileStore::default);

    let idx = store.profiles.iter().position(|p| p.name == name);
    if let Some(idx) = idx {
        if store.profiles[idx].is_default {
            return false; // Cannot delete default
        }
        store.profiles.remove(idx);

        // Remove data directory
        let dir = profiles_data_dir().join(name);
        if dir.exists() {
            let _ = std::fs::remove_dir_all(&dir);
        }

        save_store(store);
        true
    } else {
        false
    }
}

/// Set which profile is the default.
#[allow(dead_code)]
pub fn set_default(name: &str) -> bool {
    let mut guard = crate::app::lock_or_recover(&STORE);
    let store = guard.get_or_insert_with(ProfileStore::default);

    if !store.profiles.iter().any(|p| p.name == name) {
        return false;
    }

    for profile in &mut store.profiles {
        profile.is_default = profile.name == name;
    }
    save_store(store);
    true
}

/// Get the data and cache directory paths for a profile's NetworkSession.
pub fn profile_dirs(name: &str) -> (PathBuf, PathBuf) {
    let base_data = profiles_data_dir().join(name);
    let base_cache = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("~/.cache"))
        .join("cmux/webkit-profiles")
        .join(name);
    (base_data, base_cache)
}

// Per-profile NetworkSession cache (GTK main thread only).
// Maps profile name → NetworkSession.
thread_local! {
    static SESSION_CACHE: std::cell::RefCell<HashMap<String, webkit6::NetworkSession>> =
        std::cell::RefCell::new(HashMap::new());
}

/// Get or create a NetworkSession for a profile (GTK main thread only).
pub fn network_session_for(profile_name: &str) -> webkit6::NetworkSession {
    SESSION_CACHE.with(|cache| {
        let mut map = cache.borrow_mut();
        if let Some(session) = map.get(profile_name) {
            return session.clone();
        }

        let (data_dir, cache_dir) = profile_dirs(profile_name);
        let _ = std::fs::create_dir_all(&data_dir);
        let _ = std::fs::create_dir_all(&cache_dir);
        // Restrict WebKit data/cache directories to owner-only (0o700) to prevent
        // other local users from reading cookies, localStorage, or cached pages.
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&data_dir, std::fs::Permissions::from_mode(0o700));
        let _ = std::fs::set_permissions(&cache_dir, std::fs::Permissions::from_mode(0o700));

        let session = webkit6::NetworkSession::new(
            Some(data_dir.to_str().unwrap_or("")),
            Some(cache_dir.to_str().unwrap_or("")),
        );

        // Wire download handling for this session
        crate::ui::browser_panel::wire_download_handling_for_session(&session);

        map.insert(profile_name.to_string(), session.clone());
        session
    })
}
