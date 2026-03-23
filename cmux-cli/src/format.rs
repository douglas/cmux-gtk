use serde_json::Value;

/// Pretty-print a response for human consumption.
pub fn format_response(method: &str, response: &Value) {
    let ok = response
        .get("ok")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if !ok {
        if let Some(error) = response.get("error") {
            let code = error
                .get("code")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let msg = error.get("message").and_then(|v| v.as_str()).unwrap_or("");
            eprintln!("Error [{}]: {}", code, msg);
        }
        return;
    }

    let result = response.get("result");

    match method {
        "system.ping" => println!("pong"),

        "workspace.list" => {
            if let Some(workspaces) = result
                .and_then(|r| r.get("workspaces"))
                .and_then(|w| w.as_array())
            {
                for ws in workspaces {
                    let index = ws.get("index").and_then(|v| v.as_u64()).unwrap_or(0);
                    let title = ws.get("title").and_then(|v| v.as_str()).unwrap_or("?");
                    let selected = ws
                        .get("selected")
                        .or_else(|| ws.get("is_selected"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let panels = ws.get("panel_count").and_then(|v| v.as_u64()).unwrap_or(0);
                    let marker = if selected { "*" } else { " " };
                    println!("{}{} {} ({} panels)", marker, index, title, panels);
                }
            }
        }

        "system.identify" => {
            if let Some(r) = result {
                let app = r.get("app").and_then(|v| v.as_str()).unwrap_or("?");
                let platform = r.get("platform").and_then(|v| v.as_str()).unwrap_or("?");
                let version = r.get("version").and_then(|v| v.as_str()).unwrap_or("?");
                println!("{} {} v{}", app, platform, version);
            }
        }

        "system.capabilities" => {
            if let Some(methods) = result
                .and_then(|r| r.get("methods"))
                .and_then(|m| m.as_array())
            {
                for m in methods {
                    if let Some(s) = m.as_str() {
                        println!("  {}", s);
                    }
                }
            }
        }

        _ => {
            // Generic: print the result JSON
            if let Some(r) = result {
                println!("{}", serde_json::to_string_pretty(r).unwrap_or_default());
            } else {
                println!("OK");
            }
        }
    }
}

/// List available Ghostty themes from system and user directories.
pub fn run_themes(filter: Option<&str>) -> anyhow::Result<()> {
    let mut themes = Vec::new();

    // System themes: /usr/share/ghostty/themes/ or GHOSTTY_RESOURCES_DIR
    let system_dir = std::env::var("GHOSTTY_RESOURCES_DIR")
        .map(|d| std::path::PathBuf::from(d).join("themes"))
        .unwrap_or_else(|_| std::path::PathBuf::from("/usr/share/ghostty/themes"));
    collect_themes(&system_dir, &mut themes);

    // User themes: ~/.config/ghostty/themes/
    if let Some(home) = std::env::var_os("HOME") {
        let user_dir = std::path::PathBuf::from(home).join(".config/ghostty/themes");
        collect_themes(&user_dir, &mut themes);
    }

    themes.sort();
    themes.dedup();

    let filter_lower = filter.map(|f| f.to_lowercase());

    for theme in &themes {
        if let Some(ref f) = filter_lower {
            if !theme.to_lowercase().contains(f) {
                continue;
            }
        }
        println!("{theme}");
    }

    if themes.is_empty() {
        eprintln!("No Ghostty themes found.");
        eprintln!("Install ghostty or set GHOSTTY_RESOURCES_DIR.");
    }

    Ok(())
}

pub fn collect_themes(dir: &std::path::Path, themes: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if let Some(name) = entry.file_name().to_str() {
            if !name.starts_with('.') {
                themes.push(name.to_string());
            }
        }
    }
}
