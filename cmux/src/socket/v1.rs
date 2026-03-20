//! V1 text protocol parser.
//!
//! Shell integration scripts send simple text lines like:
//!   `report_pwd /home/user --tab=abc123 --panel=def456`
//!
//! This module parses them and translates to V2 JSON dispatch.

use std::sync::Arc;

use serde_json::{json, Value};

use crate::app::SharedState;
use crate::socket::v2;

/// Check if a line looks like a V1 text command (not JSON).
pub fn is_v1(line: &str) -> bool {
    let trimmed = line.trim();
    !trimmed.is_empty() && !trimmed.starts_with('{')
}

/// Parse and dispatch a V1 text line. Returns the JSON response string.
pub fn dispatch(line: &str, state: &Arc<SharedState>) -> String {
    let trimmed = line.trim();
    let (command, rest) = match trimmed.split_once(' ') {
        Some((cmd, rest)) => (cmd, rest.trim()),
        None => (trimmed, ""),
    };

    // Extract --key=value flags and positional args
    let (args, flags) = parse_args(rest);

    let workspace_id = flags.get("tab").or_else(|| flags.get("workspace"));
    let panel_id = flags.get("panel").or_else(|| flags.get("surface"));

    let (method, params) = match command {
        "report_pwd" => {
            let dir = args.first().map(|s| s.as_str()).unwrap_or("");
            let mut p = json!({"directory": dir});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            if let Some(panel) = panel_id {
                p["surface"] = json!(panel);
            }
            ("workspace.report_pwd", p)
        }
        "report_git_branch" => {
            let branch = args.first().map(|s| s.as_str()).unwrap_or("");
            let mut p = json!({"branch": branch});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("workspace.report_git_branch", p)
        }
        "clear_git_branch" => {
            let mut p = json!({});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("workspace.report_git_branch", json!({"branch": "", "workspace": ws_or_null(workspace_id)}))
        }
        "report_pr" => {
            let status = args.first().map(|s| s.as_str()).unwrap_or("");
            let mut p = json!({"status": status});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("workspace.report_pr", p)
        }
        "clear_pr" => {
            let mut p = json!({"status": ""});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("workspace.report_pr", p)
        }
        "report_tty" => {
            let tty = args.first().map(|s| s.as_str()).unwrap_or("");
            let mut p = json!({"tty": tty});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            if let Some(panel) = panel_id {
                p["surface"] = json!(panel);
            }
            ("workspace.report_tty", p)
        }
        "ports_kick" => ("workspace.ports_kick", json!({})),
        "report_shell_state" => {
            let state_val = args.first().map(|s| s.as_str()).unwrap_or("prompt");
            let mut p = json!({"state": state_val});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("workspace.set_status", p)
        }
        "ping" => ("system.ping", json!({})),
        "report_ports" => {
            let ports: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let mut p = json!({"ports": ports});
            if let Some(ws) = workspace_id {
                p["workspace"] = json!(ws);
            }
            ("workspace.report_ports", p)
        }
        _ => {
            return format!(
                "{{\"ok\":false,\"error\":{{\"code\":\"unknown_v1_command\",\"message\":\"Unknown command: {}\"}}}}\n",
                command
            );
        }
    };

    // Build a V2 JSON request and dispatch it
    let v2_json = json!({
        "id": "v1",
        "method": method,
        "params": params,
    });

    let response = v2::dispatch(&v2_json.to_string(), state);
    match serde_json::to_string(&response) {
        Ok(mut s) => {
            s.push('\n');
            s
        }
        Err(_) => "{\"ok\":false,\"error\":{\"code\":\"internal\",\"message\":\"serialization failed\"}}\n".to_string(),
    }
}

fn ws_or_null(ws: Option<&String>) -> Value {
    ws.map(|s| json!(s)).unwrap_or(Value::Null)
}

/// Parse "arg1 arg2 --flag=value --other=val" into (positional_args, flags).
/// Supports quoted arguments: `"path with spaces"`.
fn parse_args(input: &str) -> (Vec<String>, std::collections::HashMap<String, String>) {
    let mut args = Vec::new();
    let mut flags = std::collections::HashMap::new();

    let mut chars = input.chars().peekable();
    while chars.peek().is_some() {
        // Skip whitespace
        while chars.peek() == Some(&' ') {
            chars.next();
        }
        if chars.peek().is_none() {
            break;
        }

        let mut token = String::new();
        if chars.peek() == Some(&'"') {
            // Quoted string
            chars.next(); // consume opening quote
            while let Some(&ch) = chars.peek() {
                if ch == '"' {
                    chars.next();
                    break;
                }
                token.push(ch);
                chars.next();
            }
        } else {
            // Unquoted token
            while let Some(&ch) = chars.peek() {
                if ch == ' ' {
                    break;
                }
                token.push(ch);
                chars.next();
            }
        }

        if let Some(flag) = token.strip_prefix("--") {
            if let Some((key, value)) = flag.split_once('=') {
                flags.insert(key.to_string(), value.to_string());
            } else {
                flags.insert(flag.to_string(), String::new());
            }
        } else {
            args.push(token);
        }
    }

    (args, flags)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_v1() {
        assert!(is_v1("report_pwd /home/user"));
        assert!(is_v1("ping"));
        assert!(!is_v1("{\"id\": 1}"));
        assert!(!is_v1("  {\"method\": \"test\"}  "));
    }

    #[test]
    fn test_parse_args() {
        let (args, flags) = parse_args(r#"/home/user --tab=abc --panel=def"#);
        assert_eq!(args, vec!["/home/user"]);
        assert_eq!(flags.get("tab").unwrap(), "abc");
        assert_eq!(flags.get("panel").unwrap(), "def");
    }

    #[test]
    fn test_parse_quoted_args() {
        let (args, flags) = parse_args(r#""/path with spaces" --tab=abc"#);
        assert_eq!(args, vec!["/path with spaces"]);
        assert_eq!(flags.get("tab").unwrap(), "abc");
    }
}
