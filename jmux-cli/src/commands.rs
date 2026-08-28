use clap::Subcommand;

#[derive(Subcommand)]
pub enum Commands {
    /// Ping the jmux server
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

    /// Tab operations
    #[command(subcommand)]
    Tab(TabCommands),

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

    /// Identify the jmux server (platform, version)
    Identify,

    /// Show the layout tree for all workspaces
    Tree,

    /// Open the settings window
    Settings,

    /// Show sidebar state (selected workspace)
    SidebarState,

    /// Browser automation
    #[command(subcommand)]
    Browser(BrowserCommands),

    /// Markdown panel
    #[command(subcommand)]
    Markdown(MarkdownCommands),

    /// Agent conversation management
    #[command(subcommand)]
    Agent(AgentCommands),

    /// List available Ghostty terminal themes
    Themes {
        /// Filter themes by name (case-insensitive substring match)
        #[arg(long, short)]
        filter: Option<String>,
    },

    /// Configuration management
    #[command(subcommand)]
    Config(ConfigCommands),

    /// Show a live process table for all workspace terminals (like top)
    Top {
        /// Refresh interval in seconds (default: 2)
        #[arg(long, short, default_value_t = 2)]
        interval: u64,
    },

    /// Control the sidebar visibility
    #[command(subcommand)]
    Sidebar(SidebarCommands),

    /// Goal-driven agent workspaces (autonomous loop)
    #[command(args_conflicts_with_subcommands = true)]
    Goal {
        #[command(subcommand)]
        command: Option<GoalCommands>,
        /// Goal markdown file, or the goal text itself ("add a --version flag")
        goal: Option<String>,
        /// Decompose the goal into a plan first (same engine as `jmux graph`)
        #[arg(long)]
        plan: bool,
        /// Name for the run (default: derived from the goal file or text)
        #[arg(long)]
        name: Option<String>,
        /// Block until the goal completes; prints the iteration file path
        #[arg(long, conflicts_with = "plan")]
        wait: bool,
        /// Working directory (default: nearest git root above the goal file,
        /// or above your current directory for inline goal text)
        #[arg(long)]
        cwd: Option<String>,
        /// Named runner from settings (goal.runners)
        #[arg(long)]
        runner: Option<String>,
        /// Agent adapter override: claude | custom
        #[arg(long)]
        agent: Option<String>,
        /// Model override (e.g. opus, fable, sonnet)
        #[arg(long)]
        model: Option<String>,
        /// Effort override (e.g. high)
        #[arg(long)]
        effort: Option<String>,
        /// Auto-continue up to N iterations while the agent reports blocked
        /// (default 3; settings key goal.max_iterations)
        #[arg(long)]
        max_iterations: Option<u32>,
        /// Bypass all permission checks (opt-in; amplifies prompt-injection risk)
        #[arg(long)]
        full_auto: bool,
        /// Stock interactive permission prompting (you babysit approvals)
        #[arg(long, conflicts_with = "full_auto")]
        supervised: bool,
        /// Workspace title (default: "goal: <name>")
        #[arg(long)]
        title: Option<String>,
        /// Open the Run goal dialog in jmux instead of starting the run, so
        /// you can set the roles, models and instructions first
        #[arg(long)]
        configure: bool,
    },

    /// DAG orchestration: decompose a goal into parallel goal workspaces
    #[command(args_conflicts_with_subcommands = true)]
    Graph {
        #[command(subcommand)]
        command: Option<GraphCommands>,
        /// Goal markdown file, or the goal text itself (with --goal: the
        /// graph name, the older spelling)
        goal_or_name: Option<String>,
        /// Path to the top-level goal markdown file (compatibility form:
        /// `jmux graph <name> --goal <top.md>`)
        #[arg(long)]
        goal: Option<String>,
        /// Graph name (default: the repo directory, or a slug of the goal text)
        #[arg(long)]
        name: Option<String>,
        /// Max nodes running at once
        #[arg(long)]
        max_concurrency: Option<u32>,
        /// Per-node iteration cap
        #[arg(long)]
        max_iterations: Option<u32>,
        /// Work in your checkout instead of a per-node git worktree
        #[arg(long)]
        no_worktrees: bool,
        /// Skip the plan review: start work as soon as the plan is written
        #[arg(long)]
        no_review: bool,
        /// Iteration review: pause after each node iteration so you can
        /// accept it or ask for another
        #[arg(long)]
        review_iterations: bool,
        /// Default runner for orchestrator + nodes (settings goal.runners)
        #[arg(long)]
        runner: Option<String>,
        /// Bypass all permission checks (opt-in; amplifies prompt-injection risk)
        #[arg(long)]
        full_auto: bool,
        /// Stock interactive permission prompting (you babysit approvals)
        #[arg(long, conflicts_with = "full_auto")]
        supervised: bool,
    },
}

#[derive(Subcommand)]
pub enum GraphCommands {
    /// Approve the proposal (re-reads proposal.json, so your edits count)
    Approve { name: String },
    /// Ask the orchestrator to revise the proposal
    Revise {
        name: String,
        /// What to change
        #[arg(long)]
        note: String,
    },
    /// Show graph state (all graphs, or one)
    Status { name: Option<String> },
    /// Stop launching new nodes (running nodes continue)
    Pause { name: String },
    /// Relaunch interrupted nodes / retry pending merges
    Resume { name: String },
    /// Stop the graph permanently
    Stop { name: String },
    /// Reviewer verdict on one node: run another iteration
    Continue {
        name: String,
        node: String,
        /// Steering note injected into the next iteration's prompt
        #[arg(long)]
        note: Option<String>,
    },
    /// Reviewer verdict on one node: accept the iteration as final
    Accept { name: String, node: String },
}

#[derive(Subcommand)]
pub enum GoalCommands {
    /// Agent-facing fast path: record the current iteration's outcome
    /// (the iteration file's front matter is the source of truth).
    /// Spelled `complete` before; both work.
    #[command(alias = "complete")]
    Report {
        /// Goal name ("<graph>/<node>") or workspace UUID
        /// (default: this pane's run, else the only active run)
        target: Option<String>,
        /// done | blocked (fallback when the file has no front matter)
        #[arg(long)]
        status: Option<String>,
        /// Goal workspace UUID (compatibility alias for the positional)
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Show goal run status (everything running, or one run)
    Status {
        /// Goal name ("<graph>/<node>") or workspace UUID
        target: Option<String>,
        /// Goal workspace UUID (compatibility alias for the positional)
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Reviewer verdict: run another iteration (seeded with the current
    /// iteration file's feedback — edit section 4, or pass --note, to steer it)
    Continue {
        /// Goal name ("<graph>/<node>") or workspace UUID
        /// (default: this pane's run, else the only active run)
        target: Option<String>,
        /// Steering note injected into the next iteration's prompt
        #[arg(long)]
        note: Option<String>,
        /// Goal workspace UUID (compatibility alias for the positional)
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Reviewer verdict: accept the current iteration as final
    /// (graph nodes merge + unblock dependents)
    Accept {
        /// Goal name ("<graph>/<node>") or workspace UUID
        /// (default: this pane's run, else the only active run)
        target: Option<String>,
        /// Goal workspace UUID (compatibility alias for the positional)
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Stop driving a run: it stops being nudged and iterated
    /// (the workspace stays open, so nothing is lost)
    Stop {
        /// Goal name ("<graph>/<node>") or workspace UUID
        /// (default: this pane's run, else the only active run)
        target: Option<String>,
        /// Goal workspace UUID (compatibility alias for the positional)
        #[arg(long)]
        workspace: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum SidebarCommands {
    /// Show the sidebar
    Show,
    /// Hide the sidebar
    Hide,
    /// Toggle sidebar visibility
    Toggle,
    /// Show current sidebar state
    Status,
}

#[derive(Subcommand)]
pub enum ConfigCommands {
    /// Validate the configuration file
    Doctor,
    /// Print the config file path
    Path,
    /// Show documentation URL
    Docs,
    /// Reload config in running instance
    Reload,
}

#[derive(Subcommand)]
pub enum WorkspaceCommands {
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
        /// Initial panel layout: single (default), horizontal-2, vertical-2, grid-4
        #[arg(long, default_value = "single")]
        layout: String,
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
    /// Batch-reorder workspaces by name or index
    ReorderWorkspaces {
        /// Desired workspace order (names or 0-based indices). Unlisted workspaces keep their relative position at the end.
        workspaces: Vec<String>,
        /// Print the proposed change without applying it
        #[arg(long)]
        dry_run: bool,
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
    /// Perform an action on a workspace
    Action {
        /// Action: pin, unpin, toggle_pin, mark_read, mark_unread, clear_name, set_color, clear_color, rename, move_up, move_down, move_top, close_others, close_above, close_below
        action: String,
        /// Target workspace UUID
        #[arg(long)]
        workspace: Option<String>,
        /// Color value (required for set_color)
        #[arg(long)]
        color: Option<String>,
        /// Title (required for rename)
        #[arg(long)]
        title: Option<String>,
    },
    /// Report working directory for a panel
    ReportPwd {
        /// Directory path
        directory: String,
        /// Panel UUID
        #[arg(long)]
        panel: Option<String>,
        /// Target workspace UUID
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Report listening ports for a panel
    ReportPorts {
        /// Port numbers
        ports: Vec<u16>,
        /// Panel UUID
        #[arg(long)]
        panel: Option<String>,
    },
    /// Clear listening ports for a panel
    ClearPorts {
        /// Panel UUID
        #[arg(long)]
        panel: Option<String>,
    },
    /// Report TTY name for a panel
    ReportTty {
        /// TTY device path (e.g. /dev/pts/0)
        tty: String,
        /// Panel UUID
        #[arg(long)]
        panel: Option<String>,
    },
    /// Trigger port scanning (no-op on Linux, API parity)
    PortsKick,
    /// Report git branch for workspace
    ReportGit {
        /// Branch name
        branch: String,
        /// Whether the working tree is dirty
        #[arg(long)]
        dirty: bool,
    },
    /// Enable or disable iMessage chat-bubble mode for a workspace
    ImessageMode {
        /// Enable iMessage mode
        #[arg(long, conflicts_with = "disable")]
        enable: bool,
        /// Disable iMessage mode
        #[arg(long, conflicts_with = "enable")]
        disable: bool,
        /// Target workspace UUID (defaults to selected)
        #[arg(long)]
        workspace: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum NotificationCommands {
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
    /// List notifications
    List {
        /// Only show unread notifications
        #[arg(long)]
        unread: bool,
    },
    /// Clear all notifications
    Clear,
    /// Mark a notification as read
    MarkRead {
        /// Notification UUID
        id: String,
    },
    /// Dismiss (remove) a notification
    Dismiss {
        /// Notification UUID
        id: String,
    },
    /// Open (focus) the source panel/workspace of a notification
    Open {
        /// Notification UUID
        id: String,
    },
}

#[derive(Subcommand)]
pub enum SurfaceCommands {
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
    /// Send a key event to a terminal
    SendKey {
        /// Key name (e.g. "c", "Return", "Escape", "F1")
        key: String,
        /// Modifier keys (ctrl, shift, alt, super)
        #[arg(long, value_delimiter = ',')]
        mods: Vec<String>,
        /// Surface/panel UUID (sends to focused panel if not specified)
        #[arg(long)]
        surface: Option<String>,
    },
    /// Read the visible screen text from a terminal
    ReadScreen {
        /// Surface/panel UUID (reads focused panel if not specified)
        #[arg(long)]
        surface: Option<String>,
    },
    /// Flash a surface to attract attention
    Flash {
        /// Surface/panel UUID (flashes focused panel if not specified)
        #[arg(long)]
        surface: Option<String>,
    },
    /// Split the focused pane (alias for pane new)
    Split {
        /// Split orientation: horizontal or vertical
        #[arg(long, default_value = "horizontal")]
        orientation: String,
    },
    /// Close a surface (alias for pane close)
    Close {
        /// Surface/panel UUID (closes focused panel if not specified)
        id: Option<String>,
    },
    /// Refresh a terminal surface
    Refresh {
        /// Surface/panel UUID (refreshes focused panel if not specified)
        #[arg(long)]
        surface: Option<String>,
    },
    /// Clear the scrollback history of a terminal
    ClearHistory {
        /// Surface/panel UUID (clears focused panel if not specified)
        #[arg(long)]
        surface: Option<String>,
    },
    /// Perform an action on a surface (toggle_zoom, clear_screen, refresh, flash)
    Action {
        /// Action name
        action: String,
        /// Surface/panel UUID
        #[arg(long)]
        surface: Option<String>,
    },
    /// Check health/readiness of a surface
    Health {
        /// Surface/panel UUID
        #[arg(long)]
        surface: Option<String>,
    },
    /// Move a panel to a different workspace
    Move {
        /// Panel UUID (moves focused panel if not specified)
        #[arg(long)]
        panel: Option<String>,
        /// Target workspace UUID
        #[arg(long)]
        workspace: String,
        /// Split orientation when inserting: horizontal or vertical
        #[arg(long, default_value = "horizontal")]
        orientation: String,
    },
    /// Reorder a panel within its pane tabs
    Reorder {
        /// Panel UUID (reorders focused panel if not specified)
        #[arg(long)]
        panel: Option<String>,
        /// New tab index (0-based)
        index: usize,
    },
    /// Create a new surface (tabbed in the focused pane, not split)
    Create {
        /// Panel type: terminal or browser
        #[arg(long, default_value = "terminal")]
        r#type: String,
    },
    /// Move a surface into a new split pane in the given direction
    DragToSplit {
        /// Direction: left, right, up, down
        direction: String,
        /// Surface/panel UUID (moves focused panel if not specified)
        #[arg(long)]
        surface: Option<String>,
    },
}

/// Tab operations
#[derive(Subcommand)]
pub enum TabCommands {
    /// Perform an action on a tab/surface
    Action {
        /// Action: rename, clear_name, close_left, close_right, close_others, pin, unpin, mark_read, mark_unread, duplicate
        action: String,
        /// Surface/panel UUID (uses focused panel if not specified)
        #[arg(long)]
        surface: Option<String>,
        /// Title (for rename action)
        #[arg(long)]
        title: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum PaneCommands {
    /// Create a new split pane
    New {
        /// Split orientation: horizontal or vertical
        #[arg(long, default_value = "horizontal")]
        orientation: String,
    },
    /// Create a new pane (alias for new)
    Create {
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
    /// Break focused pane into a new workspace
    Break {
        /// Panel UUID (breaks focused panel if not specified)
        #[arg(long)]
        panel: Option<String>,
    },
    /// Join a pane from another workspace into the current workspace
    Join {
        /// Panel UUID to join
        id: String,
        /// Split orientation: horizontal or vertical
        #[arg(long, default_value = "horizontal")]
        orientation: String,
    },
    /// Equalize all split dividers in the current workspace
    Equalize {
        /// Target workspace UUID
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Split the focused pane without changing focus (focus stays on the active panel)
    SplitOff {
        /// Split orientation: horizontal or vertical
        #[arg(long, default_value = "horizontal")]
        direction: String,
    },
    /// List surfaces (panels) in the pane containing a panel
    Surfaces {
        /// Panel UUID (uses focused panel if not specified)
        #[arg(long)]
        panel: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum BrowserCommands {
    /// Navigate a browser panel to a URL
    Navigate {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// URL to navigate to
        url: String,
    },
    /// Execute JavaScript in a browser panel
    ExecuteJs {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// JavaScript code to execute
        script: String,
    },
    /// Get the current URL of a browser panel
    GetUrl {
        /// Panel UUID
        #[arg(long)]
        panel: String,
    },
    /// Get the page text content of a browser panel
    GetText {
        /// Panel UUID
        #[arg(long)]
        panel: String,
    },
    /// Go back in browser history
    Back {
        /// Panel UUID
        #[arg(long)]
        panel: String,
    },
    /// Go forward in browser history
    Forward {
        /// Panel UUID
        #[arg(long)]
        panel: String,
    },
    /// Reload the browser page
    Reload {
        /// Panel UUID
        #[arg(long)]
        panel: String,
    },
    /// Set browser zoom level
    SetZoom {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// Zoom level (0.25-5.0, default 1.0)
        zoom: f64,
    },
    /// Take a screenshot (HTML preview)
    Screenshot {
        /// Panel UUID
        #[arg(long)]
        panel: String,
    },
    /// Mute or unmute a browser panel's audio (toggles if --muted omitted)
    Mute {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// Explicit muted state; omit to toggle
        #[arg(long)]
        muted: Option<bool>,
    },
    /// Toggle distraction-free focus mode (hide browser chrome)
    FocusMode {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// Explicit enabled state; omit to toggle
        #[arg(long)]
        enabled: Option<bool>,
    },
    /// Grab a snapshot of the page's React component tree (best-effort)
    ReactGrab {
        /// Panel UUID
        #[arg(long)]
        panel: String,
    },

    // Phase 2: DOM interaction
    /// Click an element
    Click {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// CSS selector or @eN ref
        #[arg(long)]
        selector: String,
        /// Mouse button (left, right, middle)
        #[arg(long)]
        button: Option<String>,
    },
    /// Double-click an element
    Dblclick {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// CSS selector or @eN ref
        #[arg(long)]
        selector: String,
    },
    /// Hover over an element
    Hover {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// CSS selector or @eN ref
        #[arg(long)]
        selector: String,
    },
    /// Type text into an element (key by key)
    Type {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// CSS selector or @eN ref
        #[arg(long)]
        selector: String,
        /// Text to type
        #[arg(long)]
        text: String,
    },
    /// Fill an input element (set value directly)
    Fill {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// CSS selector or @eN ref
        #[arg(long)]
        selector: String,
        /// Value to fill
        #[arg(long)]
        value: String,
    },
    /// Clear an input element
    Clear {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// CSS selector or @eN ref
        #[arg(long)]
        selector: String,
    },
    /// Press a key on an element
    Press {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// CSS selector or @eN ref
        #[arg(long)]
        selector: String,
        /// Key name (e.g. Enter, Tab, Escape)
        #[arg(long)]
        key: String,
    },
    /// Select an option in a <select> element
    SelectOption {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// CSS selector or @eN ref
        #[arg(long)]
        selector: String,
        /// Option value
        #[arg(long)]
        value: Option<String>,
        /// Option label text
        #[arg(long)]
        label: Option<String>,
        /// Option index
        #[arg(long)]
        index: Option<u64>,
    },
    /// Check or uncheck a checkbox
    Check {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// CSS selector or @eN ref
        #[arg(long)]
        selector: String,
        /// Set checked state (default: true)
        #[arg(long)]
        checked: Option<bool>,
    },
    /// Focus an element
    Focus {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// CSS selector or @eN ref
        #[arg(long)]
        selector: String,
    },
    /// Blur (unfocus) an element
    Blur {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// CSS selector or @eN ref
        #[arg(long)]
        selector: String,
    },
    /// Scroll to a position or element
    ScrollTo {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// CSS selector or @eN ref (optional, scrolls window if omitted)
        #[arg(long)]
        selector: Option<String>,
        /// X coordinate
        #[arg(long)]
        x: Option<f64>,
        /// Y coordinate
        #[arg(long)]
        y: Option<f64>,
    },

    // Phase 3: Element queries
    /// Get HTML content of an element
    GetHtml {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// CSS selector or @eN ref
        #[arg(long)]
        selector: String,
        /// Get outerHTML instead of innerHTML
        #[arg(long)]
        outer: bool,
    },
    /// Get value of an input element
    GetValue {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// CSS selector or @eN ref
        #[arg(long)]
        selector: String,
    },
    /// Get an attribute of an element
    GetAttribute {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// CSS selector or @eN ref
        #[arg(long)]
        selector: String,
        /// Attribute name
        #[arg(long)]
        name: String,
    },
    /// Get a JavaScript property of an element
    GetProperty {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// CSS selector or @eN ref
        #[arg(long)]
        selector: String,
        /// Property name
        #[arg(long)]
        name: String,
    },
    /// Get bounding box of an element
    GetBoundingBox {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// CSS selector or @eN ref
        #[arg(long)]
        selector: String,
    },
    /// Get computed style of an element
    GetComputedStyle {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// CSS selector or @eN ref
        #[arg(long)]
        selector: String,
        /// CSS property name
        #[arg(long)]
        property: String,
    },
    /// Check if an element is visible
    IsVisible {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// CSS selector or @eN ref
        #[arg(long)]
        selector: String,
    },
    /// Check if an element is enabled
    IsEnabled {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// CSS selector or @eN ref
        #[arg(long)]
        selector: String,
    },
    /// Check if a checkbox is checked
    IsChecked {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// CSS selector or @eN ref
        #[arg(long)]
        selector: String,
    },
    /// Check if an element is editable
    IsEditable {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// CSS selector or @eN ref
        #[arg(long)]
        selector: String,
    },
    /// Count elements matching a selector
    Count {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// CSS selector
        #[arg(long)]
        selector: String,
    },

    // Phase 4: Finders
    /// Find an element and return a ref
    Find {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// CSS selector
        #[arg(long)]
        selector: String,
    },
    /// Find all elements matching a selector
    FindAll {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// CSS selector
        #[arg(long)]
        selector: String,
    },
    /// Find an element by text content
    FindByText {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// Text to search for
        #[arg(long)]
        text: String,
    },
    /// Find an element by ARIA role
    FindByRole {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// ARIA role
        #[arg(long)]
        role: String,
    },
    /// Find an element by aria-label
    FindByLabel {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// Label text
        #[arg(long)]
        label: String,
    },
    /// Find an element by placeholder text
    FindByPlaceholder {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// Placeholder text
        #[arg(long)]
        placeholder: String,
    },
    /// Find an element by data-testid
    FindByTestId {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// Test ID
        #[arg(long)]
        test_id: String,
    },
    /// Release an element ref
    ReleaseRef {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// Ref ID (e.g. @e1)
        #[arg(long)]
        ref_id: String,
    },

    // Phase 5: Advanced
    /// Wait for an element to appear
    WaitForSelector {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// CSS selector or @eN ref
        #[arg(long)]
        selector: String,
        /// Timeout in milliseconds
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// Wait for a navigation to complete
    WaitForNavigation {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// Timeout in milliseconds
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// Wait for page load to complete
    WaitForLoadState {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// Timeout in milliseconds
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// Wait for a JS expression to return truthy
    WaitForFunction {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// JavaScript expression
        #[arg(long)]
        expression: String,
        /// Timeout in milliseconds
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// Get full page HTML snapshot
    Snapshot {
        /// Panel UUID
        #[arg(long)]
        panel: String,
    },
    /// Get page title
    Title {
        /// Panel UUID
        #[arg(long)]
        panel: String,
    },
    /// Get cookies
    GetCookies {
        /// Panel UUID
        #[arg(long)]
        panel: String,
    },
    /// Set a cookie
    SetCookie {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// Cookie string (e.g. "name=value; path=/")
        #[arg(long)]
        cookie: String,
    },
    /// Clear all cookies
    ClearCookies {
        /// Panel UUID
        #[arg(long)]
        panel: String,
    },
    /// Get a localStorage value
    LocalStorageGet {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// Storage key
        #[arg(long)]
        key: String,
    },
    /// Set a localStorage value
    LocalStorageSet {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// Storage key
        #[arg(long)]
        key: String,
        /// Storage value
        #[arg(long)]
        value: String,
    },
    /// Get a sessionStorage value
    SessionStorageGet {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// Storage key
        #[arg(long)]
        key: String,
    },
    /// Set a sessionStorage value
    SessionStorageSet {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// Storage key
        #[arg(long)]
        key: String,
        /// Storage value
        #[arg(long)]
        value: String,
    },
    /// Get captured console messages
    GetConsoleMessages {
        /// Panel UUID
        #[arg(long)]
        panel: String,
    },
    /// Set dialog handler (auto-accept/dismiss alerts)
    SetDialogHandler {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// Action: "accept" or "dismiss"
        #[arg(long, default_value = "accept")]
        action: String,
        /// Text to fill in prompt dialogs
        #[arg(long)]
        text: Option<String>,
    },
    /// Inject a JavaScript script into the page
    InjectScript {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// JavaScript code
        #[arg(long)]
        script: String,
    },
    /// Inject a CSS stylesheet into the page
    InjectStyle {
        /// Panel UUID
        #[arg(long)]
        panel: String,
        /// CSS code
        #[arg(long)]
        css: String,
    },
    /// Remove all injected scripts and styles
    RemoveInjected {
        /// Panel UUID
        #[arg(long)]
        panel: String,
    },

    /// Import cookies from a local browser profile into jmux's browser
    ImportCookies {
        /// Browser to import from: firefox, chrome, chromium (default: firefox)
        #[arg(long, default_value = "firefox")]
        source: String,
        /// Profile name (reserved for future use; currently ignored)
        #[arg(long)]
        profile: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum MarkdownCommands {
    /// Open a markdown file in a new panel
    Open {
        /// Path to the markdown file
        file: String,
        /// Target workspace UUID
        #[arg(long)]
        workspace: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum AgentCommands {
    /// Fork the current agent conversation into a new workspace
    Fork {
        /// Initial message to pre-populate in the new terminal
        #[arg(long)]
        message: Option<String>,
        /// Name for the new workspace
        #[arg(long)]
        name: Option<String>,
    },
    /// Dispatch an agent lifecycle hook (namespaced by --cli)
    Hook {
        /// Hook event: session-start, session-stop, session-end, notification, subagent-start, subagent-stop
        event: String,
        /// Agent CLI name used to namespace status keys (e.g. claude, codex)
        #[arg(long, default_value = "agent")]
        cli: String,
        /// Notification message (for notification events; overrides stdin JSON)
        #[arg(long)]
        message: Option<String>,
        /// Parent panel UUID (required for subagent-start)
        #[arg(long)]
        parent: Option<String>,
        /// Panel UUID (required for subagent-stop)
        #[arg(long)]
        panel: Option<String>,
    },
    /// Render a subagent transcript read-only, Claude-CLI style, following
    /// new entries (used by jmux's sub-agent monitor panes)
    View {
        /// Path to the agent-*.jsonl transcript
        transcript: String,
    },
    /// Spawn a subagent panel next to a parent panel (Codex Teams)
    SpawnSubagent {
        /// Parent panel UUID
        #[arg(long)]
        parent_panel_id: String,
        /// CLI name for the subagent (e.g. codex)
        #[arg(long, default_value = "codex")]
        cli_name: String,
        /// Working directory for the subagent
        #[arg(long)]
        working_directory: Option<String>,
    },
}
