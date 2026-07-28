# Goal & Graph — autonomous agent loops

`jmux goal` runs one master agent against a goal file until it's done.
`jmux graph` has an orchestrator agent decompose a big goal into a DAG of
sub-goals, then executes them — in parallel where the DAG allows — with
human review gates. Design rationale: [`roadmap/DESIGN-goal-graph.md`](roadmap/DESIGN-goal-graph.md).

```
   goal.md ──► orchestrator ──► proposal.json ──► YOU review ──► scheduler
                (Claude)          (the DAG)        (Gate 1)         │
                                                        ┌───────────┼───────────┐
                                                        ▼           ▼           ▼
                                                     node A      node B      node C     ← parallel,
                                                    (worktree)  (worktree)  (worktree)    own branches
                                                        │           │           │
                                                 iteration-N.md files + merge back
```

---

## 1. The goal primitive

Write a goal file, launch it:

```sh
echo "Add a --version flag to the CLI, with a test" > ~/src/myrepo/goal.md
jmux goal ~/src/myrepo/goal.md
```

What happens:

1. jmux finds the git root above `goal.md` (that becomes the working dir).
2. A new workspace `goal: myrepo` opens with a master Claude already
   working — seeded with your goal plus an operating contract.
3. Its Task-tool sub-agents mirror into read-only panes beside it.
4. The **auto-driver** watches the pane: if the master idles it nudges it
   (max 3, only after verifying a live claude process); a real permission
   menu pauses and notifies you instead.
5. When finished, the master writes `docs/roadmap/iteration-1.md`:

```markdown
---
status: done          ← the machine-readable completion signal
iteration: 1
---
## 1. Goal        (as the agent understood it)
## 2. Critique    (what was unclear — improve your next goal file)
## 3. What I did
## 4. Feedback for next iteration
```

The **file is the source of truth** — `jmux goal complete` is just a
fast-path ping. `status: blocked` with `--max-iterations N` makes jmux
feed section 4 forward into another iteration automatically.

Useful flags:

| flag | effect |
|---|---|
| `--wait` | block until done; prints the iteration file; exit 0/2 |
| `--runner opus-high` | run with a named runner (see §4) |
| `--model fable --effort high` | ad-hoc runner override |
| `--max-iterations 3` | auto-iterate on `blocked` up to 3 loops |
| `--supervised` | stock permission prompts (you babysit) |
| `--full-auto` | bypass permissions (opt-in; see security note in the design doc) |

---

## 2. End-to-end graph demo

The worked example: a website with an interactive trail map.

### Step 0 — write the top-level goal (the only file you write)

```sh
mkdir ~/src/mapsite && cd ~/src/mapsite && git init
cat > goal.md <<'EOF'
Build a website with an interactive map of hiking trails.
- Pan/zoom map, trail data from GeoJSON files in data/
- Clicking a trail shows name, distance, elevation profile
- Mobile friendly, static hosting, tests, README.
EOF
git add . && git commit -m goal
```

### Step 1 — launch

```sh
jmux graph mapsite --goal ~/src/mapsite/goal.md --max-concurrency 3
```

A sidebar group **mapsite** appears containing one workspace:
the orchestrator agent on the left, the **graph panel** on the right.

```
┌─ graph: mapsite ────────────────────────────┬─ Graph ──────────── [proposing…] ─┐
│ ✻ Decomposing goal into a dependency graph… │                                   │
│ ⏺ Read(goal.md)                             │  Waiting for the orchestrator's   │
│ ⏺ Write(docs/roadmap/mapsite/proposal.json) │  proposal…                        │
│ ◇ Proposal ready — waiting for review.      │                                   │
└─────────────────────────────────────────────┴───────────────────────────────────┘
```

### Step 2 — Gate 1: review the proposal

When `proposal.json` lands you get a desktop notification and the panel
flips to the review gate:

```
┌─ Graph ──────────────────────────────────── [review gate] · 0/3 slots ─┐
│  [Open proposal]  [Approve & Run]                                      │
│                                                                        │
│   ( scaffold ○ )                                                       │
│   ( map-core ○ ) ( data-pipeline ○ ) ( tests-ci ○ )                    │
│   ( trail-interaction ○ )                                              │
│   ( polish-a11y-mobile ○ )                                             │
└────────────────────────────────────────────────────────────────────────┘
```

- **Open proposal** opens `proposal.json` in a notes editor pane. Edit
  node goals, deps, runners directly — `Ctrl+S`, then **Approve & Run**:
  approval *re-reads the file*, so your edits always count.
- Bigger changes? `jmux graph revise mapsite --note "data is GPX not
  GeoJSON, add a conversion node"` sends the orchestrator back to
  re-plan, and the gate re-opens.
- Skip the gate entirely with `--no-review` at launch.

### Step 3 — execution (unattended)

The scheduler launches ready nodes up to the concurrency cap. Each node
is a full goal workspace (master + sub-agent mirrors) in its **own git
worktree** on branch `graph/mapsite/<node>`, so parallel nodes can't
collide. Completed nodes are committed and merged back `--no-ff`;
conflicts never auto-resolve — the node blocks and you get a
notification with the fix path.

```
┌ sidebar ─────────┐ ┌─ graph panel ──────────────── [running] · 3/3 slots ─┐
│ ▾ mapsite        │ │  [Pause] [Stop]                                      │
│   graph: mapsite │ │                                                      │
│   scaffold     ✓ │ │   ( scaffold ✓ )                                     │
│   map-core     ✻ │ │   ( map-core ✻ ) ( data-pipeline ✻ ) ( tests-ci ✻ )  │
│   data-pipeline✻ │ │   ( trail-interaction ○ )   ← dimmed, waiting        │
│   tests-ci     ✻ │ │   ( polish-a11y-mobile ○ )                           │
│   trail-inter… ○ │ └──────────────────────────────────────────────────────┘
└──────────────────┘
```

Watch any node by clicking its workspace in the sidebar — master agent
on the left, its sub-agents mirroring beside it, live state sprites in
the sidebar rows.

### Step 4 — click a node: status, editing, and per-loop monitoring

Clicking a chip opens its **detail card** under the DAG (click again to
close):

```
┌─ Graph ─────────────────────────────────────── [running] · 3/3 slots ─┐
│   ( scaffold ✓ )                                                      │
│   ([map-core ✻]) ( data-pipeline ✻ ) ( tests-ci ✻ )     ← selected    │
│   ( trail-interaction ○ ) ( polish-a11y-mobile ○ )                    │
│  ┌─ map-core — Render the interactive map ──────────────── ✻ Running ┐│
│  │ runner: opus-high · deps: scaffold · iteration 2/4                ││
│  │ driver: running · nudges 1                                        ││
│  │ [Open workspace]                                                  ││
│  │ Iterations                                                        ││
│  │   iteration-1.md — blocked                              [Open]    ││
│  │   iteration-2.md — done                                 [Open]    ││
│  │ Goal (edits apply to future launches/iterations)                  ││
│  │ ┌───────────────────────────────────────────────────────────────┐ ││
│  │ │ Render a pan/zoom map with trail layers.                      │ ││
│  │ │ Use MapLibre GL — we need vector tiles later.█                │ ││
│  │ └───────────────────────────────────────────────────────────────┘ ││
│  │                                                    [Save goal]    ││
│  └───────────────────────────────────────────────────────────────────┘│
└───────────────────────────────────────────────────────────────────────┘
```

- **Status**: live node state, driver state (idle/nudges), block detail.
- **Monitor each loop**: every `iteration-N.md` is listed with its
  status; **Open** puts it in the notes editor beside the panel.
- **Edit each loop**: editing **section 4 (Feedback)** of the latest
  iteration file *is* the steering wheel — it seeds the next iteration.
  The **goal editor** at the bottom rewrites the node's goal for future
  launches (`Save goal` persists to `graph.json`).
- **Open workspace** jumps to the node's live agent.
- With `--review-iterations` (Gate 2), a paused node shows
  **[Continue] [Accept]** here: Continue runs the next loop seeded with
  your edited feedback; Accept merges and unblocks dependents.

### Step 5 — completion

When the last node merges, the graph flips to **complete** and you get
one final notification. You're left with the built project plus a full
audit trail:

```
~/src/mapsite/
├── goal.md  src/  data/  tests/  README.md
└── docs/roadmap/mapsite/
    ├── proposal.json  proposal.md      (the reviewed plan)
    ├── graph.json     graph.md        (authoritative state + rendering)
    ├── scaffold/iteration-1.md
    ├── map-core/iteration-{1,2}.md
    └── …one directory per node
```

---

## 3. CLI reference

```
jmux goal <goal.md> [--wait --cwd DIR --runner NAME --agent/--model/--effort
                     --max-iterations N --full-auto|--supervised --title T]
jmux goal status [WORKSPACE]        # all runs, or one
jmux goal complete [--status done|blocked]   # agent fast-path (file wins)
jmux goal continue <WORKSPACE>      # reviewer: another loop (edit §4 first)
jmux goal accept   <WORKSPACE>      # reviewer: final — merge + unblock

jmux graph <name> --goal <top.md> [--max-concurrency K --max-iterations N
                                   --no-review --review-iterations
                                   --runner NAME --full-auto|--supervised]
jmux graph status [name]            # DAG + node states (friendly rendering)
jmux graph approve <name>           # Gate 1 — re-reads proposal.json
jmux graph revise  <name> --note "…"
jmux graph pause|resume|stop <name> # resume also relaunches interrupted
                                    # nodes and retries pending merges
```

Every CLI verb has a UI equivalent in the graph panel — both drive the
same state machine, so mixing them is safe.

## 4. Runners — who executes each node

A **runner** = agent CLI + model + effort. Configure in the jmux
settings file under `goal`:

```jsonc
"goal": {
  "default_runner": "",
  "runners": {
    "opus-high": { "agent": "claude", "model": "opus",  "effort": "high" },
    "fable":     { "agent": "claude", "model": "fable" },
    "codex-sol": { "agent": "custom", "model": "gpt-sol",
                   "command_template": "codex exec --model {model} {seed_file}",
                   "state_detection": "none" }
  },
  "idle_ticks_before_nudge": 5,   // 2 s ticks of idle before a nudge
  "nudge_budget": 3,
  "wall_clock_minutes": 120
}
```

- `claude` runners get full screen-state detection, nudging, and
  sub-agent mirroring.
- `custom` runners (any CLI) are launched via `command_template`
  (placeholders `{sid} {model} {effort} {seed} {seed_file}`) and are
  tracked purely by iteration-file polling — which works, because files
  are the completion signal.
- The orchestrator is told your runner names and assigns them per node
  (`"runner": "opus-high"` in the proposal); override at Gate 1 by
  editing the proposal, later via the node's detail card.

## 5. Troubleshooting

| symptom | cause / fix |
|---|---|
| "goal … has not started" notification | Terminals only spawn once visible. Open the node's workspace (or keep the jmux window visible when launching graphs). |
| Node paused, "selection menu on screen" | First-run trust prompt or a permission menu — open the workspace and answer it once; the driver never auto-answers menus. |
| Node blocked "merge conflict" | Resolve in the base checkout, then `jmux graph resume <name>`. |
| Node blocked "merge-pending: base checkout is dirty" | Commit/stash your own changes in the repo, then `jmux graph resume <name>`. |
| Graph paused after app restart | By design — running nodes were interrupted; `jmux graph resume <name>` relaunches them. |
| Orchestrator never proposes | Check its workspace; `jmux graph revise <name> --note "write the proposal now"` re-prompts it. |

Guidance templates are user-overridable at
`~/.config/jmux/goal-guidance.md` (the per-iteration contract).
