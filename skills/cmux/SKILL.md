---
name: cmux
description: End-user control of cmux topology and routing (windows, workspaces, panes/surfaces, focus, moves, reorder, identify, trigger flash). Use when automation needs deterministic placement and navigation in a multi-pane cmux layout.
---

# cmux Core Control

Use this skill to control non-browser cmux topology and routing.

## Core Concepts

- Window: top-level cmux window.
- Workspace: tab-like group within a window.
- Pane: split container in a workspace.
- Surface: a tab within a pane (terminal or browser panel).

## CLI Structure

All commands use grouped subcommands: `cmux <group> <action> [OPTIONS]`.

Groups: `workspace`, `surface`, `pane`, `tab`, `window`, `notification`, `browser`, `markdown`.

## Fast Start

```bash
# identify current caller context
cmux identify --json

# list topology
cmux window list
cmux workspace list
cmux pane list
cmux pane surfaces

# show full layout tree
cmux tree

# create workspace / surface / split
cmux workspace new                                        # returns workspace_id
cmux workspace select <INDEX>                             # select by 0-based index (positional arg, NOT --index)
cmux surface create --json              # new tab in CURRENTLY SELECTED workspace/pane
cmux surface split --orientation horizontal  # split focused pane
cmux pane new --orientation vertical

# send command to a surface (create + run)
cmux surface create --json              # → get panel_id from result
cmux surface send-text --surface <panel_id> 'my-command\n'

# focus / move / reorder
cmux surface focus <id>                             # positional UUID
cmux surface move --panel <id> --workspace <ws_id>  # both are flags
cmux surface reorder <INDEX> --panel <id>           # INDEX positional, --panel optional
cmux pane focus-direction right                     # direction is positional (left, right, up, down)

# attention cue
cmux surface flash --surface <id>

# read terminal output
cmux surface read-screen --surface <id>
```

## Handle Model

- Default output uses short refs: `window:N`, `workspace:N`, `pane:N`, `surface:N`.
- UUIDs are still accepted as inputs.
- `cmux surface create --json` returns `{ result: { panel_id: "uuid" } }`.

## Common Patterns

### Create New Workspace and Run Command In It
```bash
# surface create always targets the CURRENTLY SELECTED workspace
# so select it first, then create the surface
WS_ID=$(cmux workspace new --json | jq -r '.result.workspace_id')
WS_INDEX=$(cmux workspace list --json | jq -r --arg id "$WS_ID" '.result[] | select(.id == $id) | .index')
cmux workspace select "$WS_INDEX"
PANEL_ID=$(cmux surface create --json | jq -r '.result.panel_id')
cmux surface send-text --surface "$PANEL_ID" 'my-command\n'
```

### Create Tab and Run Command (in current workspace)
```bash
PANEL_ID=$(cmux surface create --json | jq -r '.result.panel_id')
cmux surface send-text --surface "$PANEL_ID" 'npm run dev\n'
```

### Split and Run Command
```bash
cmux surface split --orientation horizontal --json
# new surface gets focus, send text to focused surface
cmux surface send-text 'npm test\n'
```

### Break Pane to New Workspace
```bash
cmux pane break   # moves focused pane into its own workspace
```

### Join Pane from Another Workspace
```bash
cmux pane join --pane <pane_id>  # brings pane into current workspace
```

## Deep-Dive References

| Reference | When to Use |
|-----------|-------------|
| [references/handles-and-identify.md](references/handles-and-identify.md) | Handle syntax, self-identify, caller targeting |
| [references/windows-workspaces.md](references/windows-workspaces.md) | Window/workspace lifecycle and reorder/move |
| [references/panes-surfaces.md](references/panes-surfaces.md) | Splits, surfaces, move/reorder, focus routing |
| [references/trigger-flash-and-health.md](references/trigger-flash-and-health.md) | Flash cue and surface health checks |
| [../cmux-browser/SKILL.md](../cmux-browser/SKILL.md) | Browser automation on surface-backed webviews |
| [../cmux-markdown/SKILL.md](../cmux-markdown/SKILL.md) | Markdown viewer panel with live file watching |
