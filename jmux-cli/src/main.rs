//! jmux CLI — command-line client for the jmux socket API.

mod agent_view;
mod commands;
mod config;
mod format;
mod rpc;

use clap::Parser;
use commands::*;

#[derive(Parser)]
#[command(name = "jmux", about = "jmux terminal multiplexer CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Socket path override
    #[arg(long, default_value_t = rpc::default_socket_path(), global = true)]
    socket: String,

    /// Output raw JSON
    #[arg(long, global = true)]
    json: bool,

    /// Route command to a specific window by ID (UUID)
    #[arg(long, global = true)]
    window: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Local commands that don't need the socket
    if let Commands::Themes { filter } = &cli.command {
        return format::run_themes(filter.as_deref());
    }

    // Dry-run for reorder-workspaces: fetch current order, print diff, exit.
    if let Commands::Workspace(WorkspaceCommands::ReorderWorkspaces { workspaces, dry_run: true }) =
        &cli.command
    {
        let current_resp = rpc::send_request(&cli.socket, "workspace.list", serde_json::json!({}), None)?;
        if current_resp.get("ok").and_then(|v| v.as_bool()) != Some(true) {
            eprintln!("Failed to fetch workspace list for dry-run.");
            std::process::exit(1);
        }
        let empty = vec![];
        let current: Vec<&str> = current_resp["result"]["workspaces"]
            .as_array()
            .unwrap_or(&empty)
            .iter()
            .filter_map(|ws| ws["title"].as_str())
            .collect();
        println!("Current order:");
        for (i, name) in current.iter().enumerate() {
            println!("  [{i}] {name}");
        }
        println!("Proposed order:");
        for (i, name) in workspaces.iter().enumerate() {
            println!("  [{i}] {name}");
        }
        return Ok(());
    }

    if let Commands::Config(cmd) = &cli.command {
        match cmd {
            ConfigCommands::Path => return config::run_path(),
            ConfigCommands::Doctor => return config::run_doctor(),
            ConfigCommands::Docs => return config::run_docs(),
            ConfigCommands::Reload => {
                let response = rpc::send_request(&cli.socket, "settings.open", serde_json::json!({}), None)?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&response)?);
                } else if response.get("ok").and_then(|v| v.as_bool()) == Some(true) {
                    println!("Config reloaded.");
                } else {
                    eprintln!("Reload failed.");
                    std::process::exit(1);
                }
                return Ok(());
            }
        }
    }

    // `jmux top` — live refreshing process table.
    if let Commands::Top { interval } = &cli.command {
        return run_top(&cli.socket, *interval);
    }

    // Local transcript viewer — no socket needed.
    if let Commands::Agent(AgentCommands::View { transcript }) = &cli.command {
        return agent_view::run(transcript);
    }

    // Agent hook events may involve multiple socket calls; handle them before
    // the single-dispatch main match below.
    if let Commands::Agent(AgentCommands::Hook { event, cli: agent_cli, message, parent, panel }) = &cli.command {
        match event.as_str() {
            "session-start" => {
                rpc::send_request(
                    &cli.socket,
                    "workspace.set_status",
                    serde_json::json!({"key": agent_cli, "value": "running", "icon": null, "color": null}),
                    cli.window.as_deref(),
                )?;
            }
            "session-stop" | "session-end" => {
                rpc::send_request(
                    &cli.socket,
                    "workspace.clear_status",
                    serde_json::json!({"key": agent_cli}),
                    cli.window.as_deref(),
                )?;
            }
            "notification" => {
                let (title, body) = if let Some(msg) = message {
                    (agent_cli.clone(), msg.clone())
                } else {
                    // Read JSON from stdin
                    let mut input = String::new();
                    use std::io::Read;
                    let _ = std::io::stdin().read_to_string(&mut input);
                    let v: serde_json::Value = serde_json::from_str(&input).unwrap_or(serde_json::json!({}));
                    let t = v.get("title").and_then(|x| x.as_str()).unwrap_or(agent_cli).to_string();
                    let b = v.get("body").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    (t, b)
                };
                rpc::send_request(
                    &cli.socket,
                    "notification.create",
                    serde_json::json!({"title": title, "body": body, "send_desktop": true}),
                    cli.window.as_deref(),
                )?;
            }
            "subagent-start" => {
                let parent_panel_id = match parent {
                    Some(p) => p.clone(),
                    None => {
                        eprintln!("subagent-start requires --parent <panel_id>");
                        std::process::exit(1);
                    }
                };
                let resp = rpc::send_request(
                    &cli.socket,
                    "agent.spawn_subagent",
                    serde_json::json!({
                        "parent_panel_id": parent_panel_id,
                        "cli_name": agent_cli,
                    }),
                    cli.window.as_deref(),
                )?;
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&resp)?);
                } else if let Some(pid) = resp.get("result").and_then(|r| r.get("panel_id")).and_then(|v| v.as_str()) {
                    println!("{pid}");
                } else {
                    eprintln!("subagent-start failed: {:?}", resp.get("error"));
                    std::process::exit(1);
                }
            }
            "subagent-stop" => {
                let panel_id = match panel {
                    Some(p) => p.clone(),
                    None => {
                        eprintln!("subagent-stop requires --panel <panel_id>");
                        std::process::exit(1);
                    }
                };
                // Mark the panel as finished by closing it
                rpc::send_request(
                    &cli.socket,
                    "pane.close",
                    serde_json::json!({"panel": panel_id}),
                    cli.window.as_deref(),
                )?;
            }
            other => {
                eprintln!("Unknown agent hook event: {other}");
                eprintln!("Valid events: session-start, session-stop, session-end, notification, subagent-start, subagent-stop");
                std::process::exit(1);
            }
        }
        return Ok(());
    }

    // Goal commands: launch involves optional completion polling (--wait),
    // so handle the whole family before the single-dispatch match below.
    if let Commands::Goal { .. } = &cli.command {
        return run_goal(&cli);
    }
    if let Commands::Graph { .. } = &cli.command {
        return run_graph(&cli);
    }

    let (method, params) = match &cli.command {
        Commands::Themes { .. } => unreachable!(),
        Commands::Goal { .. } => unreachable!(), // handled above
        Commands::Graph { .. } => unreachable!(), // handled above
        Commands::Config(_) => unreachable!(),
        Commands::Top { .. } => unreachable!(), // handled above
        Commands::Agent(AgentCommands::Hook { .. }) => unreachable!(), // handled above
        Commands::Agent(AgentCommands::View { .. }) => unreachable!(), // handled above
        Commands::Agent(AgentCommands::Fork { message, name }) => (
            "agent.fork_conversation",
            serde_json::json!({"message": message, "workspace_name": name}),
        ),
        Commands::Agent(AgentCommands::SpawnSubagent { parent_panel_id, cli_name, working_directory }) => (
            "agent.spawn_subagent",
            serde_json::json!({
                "parent_panel_id": parent_panel_id,
                "cli_name": cli_name,
                "working_directory": working_directory,
            }),
        ),
        Commands::Ping => ("system.ping", serde_json::json!({})),
        Commands::Capabilities => ("system.capabilities", serde_json::json!({})),
        Commands::Identify => ("system.identify", serde_json::json!({})),
        Commands::Tree => ("system.tree", serde_json::json!({})),
        Commands::Settings => ("settings.open", serde_json::json!({})),
        Commands::SidebarState => ("workspace.current", serde_json::json!({})),

        Commands::Browser(cmd) => match cmd {
            BrowserCommands::Navigate { panel, url } => (
                "browser.navigate",
                serde_json::json!({"panel": panel, "url": url}),
            ),
            BrowserCommands::ExecuteJs { panel, script } => (
                "browser.execute_js",
                serde_json::json!({"panel": panel, "script": script}),
            ),
            BrowserCommands::GetUrl { panel } => {
                ("browser.get_url", serde_json::json!({"panel": panel}))
            }
            BrowserCommands::GetText { panel } => {
                ("browser.get_text", serde_json::json!({"panel": panel}))
            }
            BrowserCommands::Back { panel } => {
                ("browser.back", serde_json::json!({"panel": panel}))
            }
            BrowserCommands::Forward { panel } => {
                ("browser.forward", serde_json::json!({"panel": panel}))
            }
            BrowserCommands::Reload { panel } => {
                ("browser.reload", serde_json::json!({"panel": panel}))
            }
            BrowserCommands::SetZoom { panel, zoom } => (
                "browser.set_zoom",
                serde_json::json!({"panel": panel, "zoom": zoom}),
            ),
            BrowserCommands::Screenshot { panel } => {
                ("browser.screenshot", serde_json::json!({"panel": panel}))
            }
            BrowserCommands::Mute { panel, muted } => {
                let mut p = serde_json::json!({"panel": panel});
                if let Some(m) = muted {
                    p["muted"] = serde_json::json!(m);
                }
                ("browser.mute", p)
            }
            BrowserCommands::FocusMode { panel, enabled } => {
                let mut p = serde_json::json!({"panel": panel});
                if let Some(e) = enabled {
                    p["enabled"] = serde_json::json!(e);
                }
                ("browser.focus_mode", p)
            }
            BrowserCommands::ReactGrab { panel } => {
                ("browser.react_grab", serde_json::json!({"panel": panel}))
            }
            BrowserCommands::Click {
                panel,
                selector,
                button,
            } => (
                "browser.click",
                serde_json::json!({"panel": panel, "selector": selector, "button": button}),
            ),
            BrowserCommands::Dblclick { panel, selector } => (
                "browser.dblclick",
                serde_json::json!({"panel": panel, "selector": selector}),
            ),
            BrowserCommands::Hover { panel, selector } => (
                "browser.hover",
                serde_json::json!({"panel": panel, "selector": selector}),
            ),
            BrowserCommands::Type {
                panel,
                selector,
                text,
            } => (
                "browser.type",
                serde_json::json!({"panel": panel, "selector": selector, "text": text}),
            ),
            BrowserCommands::Fill {
                panel,
                selector,
                value,
            } => (
                "browser.fill",
                serde_json::json!({"panel": panel, "selector": selector, "value": value}),
            ),
            BrowserCommands::Clear { panel, selector } => (
                "browser.clear",
                serde_json::json!({"panel": panel, "selector": selector}),
            ),
            BrowserCommands::Press {
                panel,
                selector,
                key,
            } => (
                "browser.press",
                serde_json::json!({"panel": panel, "selector": selector, "key": key}),
            ),
            BrowserCommands::SelectOption {
                panel,
                selector,
                value,
                label,
                index,
            } => (
                "browser.select_option",
                serde_json::json!({"panel": panel, "selector": selector, "value": value, "label": label, "index": index}),
            ),
            BrowserCommands::Check {
                panel,
                selector,
                checked,
            } => (
                "browser.check",
                serde_json::json!({"panel": panel, "selector": selector, "checked": checked}),
            ),
            BrowserCommands::Focus { panel, selector } => (
                "browser.focus",
                serde_json::json!({"panel": panel, "selector": selector}),
            ),
            BrowserCommands::Blur { panel, selector } => (
                "browser.blur",
                serde_json::json!({"panel": panel, "selector": selector}),
            ),
            BrowserCommands::ScrollTo {
                panel,
                selector,
                x,
                y,
            } => (
                "browser.scroll_to",
                serde_json::json!({"panel": panel, "selector": selector, "x": x, "y": y}),
            ),
            BrowserCommands::GetHtml {
                panel,
                selector,
                outer,
            } => (
                "browser.get_html",
                serde_json::json!({"panel": panel, "selector": selector, "outer": outer}),
            ),
            BrowserCommands::GetValue { panel, selector } => (
                "browser.get_value",
                serde_json::json!({"panel": panel, "selector": selector}),
            ),
            BrowserCommands::GetAttribute {
                panel,
                selector,
                name,
            } => (
                "browser.get_attribute",
                serde_json::json!({"panel": panel, "selector": selector, "name": name}),
            ),
            BrowserCommands::GetProperty {
                panel,
                selector,
                name,
            } => (
                "browser.get_property",
                serde_json::json!({"panel": panel, "selector": selector, "name": name}),
            ),
            BrowserCommands::GetBoundingBox { panel, selector } => (
                "browser.get_bounding_box",
                serde_json::json!({"panel": panel, "selector": selector}),
            ),
            BrowserCommands::GetComputedStyle {
                panel,
                selector,
                property,
            } => (
                "browser.get_computed_style",
                serde_json::json!({"panel": panel, "selector": selector, "property": property}),
            ),
            BrowserCommands::IsVisible { panel, selector } => (
                "browser.is_visible",
                serde_json::json!({"panel": panel, "selector": selector}),
            ),
            BrowserCommands::IsEnabled { panel, selector } => (
                "browser.is_enabled",
                serde_json::json!({"panel": panel, "selector": selector}),
            ),
            BrowserCommands::IsChecked { panel, selector } => (
                "browser.is_checked",
                serde_json::json!({"panel": panel, "selector": selector}),
            ),
            BrowserCommands::IsEditable { panel, selector } => (
                "browser.is_editable",
                serde_json::json!({"panel": panel, "selector": selector}),
            ),
            BrowserCommands::Count { panel, selector } => (
                "browser.count",
                serde_json::json!({"panel": panel, "selector": selector}),
            ),
            BrowserCommands::Find { panel, selector } => (
                "browser.find",
                serde_json::json!({"panel": panel, "selector": selector}),
            ),
            BrowserCommands::FindAll { panel, selector } => (
                "browser.find_all",
                serde_json::json!({"panel": panel, "selector": selector}),
            ),
            BrowserCommands::FindByText { panel, text } => (
                "browser.find_by_text",
                serde_json::json!({"panel": panel, "text": text}),
            ),
            BrowserCommands::FindByRole { panel, role } => (
                "browser.find_by_role",
                serde_json::json!({"panel": panel, "role": role}),
            ),
            BrowserCommands::FindByLabel { panel, label } => (
                "browser.find_by_label",
                serde_json::json!({"panel": panel, "label": label}),
            ),
            BrowserCommands::FindByPlaceholder { panel, placeholder } => (
                "browser.find_by_placeholder",
                serde_json::json!({"panel": panel, "placeholder": placeholder}),
            ),
            BrowserCommands::FindByTestId { panel, test_id } => (
                "browser.find_by_test_id",
                serde_json::json!({"panel": panel, "test_id": test_id}),
            ),
            BrowserCommands::ReleaseRef { panel, ref_id } => (
                "browser.release_ref",
                serde_json::json!({"panel": panel, "ref": ref_id}),
            ),
            BrowserCommands::WaitForSelector {
                panel,
                selector,
                timeout,
            } => (
                "browser.wait_for_selector",
                serde_json::json!({"panel": panel, "selector": selector, "timeout": timeout}),
            ),
            BrowserCommands::WaitForNavigation { panel, timeout } => (
                "browser.wait_for_navigation",
                serde_json::json!({"panel": panel, "timeout": timeout}),
            ),
            BrowserCommands::WaitForLoadState { panel, timeout } => (
                "browser.wait_for_load_state",
                serde_json::json!({"panel": panel, "timeout": timeout}),
            ),
            BrowserCommands::WaitForFunction {
                panel,
                expression,
                timeout,
            } => (
                "browser.wait_for_function",
                serde_json::json!({"panel": panel, "expression": expression, "timeout": timeout}),
            ),
            BrowserCommands::Snapshot { panel } => {
                ("browser.snapshot", serde_json::json!({"panel": panel}))
            }
            BrowserCommands::Title { panel } => {
                ("browser.title", serde_json::json!({"panel": panel}))
            }
            BrowserCommands::GetCookies { panel } => {
                ("browser.get_cookies", serde_json::json!({"panel": panel}))
            }
            BrowserCommands::SetCookie { panel, cookie } => (
                "browser.set_cookie",
                serde_json::json!({"panel": panel, "cookie": cookie}),
            ),
            BrowserCommands::ClearCookies { panel } => {
                ("browser.clear_cookies", serde_json::json!({"panel": panel}))
            }
            BrowserCommands::LocalStorageGet { panel, key } => (
                "browser.local_storage_get",
                serde_json::json!({"panel": panel, "key": key}),
            ),
            BrowserCommands::LocalStorageSet { panel, key, value } => (
                "browser.local_storage_set",
                serde_json::json!({"panel": panel, "key": key, "value": value}),
            ),
            BrowserCommands::SessionStorageGet { panel, key } => (
                "browser.session_storage_get",
                serde_json::json!({"panel": panel, "key": key}),
            ),
            BrowserCommands::SessionStorageSet { panel, key, value } => (
                "browser.session_storage_set",
                serde_json::json!({"panel": panel, "key": key, "value": value}),
            ),
            BrowserCommands::GetConsoleMessages { panel } => (
                "browser.get_console_messages",
                serde_json::json!({"panel": panel}),
            ),
            BrowserCommands::SetDialogHandler {
                panel,
                action,
                text,
            } => (
                "browser.set_dialog_handler",
                serde_json::json!({"panel": panel, "action": action, "text": text}),
            ),
            BrowserCommands::InjectScript { panel, script } => (
                "browser.inject_script",
                serde_json::json!({"panel": panel, "script": script}),
            ),
            BrowserCommands::InjectStyle { panel, css } => (
                "browser.inject_style",
                serde_json::json!({"panel": panel, "css": css}),
            ),
            BrowserCommands::RemoveInjected { panel } => (
                "browser.remove_injected",
                serde_json::json!({"panel": panel}),
            ),
            BrowserCommands::ImportCookies { source, .. } => (
                "browser.import_cookies",
                serde_json::json!({"source": source}),
            ),
        },

        Commands::Markdown(cmd) => match cmd {
            MarkdownCommands::Open { file, workspace } => (
                "markdown.open",
                serde_json::json!({"file": file, "workspace_id": workspace}),
            ),
        },

        Commands::Workspace(ws) => match ws {
            WorkspaceCommands::List => ("workspace.list", serde_json::json!({})),
            WorkspaceCommands::Current => ("workspace.current", serde_json::json!({})),
            WorkspaceCommands::New { directory, title, layout } => {
                // Use workspace.create when a non-single layout is requested so
                // the server can apply the layout after the workspace is created.
                let method = if layout == "single" { "workspace.new" } else { "workspace.create" };
                let params = serde_json::json!({
                    "directory": directory,
                    "title": title,
                    "layout": layout,
                });
                (method, params)
            }
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
                serde_json::json!({"title": title, "workspace": workspace}),
            ),
            WorkspaceCommands::Reorder { from, to } => (
                "workspace.reorder",
                serde_json::json!({"from": from, "to": to}),
            ),
            WorkspaceCommands::ReorderWorkspaces { workspaces, .. } => (
                "workspace.reorder_workspaces",
                serde_json::json!({"workspaces": workspaces}),
            ),
            WorkspaceCommands::SetStatus {
                key,
                value,
                icon,
                color,
            } => (
                "workspace.set_status",
                serde_json::json!({"key": key, "value": value, "icon": icon, "color": color}),
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
                serde_json::json!({"message": message, "level": level, "source": source}),
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
                serde_json::json!({"status": status, "url": url, "workspace": workspace}),
            ),
            WorkspaceCommands::Action {
                action,
                workspace,
                color,
                title,
            } => (
                "workspace.action",
                serde_json::json!({"action": action, "workspace": workspace, "color": color, "title": title}),
            ),
            WorkspaceCommands::ReportPwd {
                directory,
                panel,
                workspace,
            } => (
                "workspace.report_pwd",
                serde_json::json!({"directory": directory, "panel": panel, "workspace": workspace}),
            ),
            WorkspaceCommands::ReportPorts { ports, panel } => (
                "workspace.report_ports",
                serde_json::json!({"ports": ports, "panel": panel}),
            ),
            WorkspaceCommands::ClearPorts { panel } => {
                ("workspace.clear_ports", serde_json::json!({"panel": panel}))
            }
            WorkspaceCommands::ReportTty { tty, panel } => (
                "workspace.report_tty",
                serde_json::json!({"tty": tty, "panel": panel}),
            ),
            WorkspaceCommands::PortsKick => ("workspace.ports_kick", serde_json::json!({})),
            WorkspaceCommands::ReportGit { branch, dirty } => (
                "workspace.report_git_branch",
                serde_json::json!({"branch": branch, "is_dirty": dirty}),
            ),
            WorkspaceCommands::ImessageMode { enable, disable, workspace } => {
                let enabled = if *enable {
                    true
                } else if *disable {
                    false
                } else {
                    eprintln!("Provide --enable or --disable");
                    std::process::exit(1);
                };
                (
                    "workspace.set_imessage_mode",
                    serde_json::json!({"enabled": enabled, "workspace": workspace}),
                )
            }
        },

        Commands::Surface(surf) => match surf {
            SurfaceCommands::SendText { text, surface } => {
                let unescaped = text.replace("\\n", "\n");
                (
                    "surface.send_input",
                    serde_json::json!({"input": unescaped, "surface": surface}),
                )
            }
            SurfaceCommands::List { workspace } => {
                ("surface.list", serde_json::json!({"workspace": workspace}))
            }
            SurfaceCommands::Current => ("surface.current", serde_json::json!({})),
            SurfaceCommands::Focus { id } => ("surface.focus", serde_json::json!({"panel": id})),
            SurfaceCommands::SendKey { key, mods, surface } => (
                "surface.send_key",
                serde_json::json!({"key": key, "mods": mods, "surface": surface}),
            ),
            SurfaceCommands::ReadScreen { surface } => {
                ("surface.read_text", serde_json::json!({"surface": surface}))
            }
            SurfaceCommands::Flash { surface } => (
                "surface.trigger_flash",
                serde_json::json!({"surface": surface}),
            ),
            SurfaceCommands::Split { orientation } => (
                "surface.split",
                serde_json::json!({"orientation": orientation}),
            ),
            SurfaceCommands::Close { id } => ("surface.close", serde_json::json!({"panel": id})),
            SurfaceCommands::Refresh { surface } => {
                ("surface.refresh", serde_json::json!({"surface": surface}))
            }
            SurfaceCommands::ClearHistory { surface } => (
                "surface.clear_history",
                serde_json::json!({"surface": surface}),
            ),
            SurfaceCommands::Action { action, surface } => (
                "surface.action",
                serde_json::json!({"action": action, "surface": surface}),
            ),
            SurfaceCommands::Health { surface } => {
                ("surface.health", serde_json::json!({"surface": surface}))
            }
            SurfaceCommands::Move {
                panel,
                workspace,
                orientation,
            } => (
                "surface.move",
                serde_json::json!({"panel": panel, "workspace": workspace, "orientation": orientation}),
            ),
            SurfaceCommands::Reorder { panel, index } => (
                "surface.reorder",
                serde_json::json!({"panel": panel, "index": index}),
            ),
            SurfaceCommands::Create { r#type } => {
                ("surface.create", serde_json::json!({"type": r#type}))
            }
            SurfaceCommands::DragToSplit { direction, surface } => (
                "surface.drag_to_split",
                serde_json::json!({"direction": direction, "surface": surface}),
            ),
        },

        Commands::Tab(tab) => match tab {
            TabCommands::Action {
                action,
                surface,
                title,
            } => (
                "tab.action",
                serde_json::json!({"action": action, "surface": surface, "title": title}),
            ),
        },

        Commands::Pane(pane) => match pane {
            PaneCommands::New { orientation } => {
                ("pane.new", serde_json::json!({"orientation": orientation}))
            }
            PaneCommands::Create { orientation } => (
                "pane.create",
                serde_json::json!({"orientation": orientation}),
            ),
            PaneCommands::List { workspace } => {
                ("pane.list", serde_json::json!({"workspace": workspace}))
            }
            PaneCommands::Focus { id } => ("pane.focus", serde_json::json!({"panel": id})),
            PaneCommands::Close { id } => ("pane.close", serde_json::json!({"panel": id})),
            PaneCommands::Last { workspace } => {
                ("pane.last", serde_json::json!({"workspace": workspace}))
            }
            PaneCommands::Swap { a, b } => ("pane.swap", serde_json::json!({"a": a, "b": b})),
            PaneCommands::Resize { amount, panel } => (
                "pane.resize",
                serde_json::json!({"amount": amount, "panel": panel}),
            ),
            PaneCommands::FocusDirection { direction } => (
                "pane.focus_direction",
                serde_json::json!({"direction": direction}),
            ),
            PaneCommands::Break { panel } => ("pane.break", serde_json::json!({"panel": panel})),
            PaneCommands::Join { id, orientation } => (
                "pane.join",
                serde_json::json!({"panel": id, "orientation": orientation}),
            ),
            PaneCommands::Equalize { workspace } => {
                ("pane.equalize", serde_json::json!({"workspace": workspace}))
            }
            PaneCommands::Surfaces { panel } => {
                ("pane.surfaces", serde_json::json!({"panel": panel}))
            }
            PaneCommands::SplitOff { direction } => (
                "pane.split_off",
                serde_json::json!({"orientation": direction}),
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
                    "title": title, "body": body, "workspace": workspace,
                    "surface": surface, "send_desktop": !no_desktop,
                }),
            ),
            NotificationCommands::List { unread } => (
                "notification.list",
                serde_json::json!({"unread": unread}),
            ),
            NotificationCommands::Clear => ("notification.clear", serde_json::json!({})),
            NotificationCommands::MarkRead { id } => (
                "notification.mark_read",
                serde_json::json!({"id": id}),
            ),
            NotificationCommands::Dismiss { id } => (
                "notification.dismiss",
                serde_json::json!({"id": id}),
            ),
            NotificationCommands::Open { id } => (
                "notification.open",
                serde_json::json!({"id": id}),
            ),
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
                "title": title, "body": body, "workspace": workspace,
                "surface": surface, "send_desktop": !no_desktop,
            }),
        ),

        Commands::Sidebar(cmd) => match cmd {
            SidebarCommands::Show => ("sidebar.show", serde_json::json!({})),
            SidebarCommands::Hide => ("sidebar.hide", serde_json::json!({})),
            SidebarCommands::Toggle => ("sidebar.toggle", serde_json::json!({})),
            SidebarCommands::Status => ("sidebar.status", serde_json::json!({})),
        },
    };

    let response = rpc::send_request(&cli.socket, method, params, cli.window.as_deref())?;

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&response)?);
    } else {
        format::format_response(method, &response);
    }

    // Exit with error code if the response indicates failure
    if response.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        std::process::exit(1);
    }

    Ok(())
}

/// `jmux graph …` — DAG orchestration of goal workspaces.
fn run_graph(cli: &Cli) -> anyhow::Result<()> {
    let Commands::Graph {
        command,
        name,
        goal,
        max_concurrency,
        max_iterations,
        no_review,
        review_iterations,
        runner,
        full_auto,
        supervised,
    } = &cli.command
    else {
        unreachable!()
    };

    let (method, params) = if let Some(cmd) = command {
        match cmd {
            GraphCommands::Approve { name } => ("graph.approve", serde_json::json!({"name": name})),
            GraphCommands::Revise { name, note } => {
                ("graph.revise", serde_json::json!({"name": name, "note": note}))
            }
            GraphCommands::Status { name } => ("graph.status", serde_json::json!({"name": name})),
            GraphCommands::Pause { name } => ("graph.pause", serde_json::json!({"name": name})),
            GraphCommands::Resume { name } => ("graph.resume", serde_json::json!({"name": name})),
            GraphCommands::Stop { name } => ("graph.stop", serde_json::json!({"name": name})),
        }
    } else {
        let (Some(name), Some(goal)) = (name, goal) else {
            eprintln!("usage: jmux graph <name> --goal <top.md> [--max-concurrency K] …");
            eprintln!("       jmux graph approve|revise|status|pause|resume|stop <name>");
            std::process::exit(2);
        };
        let abs = std::fs::canonicalize(goal)
            .map_err(|e| anyhow::anyhow!("cannot resolve goal file '{goal}': {e}"))?;
        let permission_mode = if *full_auto {
            "bypassPermissions"
        } else if *supervised {
            "supervised"
        } else {
            "acceptEdits"
        };
        let mut params = serde_json::json!({
            "name": name,
            "goal": abs.to_string_lossy(),
            "review": !*no_review,
            "review_iterations": *review_iterations,
            "permission_mode": permission_mode,
        });
        let obj = params.as_object_mut().expect("params is an object");
        if let Some(v) = max_concurrency {
            obj.insert("max_concurrency".into(), serde_json::json!(v));
        }
        if let Some(v) = max_iterations {
            obj.insert("max_iterations".into(), serde_json::json!(v));
        }
        if let Some(v) = runner {
            obj.insert("runner".into(), serde_json::json!(v));
        }
        ("graph.create", params)
    };

    let response = rpc::send_request(&cli.socket, method, params, cli.window.as_deref())?;
    if cli.json || method != "graph.status" {
        println!("{}", serde_json::to_string_pretty(&response)?);
    } else {
        print_graph_status(&response);
    }
    if response.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        std::process::exit(1);
    }
    Ok(())
}

/// Human-friendly `graph status` rendering.
fn print_graph_status(response: &serde_json::Value) {
    let result = &response["result"];
    let empty = vec![];
    let graphs: Vec<&serde_json::Value> = if let Some(gs) = result["graphs"].as_array() {
        gs.iter().collect()
    } else if result.is_object() && result.get("name").is_some() {
        vec![result]
    } else {
        empty.iter().collect()
    };
    if graphs.is_empty() {
        println!("no graphs");
        return;
    }
    for g in graphs {
        println!(
            "graph {} — {} (concurrency {}, iterations/node {})",
            g["name"].as_str().unwrap_or("?"),
            g["status"].as_str().unwrap_or("?"),
            g["max_concurrency"].as_u64().unwrap_or(1),
            g["max_iterations"].as_u64().unwrap_or(1),
        );
        for n in g["nodes"].as_array().unwrap_or(&empty) {
            let deps = n["deps"]
                .as_array()
                .map(|d| {
                    d.iter()
                        .filter_map(|x| x.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            println!(
                "  [{}] {}{}{}",
                n["status"].as_str().unwrap_or("?"),
                n["id"].as_str().unwrap_or("?"),
                if deps.is_empty() {
                    String::new()
                } else {
                    format!("  <- {deps}")
                },
                n["detail"]
                    .as_str()
                    .map(|d| format!("  ({d})"))
                    .unwrap_or_default(),
            );
        }
    }
}

/// `jmux goal …` — launch / track goal-driven agent workspaces
/// (docs/roadmap/DESIGN-goal-graph.md).
fn run_goal(cli: &Cli) -> anyhow::Result<()> {
    let Commands::Goal {
        command,
        path,
        wait,
        cwd,
        runner,
        agent,
        model,
        effort,
        max_iterations,
        full_auto,
        supervised,
        title,
    } = &cli.command
    else {
        unreachable!()
    };

    if let Some(cmd) = command {
        let (method, params) = match cmd {
            GoalCommands::Complete { status, workspace } => {
                let ws = workspace
                    .clone()
                    .or_else(|| std::env::var("JMUX_WORKSPACE_ID").ok());
                let Some(ws) = ws else {
                    eprintln!(
                        "goal complete: no workspace — pass --workspace or run inside a jmux pane"
                    );
                    std::process::exit(1);
                };
                (
                    "goal.complete",
                    serde_json::json!({"workspace": ws, "status": status}),
                )
            }
            GoalCommands::Status { workspace } => {
                ("goal.status", serde_json::json!({"workspace": workspace}))
            }
            GoalCommands::Continue { workspace } => {
                ("goal.continue", serde_json::json!({"workspace": workspace}))
            }
            GoalCommands::Accept { workspace } => {
                ("goal.accept", serde_json::json!({"workspace": workspace}))
            }
        };
        let response = rpc::send_request(&cli.socket, method, params, cli.window.as_deref())?;
        println!("{}", serde_json::to_string_pretty(&response)?);
        if response.get("ok").and_then(|v| v.as_bool()) != Some(true) {
            std::process::exit(1);
        }
        return Ok(());
    }

    let Some(path) = path else {
        eprintln!("usage: jmux goal <goal.md> [--wait] [--runner NAME | --agent/--model/--effort]");
        std::process::exit(2);
    };
    let abs = std::fs::canonicalize(path)
        .map_err(|e| anyhow::anyhow!("cannot resolve goal file '{path}': {e}"))?;

    let permission_mode = if *full_auto {
        "bypassPermissions"
    } else if *supervised {
        "supervised"
    } else {
        "acceptEdits"
    };
    let mut params = serde_json::json!({
        "goal": abs.to_string_lossy(),
        "permission_mode": permission_mode,
    });
    let obj = params.as_object_mut().expect("params is an object");
    if let Some(v) = cwd {
        obj.insert("cwd".into(), serde_json::json!(v));
    }
    if let Some(v) = runner {
        obj.insert("runner".into(), serde_json::json!(v));
    }
    if let Some(v) = agent {
        obj.insert("agent".into(), serde_json::json!(v));
    }
    if let Some(v) = model {
        obj.insert("model".into(), serde_json::json!(v));
    }
    if let Some(v) = effort {
        obj.insert("effort".into(), serde_json::json!(v));
    }
    if let Some(v) = max_iterations {
        obj.insert("max_iterations".into(), serde_json::json!(v));
    }
    if let Some(v) = title {
        obj.insert("title".into(), serde_json::json!(v));
    }

    let response = rpc::send_request(&cli.socket, "goal.create", params, cli.window.as_deref())?;
    if response.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        eprintln!(
            "goal launch failed: {}",
            response
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("unknown error")
        );
        std::process::exit(1);
    }
    let result = &response["result"];
    let workspace_id = result["workspace_id"].as_str().unwrap_or("").to_string();
    if cli.json && !*wait {
        println!("{}", serde_json::to_string_pretty(&response)?);
        return Ok(());
    }
    println!(
        "goal '{}' launched (runner {}, iteration {})",
        result["goal"].as_str().unwrap_or("?"),
        result["runner"].as_str().unwrap_or("?"),
        result["iteration"].as_u64().unwrap_or(1),
    );
    println!("  workspace: {workspace_id}");
    println!("  output:    {}", result["output"].as_str().unwrap_or("?"));
    if !*wait {
        return Ok(());
    }

    // Poll goal.status until the run reaches a terminal state. Polling (not a
    // blocking RPC) survives app restarts and socket drops.
    let mut last_printed = String::new();
    let mut consecutive_failures = 0u32;
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        let resp = rpc::send_request(
            &cli.socket,
            "goal.status",
            serde_json::json!({"workspace": workspace_id}),
            cli.window.as_deref(),
        );
        let resp = match resp {
            Ok(r) => {
                consecutive_failures = 0;
                r
            }
            Err(e) => {
                consecutive_failures += 1;
                if consecutive_failures == 1 {
                    eprintln!("(connection lost, retrying: {e})");
                }
                if consecutive_failures > 120 {
                    eprintln!("goal --wait: gave up after {consecutive_failures} failed polls");
                    std::process::exit(1);
                }
                std::thread::sleep(std::time::Duration::from_secs(2));
                continue;
            }
        };
        if resp.get("ok").and_then(|v| v.as_bool()) != Some(true) {
            // The app restarted and forgot the run, or the workspace closed.
            eprintln!(
                "goal --wait: status unavailable ({}); check the iteration file",
                resp.get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown")
            );
            std::process::exit(1);
        }
        let r = &resp["result"];
        let status = r["status"].as_str().unwrap_or("");
        let detail = r["detail"].as_str().unwrap_or("");
        let line = format!("{status} {detail}");
        match status {
            "done" => {
                println!("goal done: {}", r["output"].as_str().unwrap_or("?"));
                return Ok(());
            }
            "blocked" => {
                eprintln!(
                    "goal blocked ({detail}): see {}",
                    r["output"].as_str().unwrap_or("?")
                );
                std::process::exit(2);
            }
            _ => {
                if line != last_printed {
                    if status == "needs-attention" {
                        eprintln!("goal needs attention: {detail}");
                    }
                    last_printed = line;
                }
            }
        }
    }
}

/// Run the live `jmux top` process viewer, refreshing at `interval` seconds.
/// Exits cleanly on Ctrl+C (SIGINT).
fn run_top(socket: &str, interval: u64) -> anyhow::Result<()> {
    use std::sync::atomic::Ordering;

    static INTERRUPTED: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);

    extern "C" fn handle_sigint(_: libc::c_int) {
        INTERRUPTED.store(true, Ordering::Relaxed);
    }

    // Install SIGINT handler so Ctrl+C exits cleanly.
    // SAFETY: signal handler only writes an AtomicBool — async-signal-safe.
    #[allow(clippy::fn_to_numeric_cast)]
    unsafe {
        libc::signal(libc::SIGINT, handle_sigint as *const () as libc::sighandler_t);
    }

    loop {
        if INTERRUPTED.load(Ordering::Relaxed) {
            // Restore cursor (in case we hid it) and exit.
            print!("\x1b[?25h"); // show cursor
            break;
        }

        let response = rpc::send_request(socket, "system.processes", serde_json::json!({}), None);
        match response {
            Err(e) => {
                eprintln!("jmux top: {e}");
                std::process::exit(1);
            }
            Ok(resp) => {
                // Clear screen and render table.
                print!("\x1b[2J\x1b[H"); // clear screen, cursor home
                println!(
                    "jmux top — refreshing every {}s  (Ctrl+C to quit)\n",
                    interval
                );

                let processes = resp
                    .get("result")
                    .and_then(|r| r.get("processes"))
                    .and_then(|p| p.as_array());

                match processes {
                    None => println!("(no data — is jmux running?)"),
                    Some(procs) if procs.is_empty() => {
                        println!("(no terminal panels with TTY information)")
                    }
                    Some(procs) => {
                        // Sort by cpu_percent descending.
                        let mut rows: Vec<&serde_json::Value> = procs.iter().collect();
                        rows.sort_by(|a, b| {
                            let ca = a["cpu_percent"].as_f64().unwrap_or(0.0);
                            let cb = b["cpu_percent"].as_f64().unwrap_or(0.0);
                            cb.partial_cmp(&ca).unwrap_or(std::cmp::Ordering::Equal)
                        });

                        println!(
                            "{:<20} {:<16} {:<16} {:>7} {:>10} {:>8}  Status",
                            "Workspace", "Panel", "Command", "CPU%", "Mem (MB)", "PID",
                        );
                        println!("{}", "-".repeat(92));

                        for row in &rows {
                            let ws = row["workspace_name"].as_str().unwrap_or("");
                            let panel = row["panel_id"].as_str().unwrap_or("");
                            let cmd = row["command"].as_str().unwrap_or("");
                            let cpu = row["cpu_percent"].as_f64().unwrap_or(0.0);
                            let mem = row["rss_mb"].as_f64().unwrap_or(0.0);
                            let pid = row["pid"].as_u64().unwrap_or(0);
                            let status = row["status"].as_str().unwrap_or("");
                            let ws_name = row["workspace_name"].as_str().unwrap_or(ws);
                            println!(
                                "{:<20} {:<16} {:<16} {:>7.1} {:>10.1} {:>8}  {}",
                                trunc(ws_name, 20),
                                trunc(panel, 16),
                                trunc(cmd, 16),
                                cpu,
                                mem,
                                pid,
                                status,
                            );
                        }
                    }
                }
            }
        }

        // Sleep for interval, waking up every 100ms to check for Ctrl+C.
        let ticks = interval * 10;
        for _ in 0..ticks {
            if INTERRUPTED.load(Ordering::Relaxed) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    Ok(())
}

fn trunc(s: &str, max: usize) -> &str {
    let bytes = s.as_bytes();
    if bytes.len() <= max {
        s
    } else {
        // Find last char boundary at or before max.
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}
