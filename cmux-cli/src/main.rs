//! cmux CLI — command-line client for the cmux socket API.

use clap::{Parser, Subcommand};
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};

const IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const MAX_RESPONSE_LEN: usize = 1024 * 1024;

static REQUEST_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Parser)]
#[command(name = "cmux", about = "cmux terminal multiplexer CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Socket path override
    #[arg(long, default_value_t = default_socket_path(), global = true)]
    socket: String,

    /// Output raw JSON
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Ping the cmux server
    Ping,

    /// Workspace management
    #[command(subcommand)]
    Workspace(WorkspaceCommands),

    /// Surface (terminal) operations
    #[command(subcommand)]
    Surface(SurfaceCommands),

    /// Pane operations
    #[command(subcommand)]
    Pane(PaneCommands),

    /// Notification management
    #[command(subcommand)]
    Notification(NotificationCommands),

    /// Send a notification (shorthand for notification create)
    Notify {
        /// Notification title
        #[arg(long)]
        title: String,
        /// Notification body
        #[arg(long, default_value = "")]
        body: String,
        /// Target workspace UUID
        #[arg(long)]
        workspace: Option<String>,
        /// Target surface/panel UUID
        #[arg(long)]
        surface: Option<String>,
        /// Suppress desktop notification
        #[arg(long)]
        no_desktop: bool,
    },

    /// List available API methods
    Capabilities,

    /// Identify the cmux server (platform, version)
    Identify,

    /// Show the layout tree for all workspaces
    Tree,

    /// Open the settings window
    Settings,

    /// Show sidebar state (selected workspace)
    SidebarState,
}

#[derive(Subcommand)]
enum WorkspaceCommands {
    /// List all workspaces
    List,
    /// Show current (selected) workspace
    Current,
    /// Create a new workspace
    New {
        /// Working directory
        #[arg(long)]
        directory: Option<String>,
        /// Workspace title
        #[arg(long)]
        title: Option<String>,
    },
    /// Select a workspace by index (0-based)
    Select {
        /// Workspace index
        index: usize,
    },
    /// Select the next workspace
    Next {
        /// Wrap around when reaching the end (default: true)
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        wrap: bool,
    },
    /// Select the previous workspace
    Previous {
        /// Wrap around when reaching the start (default: true)
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        wrap: bool,
    },
    /// Select the last workspace
    Last,
    /// Jump to the newest unread workspace
    LatestUnread,
    /// Close a workspace
    Close {
        /// Workspace index (closes selected if not specified)
        index: Option<usize>,
    },
    /// Rename a workspace
    Rename {
        /// New title
        title: String,
        /// Target workspace UUID (defaults to selected)
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Reorder a workspace
    Reorder {
        /// Source index
        from: usize,
        /// Destination index
        to: usize,
    },
    /// Set status metadata
    SetStatus {
        /// Status key
        #[arg(long)]
        key: String,
        /// Status value
        #[arg(long)]
        value: String,
        /// Optional icon
        #[arg(long)]
        icon: Option<String>,
        /// Optional color
        #[arg(long)]
        color: Option<String>,
    },
    /// Clear all status entries
    ClearStatus {
        /// Target workspace UUID
        #[arg(long)]
        workspace: Option<String>,
    },
    /// List status entries
    ListStatus {
        /// Target workspace UUID
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Set progress bar
    SetProgress {
        /// Progress value (0.0 to 1.0, >1.0 for indeterminate)
        value: f64,
        /// Optional label
        #[arg(long)]
        label: Option<String>,
    },
    /// Clear progress bar
    ClearProgress {
        /// Target workspace UUID
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Append a log entry
    Log {
        /// Log message
        message: String,
        /// Log level (info, warning, error, success, progress)
        #[arg(long, default_value = "info")]
        level: String,
        /// Source name
        #[arg(long)]
        source: Option<String>,
    },
    /// Clear all log entries
    ClearLog {
        /// Target workspace UUID
        #[arg(long)]
        workspace: Option<String>,
    },
    /// List log entries
    ListLog {
        /// Target workspace UUID
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Report PR status for a workspace
    ReportPr {
        /// PR status: open, merged, closed, draft
        status: String,
        /// PR URL
        #[arg(long)]
        url: Option<String>,
        /// Target workspace UUID
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Perform an action on a workspace (pin, unpin, toggle_pin)
    Action {
        /// Action name: pin, unpin, toggle_pin
        action: String,
        /// Target workspace UUID
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Report git branch for workspace
    ReportGit {
        /// Branch name
        branch: String,
        /// Whether the working tree is dirty
        #[arg(long)]
        dirty: bool,
    },
}

#[derive(Subcommand)]
enum NotificationCommands {
    /// Create a notification
    Create {
        /// Notification title
        #[arg(long)]
        title: String,
        /// Notification body
        #[arg(long, default_value = "")]
        body: String,
        /// Target workspace UUID
        #[arg(long)]
        workspace: Option<String>,
        /// Target surface/panel UUID
        #[arg(long)]
        surface: Option<String>,
        /// Suppress desktop notification
        #[arg(long)]
        no_desktop: bool,
    },
    /// List all notifications
    List,
    /// Clear all notifications
    Clear,
}

#[derive(Subcommand)]
enum SurfaceCommands {
    /// Send text input to a terminal
    SendText {
        /// Text to send (supports \n for newline)
        text: String,
        /// Surface handle
        #[arg(long)]
        surface: Option<String>,
    },
    /// List surfaces (panels) in the current workspace
    List {
        /// Target workspace UUID
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Show the currently focused surface
    Current,
    /// Focus a surface by ID
    Focus {
        /// Surface/panel UUID
        id: String,
    },
    /// Flash a surface to attract attention
    Flash {
        /// Surface/panel UUID (flashes focused panel if not specified)
        #[arg(long)]
        surface: Option<String>,
    },
}

#[derive(Subcommand)]
enum PaneCommands {
    /// Create a new split pane
    New {
        /// Split orientation: horizontal or vertical
        #[arg(long, default_value = "horizontal")]
        orientation: String,
    },
    /// List panes in the current workspace
    List {
        /// Target workspace UUID
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Focus a pane by ID
    Focus {
        /// Panel UUID
        id: String,
    },
    /// Close a pane by ID (closes focused pane if not specified)
    Close {
        /// Panel UUID
        id: Option<String>,
    },
    /// Switch to the previously focused pane
    Last {
        /// Target workspace UUID
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Swap two panes in the layout
    Swap {
        /// First panel UUID
        a: String,
        /// Second panel UUID
        b: String,
    },
    /// Resize the split containing a pane
    Resize {
        /// Amount to adjust (-0.05 to shrink, 0.05 to grow)
        amount: f64,
        /// Panel UUID (defaults to focused)
        #[arg(long)]
        panel: Option<String>,
    },
    /// Focus the neighboring pane in a direction
    FocusDirection {
        /// Direction: left, right, up, down
        direction: String,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let (method, params) = match &cli.command {
        Commands::Ping => ("system.ping", serde_json::json!({})),
        Commands::Capabilities => ("system.capabilities", serde_json::json!({})),
        Commands::Identify => ("system.identify", serde_json::json!({})),
        Commands::Tree => ("system.tree", serde_json::json!({})),
        Commands::Settings => ("settings.open", serde_json::json!({})),
        Commands::SidebarState => ("workspace.current", serde_json::json!({})),

        Commands::Workspace(ws) => match ws {
            WorkspaceCommands::List => ("workspace.list", serde_json::json!({})),
            WorkspaceCommands::Current => ("workspace.current", serde_json::json!({})),
            WorkspaceCommands::New { directory, title } => (
                "workspace.new",
                serde_json::json!({
                    "directory": directory,
                    "title": title,
                }),
            ),
            WorkspaceCommands::Select { index } => {
                ("workspace.select", serde_json::json!({"index": index}))
            }
            WorkspaceCommands::Next { wrap } => {
                ("workspace.next", serde_json::json!({"wrap": wrap}))
            }
            WorkspaceCommands::Previous { wrap } => {
                ("workspace.previous", serde_json::json!({"wrap": wrap}))
            }
            WorkspaceCommands::Last => ("workspace.last", serde_json::json!({})),
            WorkspaceCommands::LatestUnread => ("workspace.latest_unread", serde_json::json!({})),
            WorkspaceCommands::Close { index } => {
                let mut params = serde_json::json!({});
                if let Some(idx) = index {
                    params["index"] = serde_json::json!(idx);
                }
                ("workspace.close", params)
            }
            WorkspaceCommands::Rename { title, workspace } => (
                "workspace.rename",
                serde_json::json!({
                    "title": title,
                    "workspace": workspace,
                }),
            ),
            WorkspaceCommands::Reorder { from, to } => (
                "workspace.reorder",
                serde_json::json!({"from": from, "to": to}),
            ),
            WorkspaceCommands::SetStatus {
                key,
                value,
                icon,
                color,
            } => (
                "workspace.set_status",
                serde_json::json!({
                    "key": key,
                    "value": value,
                    "icon": icon,
                    "color": color,
                }),
            ),
            WorkspaceCommands::ClearStatus { workspace } => (
                "workspace.clear_status",
                serde_json::json!({"workspace": workspace}),
            ),
            WorkspaceCommands::ListStatus { workspace } => (
                "workspace.list_status",
                serde_json::json!({"workspace": workspace}),
            ),
            WorkspaceCommands::SetProgress { value, label } => (
                "workspace.set_progress",
                serde_json::json!({"value": value, "label": label}),
            ),
            WorkspaceCommands::ClearProgress { workspace } => (
                "workspace.clear_progress",
                serde_json::json!({"workspace": workspace}),
            ),
            WorkspaceCommands::Log {
                message,
                level,
                source,
            } => (
                "workspace.append_log",
                serde_json::json!({
                    "message": message,
                    "level": level,
                    "source": source,
                }),
            ),
            WorkspaceCommands::ClearLog { workspace } => (
                "workspace.clear_log",
                serde_json::json!({"workspace": workspace}),
            ),
            WorkspaceCommands::ListLog { workspace } => (
                "workspace.list_log",
                serde_json::json!({"workspace": workspace}),
            ),
            WorkspaceCommands::ReportPr {
                status,
                url,
                workspace,
            } => (
                "workspace.report_pr",
                serde_json::json!({
                    "status": status,
                    "url": url,
                    "workspace": workspace,
                }),
            ),
            WorkspaceCommands::Action { action, workspace } => (
                "workspace.action",
                serde_json::json!({
                    "action": action,
                    "workspace": workspace,
                }),
            ),
            WorkspaceCommands::ReportGit { branch, dirty } => (
                "workspace.report_git_branch",
                serde_json::json!({"branch": branch, "is_dirty": dirty}),
            ),
        },

        Commands::Surface(surf) => match surf {
            SurfaceCommands::SendText { text, surface } => {
                let unescaped = text.replace("\\n", "\n");
                (
                    "surface.send_input",
                    serde_json::json!({
                        "input": unescaped,
                        "surface": surface,
                    }),
                )
            }
            SurfaceCommands::List { workspace } => (
                "surface.list",
                serde_json::json!({"workspace": workspace}),
            ),
            SurfaceCommands::Current => ("surface.current", serde_json::json!({})),
            SurfaceCommands::Focus { id } => (
                "surface.focus",
                serde_json::json!({"panel": id}),
            ),
            SurfaceCommands::Flash { surface } => (
                "surface.trigger_flash",
                serde_json::json!({"surface": surface}),
            ),
        },

        Commands::Pane(pane) => match pane {
            PaneCommands::New { orientation } => {
                ("pane.new", serde_json::json!({"orientation": orientation}))
            }
            PaneCommands::List { workspace } => (
                "pane.list",
                serde_json::json!({"workspace": workspace}),
            ),
            PaneCommands::Focus { id } => (
                "pane.focus",
                serde_json::json!({"panel": id}),
            ),
            PaneCommands::Close { id } => (
                "pane.close",
                serde_json::json!({"panel": id}),
            ),
            PaneCommands::Last { workspace } => (
                "pane.last",
                serde_json::json!({"workspace": workspace}),
            ),
            PaneCommands::Swap { a, b } => (
                "pane.swap",
                serde_json::json!({"a": a, "b": b}),
            ),
            PaneCommands::Resize { amount, panel } => (
                "pane.resize",
                serde_json::json!({"amount": amount, "panel": panel}),
            ),
            PaneCommands::FocusDirection { direction } => (
                "pane.focus_direction",
                serde_json::json!({"direction": direction}),
            ),
        },

        Commands::Notification(notif) => match notif {
            NotificationCommands::Create {
                title,
                body,
                workspace,
                surface,
                no_desktop,
            } => (
                "notification.create",
                serde_json::json!({
                    "title": title,
                    "body": body,
                    "workspace": workspace,
                    "surface": surface,
                    "send_desktop": !no_desktop,
                }),
            ),
            NotificationCommands::List => ("notification.list", serde_json::json!({})),
            NotificationCommands::Clear => ("notification.clear", serde_json::json!({})),
        },

        Commands::Notify {
            title,
            body,
            workspace,
            surface,
            no_desktop,
        } => (
            "notification.create",
            serde_json::json!({
                "title": title,
                "body": body,
                "workspace": workspace,
                "surface": surface,
                "send_desktop": !no_desktop,
            }),
        ),
    };

    let response = send_request(&cli.socket, method, params)?;

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&response)?);
    } else {
        format_response(method, &response);
    }

    // Exit with error code if the response indicates failure
    if response.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        std::process::exit(1);
    }

    Ok(())
}

/// Send a v2 request to the cmux socket and return the response.
fn send_request(socket_path: &str, method: &str, params: Value) -> anyhow::Result<Value> {
    let mut stream = UnixStream::connect(socket_path)
        .map_err(|e| anyhow::anyhow!("Cannot connect to cmux at {}: {}", socket_path, e))?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;

    let id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    let request = serde_json::json!({
        "id": id,
        "method": method,
        "params": params,
    });

    let request_json = serde_json::to_string(&request)?;
    stream.write_all(request_json.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;

    let limited = (&stream).take((MAX_RESPONSE_LEN + 1) as u64);
    let mut reader = BufReader::new(limited);
    let mut line = String::new();
    let bytes_read = reader.read_line(&mut line)?;
    if bytes_read == 0 {
        anyhow::bail!("cmux closed socket without a response");
    }
    if line.len() > MAX_RESPONSE_LEN {
        anyhow::bail!("cmux response exceeded {} bytes", MAX_RESPONSE_LEN);
    }

    let response: Value = serde_json::from_str(line.trim())?;
    Ok(response)
}

fn default_socket_path() -> String {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        let path = std::path::Path::new(&dir);
        if path.is_absolute() {
            if let Ok(meta) = std::fs::metadata(path) {
                let my_uid = unsafe { libc::getuid() };
                if meta.is_dir() && meta.uid() == my_uid && (meta.mode() & 0o777) == 0o700 {
                    return format!("{}/cmux.sock", dir);
                }
            }
        }
    }

    format!("/tmp/cmux-{}.sock", unsafe { libc::getuid() })
}

/// Pretty-print a response for human consumption.
fn format_response(method: &str, response: &Value) {
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
