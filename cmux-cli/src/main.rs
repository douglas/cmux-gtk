//! cmux CLI — command-line client for the cmux socket API.

mod commands;
mod format;
mod rpc;

use clap::Parser;
use commands::*;

#[derive(Parser)]
#[command(name = "cmux", about = "cmux terminal multiplexer CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Socket path override
    #[arg(long, default_value_t = rpc::default_socket_path(), global = true)]
    socket: String,

    /// Output raw JSON
    #[arg(long, global = true)]
    json: bool,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Local commands that don't need the socket
    if let Commands::Themes { filter } = &cli.command {
        return format::run_themes(filter.as_deref());
    }

    let (method, params) = match &cli.command {
        Commands::Themes { .. } => unreachable!(),
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
            WorkspaceCommands::New { directory, title } => (
                "workspace.new",
                serde_json::json!({"directory": directory, "title": title}),
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
                serde_json::json!({"title": title, "workspace": workspace}),
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
                "title": title, "body": body, "workspace": workspace,
                "surface": surface, "send_desktop": !no_desktop,
            }),
        ),
    };

    let response = rpc::send_request(&cli.socket, method, params)?;

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
