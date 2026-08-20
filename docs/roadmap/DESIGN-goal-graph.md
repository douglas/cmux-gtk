# Design: `jmux goal` / `jmux graph` — autonomous loop system

Status: approved design, phased build in progress.
Reviewed by: Fable 5 adversarial design review (2026-07-27); all CRITICAL and
IMPORTANT findings are folded in below.

## Vision

Two commands turn jmux into an autonomous, human-gated engineering loop:

- `jmux goal <goal.md>` — one workspace, one **master agent** working a goal to
  completion, its Task-tool sub-agents mirrored beside it, iterating until done.
- `jmux graph <name> --goal <top.md>` — an **orchestrator agent** decomposes a
  top-level goal into a DAG of sub-goals; jmux's deterministic scheduler runs
  the nodes (in parallel where the DAG allows), each node itself an iterating
  `goal`; humans approve the plan and (optionally) each iteration through
  editor-based review gates in the UI.

Three layers, each owning what it is best at:

| Layer     | Owner                | Responsibility |
|-----------|----------------------|----------------|
| Design    | Orchestrator agent   | Decompose goal into DAG; re-plan from review feedback |
| Approval  | Human (review gates) | Approve/edit the DAG; optionally gate node iterations |
| Execution | jmux (Rust)          | Schedule, launch, drive, detect completion, merge, persist |

## Core principles (from review)

1. **Files are the source of truth, not signals.** A goal iteration is complete
   when `iteration-N.md` exists with valid front matter (`status: done|blocked`).
   The scheduler polls for files. `jmux goal complete` is an optional fast-path
   ping, never the only signal. This makes completion restart-safe and deletes
   the sentinel-attribution problem (Task sub-agents inherit the master's
   `JMUX_PANEL_ID`, so env attribution alone is ambiguous).
2. **Single writer per file.** The scheduler owns `graph.json`. The orchestrator
   *proposes* (`proposal.json`/`proposal.md`). `graph.md` is generated output.
   Humans edit through gates; gates write via the scheduler.
3. **Seed via argv, never via injected keystrokes.** Masters launch as
   `<runner command> "<seed>"` so there is no TUI-timing race. `send_input` is
   reserved for the auto-driver's bounded nudges.
4. **jmux stamps identity at launch.** `goal.create` generates the session UUID
   itself (for Claude runners: `--session-id <uuid>`) and sets
   `panel.agent_session_id` directly — sub-agent mirroring, crash-restore
   resume, and driver bookkeeping all key off it. (`workspace.create command=`
   panes never run shell integration, so the wrapper-based reporting path does
   not apply — verified.)
5. **UI and CLI are the same state machine.** Every UI button writes the same
   file / calls the same socket method the CLI verb would. No drift possible.

## Runners — which agent + model executes a goal

Every goal (and every graph node) is executed by a **runner**: a named
combination of agent CLI, model, effort, and launch semantics.

### Settings schema (`settings.json` → `goal.runners`)

```jsonc
"goal": {
  "default_runner": "claude-default",
  "runners": {
    "claude-default":  { "agent": "claude", "model": "",            "effort": "" },
    "opus-high":       { "agent": "claude", "model": "opus",        "effort": "high" },
    "fable":           { "agent": "claude", "model": "fable",       "effort": "" },
    "codex-sol":       { "agent": "custom", "model": "gpt-sol",
                         "command_template": "codex exec --model {model} {seed_file}",
                         "state_detection": "none" }
  }
}
```

### Built-in adapters

- **`claude`** — launches
  `claude --session-id {sid} [--model {model}] [--permission-mode <mode>] {seed}`;
  effort (when set) is applied via the model alias (`opus`, `sonnet`, …) plus
  Claude Code's effort mechanism; full `ClaudeState` detection, sub-agent
  mirroring, and nudge support.
- **`custom`** — a `command_template` with `{sid}`, `{model}`, `{effort}`,
  `{seed}` (shell-quoted inline) or `{seed_file}` (path to a file containing
  the seed) placeholders. Capability flags degrade gracefully:
  - `state_detection: "claude" | "none"` — with `none`, the auto-driver never
    nudges and completion relies purely on iteration-file polling + process
    liveness (which is fully supported by design principle 1).
  - Sub-agent mirroring is Claude-only (transcript format); other runners
    simply don't get monitor panes.

### Selection surface

- CLI: `jmux goal <path> --runner opus-high`, or ad hoc
  `--agent claude --model fable --effort high`.
- Graph: each node in `proposal.md`/`graph.json` carries `runner:`; the
  orchestrator proposes runner assignments (guidance tells it what's available
  and their cost/strength trade-offs); humans re-assign at Gate 1 by editing
  the line. Unset ⇒ `default_runner`.
- UI: node chip context menu → "Runner ▸" submenu; review editor shows the
  `runner:` line per node.

## The `goal` primitive

### Launch (`jmux goal <path>` → socket `goal.create`)

1. cwd = nearest git root above `goal.md` (fallback: its directory);
   `--cwd` overrides.
2. Seed = goal text + guidance template (below) + optional upstream context
   *by file reference* (never inlined — avoids the 128 KiB send_input cap and
   keeps context small).
3. Create workspace (`directory=cwd`, title `goal: <name>`), first panel
   command = runner launch command, `agent_session_id` stamped (claude
   runners), sub-agent monitor enabled for the workspace.
4. Register the goal with the in-app **goal registry** (drives ticker,
   status, iteration bookkeeping). Registry state is persisted in the session
   snapshot so restarts reconcile instead of forgetting.

### Guidance template

Shipped default embedded in the binary; user-overridable at
`~/.config/jmux/goal-guidance.md` (checked first). Placeholders: `{iteration}`,
`{output_path}`, `{goal_name}`, `{upstream_refs}`. Core contract it instructs:

- Work the goal; delegate to sub-agents as useful.
- Write `{output_path}` (= `docs/roadmap/…/iteration-{iteration}.md`) with
  front matter `status: done|blocked` and exactly four sections:
  1. **Goal** — the goal as understood.
  2. **Critique** — what was unclear/ambiguous/underspecified.
  3. **What I did** — summary of work + outcome.
  4. **Feedback for next iteration** — improvements to feed forward.
- Optionally run `jmux goal complete` afterwards (fast-path notification).

### Completion detection (layered, restart-safe)

1. **Primary:** registry ticker polls for `iteration-N.md`; front matter parsed
   (missing/malformed file after the agent goes idle ⇒ protocol violation ⇒
   escalate, never hang).
2. **Fast path:** `jmux goal complete [--status …]` socket call.
3. **Runner exit:** master process exit without an iteration file ⇒ `blocked:
   exited`.

### Auto-driver (per goal workspace, 2 s ticker like agent_monitor)

- Runner must have `state_detection: claude`; otherwise driver is poll-only.
- **Idle** (`ClaudeState == None`) for **K consecutive ticks** (debounce,
  default 5 = 10 s) **and** a live claude process verified on the panel (the
  existing `/proc` walk keyed on `JMUX_PANEL_ID`) **and** no iteration file yet
  ⇒ send one nudge ("continue working toward the goal; when finished write the
  iteration file"). Nudge budget default 3; exhausted ⇒ `needs-attention`
  escalation, driver stops nudging.
- **No live claude process** ⇒ never send anything (an idle *shell* would
  execute the nudge as a command — verified hazard). Treat as runner exit.
- **NeedsInput** (STRICT form only: pointer-menu / "enter to select" footer —
  the "ends with ?" heuristic is too twitchy for orchestration) ⇒ pause,
  notify. Never blind-nudge a menu.
- **Caps:** per-goal wall-clock cap (default 2 h) and nudge budget; either
  exhausted ⇒ `blocked: timeout` / `needs-attention`.
- **Notifications** rate-limited per goal (digest, min 60 s between).

### Iteration loop

`--max-iterations N` (default 1 for bare goals; graphs default 4). After
iteration i completes with `status != done` and i < N: fresh session (new
UUID), same workspace, seed = guidance + *file reference* to iteration-i.md's
Feedback. `status: done` or cap ⇒ goal finished.

### Permissions

Default `--permission-mode acceptEdits` (claude adapter). `--full-auto` opts
into bypass; `--supervised` leaves stock prompting (accepting babysitting).
Security note recorded: edge payloads feed upstream agent output into
downstream prompts — bypass mode amplifies prompt-injection blast radius, so
bypass is per-invocation opt-in, never a settings default.

Resolved mode = per-invocation flag > runner `permission_mode` >
`goal.permission_mode` > `acceptEdits`; an unknown value warns and falls
through. `bypassPermissions` is *enforced* out of both config sources
(`goal::validate_permission_mode`), because a runner name can be picked by
the planning agent and a settings default applies to runs nobody opted in
for. The gap between "asks about everything" and "asks about nothing" is
closed by per-runner `allowed_tools` → `claude --allowedTools`: pre-approve
read/build/test commands so `acceptEdits` runs unattended while destructive
commands still prompt and escalate.

Claude Code's first-run workspace-trust dialog is left to escalate. The only
non-interactive pre-trust is `projects[<path>].hasTrustDialogAccepted` in
`~/.claude.json` — a live, credential-bearing config file rewritten wholesale
by every running claude session; jmux will not read-modify-write it to save
one keypress. Documented alternative: trust covers ancestors, so trusting the
directory *above* the repo covers every `<repo>-worktrees/<node>`.

### `--wait`

Client-side **polling** of `goal.status` (1 s), because the socket protocol is
strict request/response with a 10 s client read timeout. Survives app
restarts/socket drops transparently. Prints the iteration file path on
completion; exit code reflects `done` (0) / `blocked` (2).

## The `graph` system

### Files (per graph, under `docs/roadmap/<name>/`)

- `proposal.json` + `proposal.md` — orchestrator-written (only file it writes).
- `graph.json` — **scheduler-owned** authoritative state: nodes
  `{id, goal (inline text or path), deps[], runner, status, workspace_id?,
  session_id?, iteration, worktree?}`, edges, gate config, caps, graph status.
- `graph.md` — generated, read-only rendering (`jmux graph status`).
- `<node-id>/iteration-N.md` — per-node iteration files.

### State machines

- Node: `pending → ready → running → review → done | blocked`
  (`review` only when the iteration gate is on)
- Graph: `proposing → proposed → running → complete | paused | failed`

### Scheduler (in-app, deterministic)

- On approve: copy proposal → `graph.json`, mark ready set.
- Launch ready nodes up to `--max-concurrency` (default **1**; >1 requires
  worktrees, below). Each node = `goal.create` with the node's runner,
  upstream iteration files as `{upstream_refs}`.
  - With background spawn landed, K>1 no longer costs K focus steals, so the
    default of 1 is now a *cost/merge-risk* choice rather than a UI limitation.
    Revisit the default separately — not changed in this phase.
- Node completion (file-based) ⇒ merge step ⇒ re-evaluate ready set.
- `blocked` node ⇒ halt dependents, escalate. Graph-level caps: max total
  iterations, wall clock.
- **Restart reconciliation:** on app start, graphs with `running` nodes are
  reconciled: live workspace+session ⇒ resume tracking; gone ⇒ mark
  `interrupted`, require `jmux graph resume`.

### Concurrency & worktrees

`--max-concurrency K>1` ⇒ each node runs in a scheduler-managed
`git worktree` at `<repo>-worktrees/<node-id>` on branch
`graph/<name>/<node-id>`. On node completion the scheduler merges the branch
into the graph's base branch; **conflicts are never auto-resolved** — node
goes `blocked: merge-conflict`, escalation with "Open worktree" action.
Iteration files are written inside the worktree and merged with the branch.

### Gates

- **Gate 1 — decomposition review** (default ON, `--no-review` to skip):
  graph pauses at `proposed`. UI opens `proposal.md` in the review editor
  (below). Approve & Run / Request Revision / Discard.
- **Gate 2 — iteration review** (default OFF, `--review-iterations`):
  node pauses at `review` after each iteration file; editor opens it;
  Save & Continue (edited Feedback seeds next iteration) / Accept & Finish
  Node (merge + unblock dependents) / Open Diff.
- CLI equivalents: `jmux graph approve|revise|note <node> "…"`,
  `jmux goal continue|accept`.

### Background spawn — LANDED

ghostty surfaces spawn their command on first resize, which requires the widget
to be mapped and visible — so background workspaces used to run nothing until
visited, and the scheduler had to *select* every node workspace on launch.

**Headless spawn** now starts them without visibility. Mechanism:

- `GhosttyGlSurface::spawn_headless(w_px, h_px)` (ghostty-gtk) creates the
  `ghostty_surface_t` — which is what opens the pty and spawns the child —
  outside the allocation path, then applies the synthetic size by hand with
  `ghostty_surface_set_size`. GTK4 realizes on *map*, so an unmapped surface has
  no GL context and `ghostty_surface_new` would fail in `surfaceInit` (libghostty
  loads GLAD from the current context); `spawn_headless` therefore calls
  `realize()` itself, which walks up the parent chain and creates a context
  without mapping or allocating anything (it will even realize a
  never-presented window — nothing appears on screen).
- `AppState::spawn_panel_headless(panel_id)` (jmux) parks the surface in the
  **spawn nursery**: a `GtkBox` page of the content `GtkStack` that is never made
  the visible child, so it is never allocated and the "visuals only touch the
  visible page" invariant is untouched. When the workspace is finally opened, the
  normal page build reparents the surface out of the nursery; its first real
  allocation only *resizes* the already-running terminal.
- A spawn-once guard (`spawn_started`, set before `ghostty_surface_new` and
  cleared only when creation failed) means the two paths are mutually exclusive:
  a later real allocation never spawns a second child.
- Drivers: the goal driver (`ensure_agent_spawned`, every tick) for node runs,
  and `graph::ensure_orchestrators_spawned` for the orchestrator (which is
  selected but may still be unmapped with the quake window down).

Consequences: `launch_goal` takes `select`, false for scheduler-launched nodes
(`TabManager::add_workspace_keep_selection` honours `new_workspace_placement`
without moving the user's selection), and the driver's "never spawned" watchdog
is now a fault report rather than an instruction to open the workspace.
`read_screen_text` needs no mapping (it reads terminal state, not pixels), so
the driver reads and nudges background agents normally.

## UI (mockups: goal-graph-ui-mockups artifact)

- **Graph panel** (`PanelType::Graph`): DAG chips colored by node state,
  clickable (jump) with context menu (Unblock / Skip / View iterations /
  Runner ▸ / Open worktree); header pill = graph state; slot counter;
  ⏸ ■ ⋮ controls. Renders `graph.json`; refreshed by scheduler tick.
- **Review editor**: the notes-panel editor pointed at the gate file +
  a new **review action bar** (`GtkActionBar`) whose buttons call the same
  socket methods as the CLI verbs.
- **Sidebar**: nodes join sidebar group `<name>`; ClaudeState sprites; amber
  `review` badge on group and row while a gate is pending; pending nodes
  dimmed.
- **Notifications**: existing notification center; every escalation carries a
  jump action (Review / Jump to pane / Open worktree).

## New surface area

*Superseded by the CLI reference in [`../GOAL-GRAPH.md`](../GOAL-GRAPH.md) §3
— both launch verbs now take the goal as a file **or** as inline text, runs are
addressed by name rather than by workspace UUID, `complete` is spelled
`report`, and `goal stop` / `continue --note` / `goal --plan` exist. The
socket methods below all still work; `goal.stop` joined them.*

CLI: `jmux goal <path> [--wait --cwd --max-iterations --runner --agent
--model --effort --full-auto --supervised --graph <name> --node <id>]`,
`jmux goal complete|status|continue|accept`, `jmux graph <name> --goal <top.md>
[--max-concurrency --max-iterations --no-review --review-iterations]`,
`jmux graph approve|revise|note|status|stop|resume`.

Socket: `goal.create`, `goal.status`, `goal.complete`, `goal.continue`,
`goal.accept`, `goal.stop`, `graph.create`, `graph.approve`, `graph.revise`,
`graph.status`, `graph.stop`, `graph.resume`.

## Build phases

- **Phase 0 — infrastructure**: runner registry + claude adapter;
  session-id-stamped launch; liveness-checked, debounced idle detection;
  permission-mode plumbing. (Headless spawn deferred to Phase 2½ — bare
  `jmux goal` creates the workspace selected, so it spawns visibly.)
- **Phase 1 — `jmux goal` end-to-end**: goal registry + ticker; guidance
  template (overridable); file-based completion; auto-driver; `goal.create/
  status/complete`; CLI with polling `--wait`; iteration loop.
- **Phase 2 — graph state + serial scheduler**: `graph.json` schema (frozen as
  the orchestrator contract), state machines, K=1 execution of hand-authored
  graphs, restart reconciliation. **2½:** headless spawn.
- **Phase 3 — parallelism**: worktrees + merge step, K>1, edge payload refs,
  sidebar grouping.
- **Phase 4 — AI decomposition + Gate 1**: orchestrator guidance template,
  proposal contract, review editor + action bar, revise loop.
- **Phase 5 — Gate 2 + graph panel polish**: iteration gates, `PanelType::Graph`
  DAG rendering, runner submenu, notification rate-limit digest.
