# Goal & Graph — autonomous agent loops

**Which one?** Fits in one sitting → `jmux goal "…"`. Needs a plan first →
`jmux goal --plan "…"` (or `jmux graph "…"`, the same thing with the graph
flags). Both take the goal as a file *or* as literal text.

`jmux goal` runs one agent against a goal until it's done. `jmux graph`
first has an agent write a **plan** — the goal split into **nodes** (smaller
goals, some depending on others) — you review the plan, then jmux runs the
nodes, in parallel where the plan allows. Each node reports back in an
**iteration report**; who executes it is its **runner**. That's the whole
vocabulary. Design rationale:
[`roadmap/DESIGN-goal-graph.md`](roadmap/DESIGN-goal-graph.md).

Three things ever happen to a run: it's **working**, it **needs you**, or
it's **finished**. Notifications say which, plus why.

```
   goal.md ──► planning agent ──► the plan ──► YOU review ──► jmux runs it
                 (Claude)      (proposal.json) (plan review)      │
                                                        ┌───────────┼───────────┐
                                                        ▼           ▼           ▼
                                                     node A      node B      node C     ← parallel,
                                                    (worktree)  (worktree)  (worktree)    own branches
                                                        │           │           │
                                                 iteration reports + merge back
```

Every node works in its **own git worktree** on its own branch, never in
your checkout, and merges back when it finishes (`--no-worktrees` opts out).

---

## 1. Running one goal

Say what you want, from the repo you want it in:

```sh
cd ~/src/myrepo
jmux goal "Add a --version flag to the CLI, with a test"
```

The argument is a **path if that file exists, otherwise the goal text**. For
inline text jmux writes the goal file itself, under
`~/.local/share/jmux/goal-texts/` — never into your repo. The long-hand still
works, and is the right call for a goal worth keeping and editing:

```sh
echo "Add a --version flag to the CLI, with a test" > ~/src/myrepo/goal.md
jmux goal ~/src/myrepo/goal.md
```

What happens:

1. jmux finds the git root — above `goal.md` for a file, above your current
   directory for inline text (that becomes the working dir).
2. A new workspace `goal: myrepo` opens with a Claude agent already
   working — seeded with your goal plus an operating contract.
3. Its Task-tool sub-agents mirror into read-only panes beside it.
4. jmux watches the pane: if the agent stops early it reminds it to keep
   going (up to 3 times, and only after checking the agent is still
   running); a real permission menu pauses and notifies you instead.
5. When finished, the agent writes its **iteration report**,
   `docs/roadmap/iteration-1.md` (see `goal.output_dir` in §4 to move it):

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

The **file is the source of truth** — `jmux goal report` (the agent-facing
verb, spelled `jmux goal complete` before; both work) is just a fast-path
ping. On `status: blocked` jmux feeds section 4 forward into another
iteration and runs again, up to 3 times (`--max-iterations N`, or the
`goal.max_iterations` setting).

Watch and steer it by **name** — never by workspace UUID. The name is what
the launch printed (`myrepo`, or `<graph>/<node>` for graph nodes), and with
exactly one run going you can leave it off entirely:

```sh
jmux goal status                       # every run + every graph
jmux goal continue --note "use MapLibre, not Leaflet"
jmux goal accept myrepo                # this iteration is final
jmux goal stop myrepo                  # stop driving it; workspace stays open
```

Useful flags:

| flag | effect |
|---|---|
| `--plan` | write a plan first, then run it (delegates to `graph`) |
| `--name NAME` | override the derived run name |
| `--wait` | block until done; prints the iteration file; exit 0/2 |
| `--cwd DIR` | working directory override (default: the git root) |
| `--runner opus-high` | run with a named runner (see §4) |
| `--model fable --effort high` | ad-hoc runner override |
| `--max-iterations 5` | auto-iterate on `blocked` up to 5 loops (default 3) |
| `--supervised` | stock permission prompts (you babysit) |
| `--full-auto` | bypass every permission check (per-invocation opt-in — see §4 Permissions) |

Without either flag the run uses the configured mode (default `acceptEdits`)
plus whatever tools its runner pre-approves — see **Permissions** in §4.

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
jmux graph ~/src/mapsite/goal.md --max-concurrency 3
```

The graph name comes from the repo directory (`mapsite`); `--name` overrides
it. Inline text works here too (`jmux graph "build a trail map site"`, named
from the first few words), as does the older `jmux graph mapsite --goal
~/src/mapsite/goal.md`.

A sidebar group **mapsite** appears containing one workspace:
the planning agent on the left, the **graph panel** on the right.

```
┌─ graph: mapsite ────────────────────────────┬─ Graph ──────────── [planning…] ──┐
│ ✻ Splitting the goal into nodes…            │                                   │
│ ⏺ Read(goal.md)                             │  Waiting for the plan…            │
│ ⏺ Write(docs/roadmap/mapsite/proposal.json) │                                   │
│ ◇ Plan ready — waiting for review.          │                                   │
└─────────────────────────────────────────────┴───────────────────────────────────┘
```

### Step 2 — plan review (Gate 1)

When the plan lands you get a desktop notification and the panel shows it
for review:

```
┌─ Graph ─────────────────────────────────── [plan review] · 0/3 slots ─┐
│  [Open proposal]  [Approve & Run]                                      │
│                                                                        │
│   ( scaffold ○ )                                                       │
│   ( map-core ○ ) ( data-pipeline ○ ) ( tests-ci ○ )                    │
│   ( trail-interaction ○ )                                              │
│   ( polish-a11y-mobile ○ )                                             │
└────────────────────────────────────────────────────────────────────────┘
```

- **Open proposal** opens `proposal.json` (the plan file) in a notes
  editor pane. Edit node goals, deps, runners directly — `Ctrl+S`, then
  **Approve & Run**: approval *re-reads the file*, so your edits always
  count.
- Bigger changes? `jmux graph revise mapsite --note "data is GPX not
  GeoJSON, add a conversion node"` asks for a new plan, and the review
  re-opens.
- Skip the plan review entirely with `--no-review` at launch.

### Step 3 — running the plan (unattended)

jmux launches ready nodes up to the concurrency cap. Each node is a full
goal workspace (agent + sub-agent mirrors) in its **own git worktree** on
branch `graph/mapsite/<node>` — your checkout is never the agent's
workbench, and parallel nodes can't collide. Finished nodes are committed
and merged back `--no-ff`; conflicts never auto-resolve — the node stops
and you get a notification with the fix path. `--no-worktrees` puts every
node in your checkout instead (the older behaviour).

Nodes start **in the background**: a node's agent begins working the moment the
scheduler launches it, whether or not you are looking at its workspace (and even
with the quake drop-down closed). Your selected workspace is never switched for
you — new node workspaces just appear in the sidebar.

```
┌ sidebar ─────────┐ ┌─ graph panel ──────────────── [working] · 3/3 slots ─┐
│ ▾ mapsite        │ │  [Pause] [Stop]                                      │
│   graph: mapsite │ │                                                      │
│   scaffold     ✓ │ │   ( scaffold ✓ )                                     │
│   map-core     ✻ │ │   ( map-core ✻ ) ( data-pipeline ✻ ) ( tests-ci ✻ )  │
│   data-pipeline✻ │ │   ( trail-interaction ○ )   ← dimmed, waiting        │
│   tests-ci     ✻ │ │   ( polish-a11y-mobile ○ )                           │
│   trail-inter… ○ │ └──────────────────────────────────────────────────────┘
└──────────────────┘
```

Watch any node by clicking its workspace in the sidebar — its agent on
the left, its sub-agents mirroring beside it, live state sprites in the
sidebar rows.

### Step 4 — click a node: status, editing, and per-loop monitoring

Clicking a chip opens its **detail card** under the DAG (click again to
close):

```
┌─ Graph ─────────────────────────────────────── [working] · 3/3 slots ─┐
│   ( scaffold ✓ )                                                      │
│   ([map-core ✻]) ( data-pipeline ✻ ) ( tests-ci ✻ )     ← selected    │
│   ( trail-interaction ○ ) ( polish-a11y-mobile ○ )                    │
│  ┌─ map-core — Render the interactive map ──────────────── ✻ working ┐│
│  │ runner: opus-high · deps: scaffold · iteration 2/4                ││
│  │ agent: working · 1 reminders sent                                 ││
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

- **Status**: what the node is doing, what its agent is doing, and why
  it stopped if it did.
- **Monitor each loop**: every `iteration-N.md` is listed with its
  status; **Open** puts it in the notes editor beside the panel.
- **Edit each loop**: editing **section 4 (Feedback)** of the latest
  iteration file *is* the steering wheel — it seeds the next iteration.
  The **goal editor** at the bottom rewrites the node's goal for future
  launches (`Save goal` persists to `graph.json`).
- **Open workspace** jumps to the node's live agent.
- With `--review-iterations` (iteration review, Gate 2), a paused node
  shows **[Continue] [Accept]** here: Continue runs the next loop seeded
  with your edited feedback; Accept merges and unblocks dependents. From
  the shell: `jmux graph continue mapsite map-core --note "…"` /
  `jmux graph accept mapsite map-core` (equivalently
  `jmux goal continue|accept mapsite/map-core`).

### Step 5 — completion

When the last node merges, the graph flips to **finished** and you get
one final notification. You're left with the built project plus a full
audit trail (under `goal.output_dir` — §4 — which defaults to
`docs/roadmap`):

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

`GOAL` is a markdown file if that path exists, otherwise the goal text.
`NAME` is a run name (`<graph>/<node>` for graph nodes; a bare node id works
when it's unambiguous) or a workspace UUID for scripts. Omit `NAME` and the
verb hits this pane's run, else the only active run; ambiguity is an error
listing the candidates.

```
jmux goal <GOAL> [--plan --name N --wait --cwd DIR --runner NAME
                  --agent/--model/--effort --max-iterations N
                  --full-auto|--supervised --title T]
jmux goal status [NAME]             # friendly; no NAME = all runs + all graphs
jmux goal continue [NAME] [--note "…"]   # reviewer: another loop
jmux goal accept   [NAME]           # reviewer: final — merge + unblock
jmux goal stop     [NAME]           # stop driving it (workspace kept)
jmux goal report   [NAME] [--status done|blocked]   # agent fast-path (file wins)

jmux graph <GOAL> [--name NAME --max-concurrency K --max-iterations N
                   --no-review --review-iterations --no-worktrees
                   --runner NAME --full-auto|--supervised]
jmux graph status [name]            # plan + node states (friendly rendering)
jmux graph approve <name>           # plan review — re-reads proposal.json
jmux graph revise  <name> --note "…"
jmux graph pause|resume|stop <name> # resume also relaunches interrupted
                                    # nodes and retries pending merges
jmux graph continue|accept <name> <node> [--note "…"]   # iteration review
```

- `--json` (global) prints the raw JSON-RPC response for any of these.
- `jmux goal complete` still works as a hidden alias of `jmux goal report`,
  and the socket method is still `goal.complete` — running agents were told
  that spelling.
- `jmux goal --plan "…"` and `jmux graph "…"` create the same thing;
  `jmux graph` is the management surface from there on.

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
  "idle_ticks_before_nudge": 5,   // 2 s ticks of idle before a reminder
  "nudge_budget": 3,              // reminders before jmux escalates to you
  "wall_clock_minutes": 120,      // per-run cap, 0 = uncapped
  "max_iterations": 3,            // bare-goal default for --max-iterations
  "output_dir": "docs/roadmap",   // where iteration reports + graph state go
  "permission_mode": "acceptEdits" // default mode when nothing overrides it
}
```

- `claude` runners get full screen-state detection, nudging, and
  sub-agent mirroring.
- `custom` runners (any CLI) are launched via `command_template`
  (placeholders `{sid} {model} {effort} {seed} {seed_file}`) and are
  tracked purely by iteration-file polling — which works, because files
  are the completion signal.
- The planning agent is told your runner names and assigns them per node
  (`"runner": "opus-high"` in the plan); override during plan review by
  editing the plan, later via the node's detail card.

### Permissions — how a run stays unattended

Three modes:

| mode | what the agent may do |
|---|---|
| `supervised` | nothing without asking (stock prompting — you babysit) |
| `acceptEdits` (default) | apply file edits without asking; every command still asks |
| `bypassPermissions` | everything, no prompts |

(claude's other `--permission-mode` values — `plan`, `auto`, `manual`,
`dontAsk` — are accepted and passed straight through.)

An unanswered prompt is not a hang: jmux sees the menu, stops nudging and
escalates ("needs you") so you can answer it. That is the designed behaviour
— jmux never auto-answers a menu.

**Precedence** — first one that applies wins:

1. the per-invocation flag: `--full-auto` (= `bypassPermissions`) or
   `--supervised`
2. the runner's `permission_mode`
3. `goal.permission_mode`
4. `acceptEdits`

A value that isn't a known mode is ignored with a warning in the log and the
next source applies, so a typo never silently changes what an agent may do.

**Pre-approved tools.** `acceptEdits` alone stops at the first `Bash` call.
A runner's `allowed_tools` is a list of Claude Code tool patterns handed to
`claude --allowedTools`, which is what makes the default mode able to run for
hours without you:

```jsonc
"runners": {
  "unattended": {
    "agent": "claude", "model": "opus", "effort": "high",
    "permission_mode": "acceptEdits",
    "allowed_tools": [
      "Read", "Grep", "Glob",
      "Bash(cargo build:*)", "Bash(cargo check:*)", "Bash(cargo test:*)",
      "Bash(cargo fmt:*)", "Bash(cargo clippy:*)",
      "Bash(git status:*)", "Bash(git diff:*)", "Bash(git log:*)",
      "Bash(git add:*)", "Bash(git commit:*)",
      "Bash(ls:*)", "Bash(rg:*)", "Bash(find:*)"
    ]
  }
}
```

Then `jmux graph "…" --runner unattended` runs its read/build/test loop
without stopping to ask. Pattern syntax is Claude Code's own: a bare tool name
(`Read`) allows the whole tool, `Bash(cargo test:*)` allows every command
whose prefix matches. `allowed_tools` applies to `claude` runners only — a
`custom` runner's `command_template` owns its whole command line.

What the allowlist deliberately does **not** cover: anything destructive or
outbound — `rm`, `git push`, `git reset --hard`, package installs, `curl`,
`sudo`, editors, migrations. Those still raise a prompt, and the prompt still
escalates to you. A node that stops on one is the system working, not a
failure: you answer once and `jmux goal continue`.

**Security note.** Node edges feed one agent's output into the next agent's
prompt, so anything an agent reads (a dependency's report, a fetched page, a
file in the repo) can try to steer it. An allowlist bounds that: the worst a
successful injection buys is a command that was already on your safe list.
`--full-auto` removes that bound entirely, which is why jmux keeps it a
per-invocation, human-typed flag: `permission_mode: "bypassPermissions"` in
`goal.permission_mode` **or** in a runner is refused (logged, then the next
source applies). Config is ambient — a runner name can be chosen by the
planning agent itself, and a settings default applies to runs you never
thought about — so bypass never becomes a property of your config.

### Where the audit trail lands

`goal.output_dir` is **repo-relative** — an absolute path or one containing
`..` is rejected and the default is used. It holds the whole audit trail:
`<output_dir>/iteration-N.md` for a bare goal, and
`<output_dir>/<graph>/{proposal.json,graph.json,graph.md,<node>/iteration-N.md}`
for a graph. `.jmux/` is a good choice for repos where `docs/` is real
documentation. A graph stores the directory it was created with, so changing
the setting never moves a graph that is already running.

Don't want the trail committed? Add the directory to `.gitignore`:

```sh
echo "/.jmux/" >> .gitignore
```

In worktree mode the node's agent commits its own work (`git add -A` in the
worktree), so an ignored output directory simply stays out of those commits —
the files are still on disk and jmux still reads them.

## 5. Troubleshooting

| symptom | cause / fix |
|---|---|
| "several goal runs are active — name one" | More than one run is going and you gave no name. `jmux goal status` lists them; pass the name (`mapsite/map-core`, or the bare node id when it's unique). |
| "… failed to start" / "running but its terminal cannot be read" | Should not happen: node agents start without being visible (headless spawn). If you see it, jmux could not create the terminal — open the workspace and check the jmux log for a `spawn_headless` / `ghostty_surface_new` error. |
| "waiting on a prompt" | First-run trust prompt or a permission menu — open the workspace and answer it once; jmux never auto-answers menus. |
| First run on a new repo stops immediately | Expected once. Claude Code asks "do you trust the files in this folder?" the first time it starts in a directory, and each graph node runs in a **new** worktree (`<repo>-worktrees/<node>`), which counts as a new directory. jmux escalates instead of answering it. See "trust prompt", below. |
| Node needs you, "merge conflict" | Resolve it in your checkout, then `jmux graph resume <name>`. |
| Node needs you, "merge waiting: your checkout has uncommitted changes" | Commit or stash your own changes, then `jmux graph resume <name>`. (jmux ignores its own files under `goal.output_dir` when deciding this.) |
| Graph paused after a restart | By design — running nodes were interrupted; `jmux graph resume <name>` relaunches them. |
| No plan ever appears | Check the graph's workspace; `jmux graph revise <name> --note "write the plan now"` re-prompts the agent. |

### The trust prompt on a new worktree

Claude Code trusts a directory only after someone answers its trust dialog
there, and it keys that per project — its own git root for a checkout. A
graph node's worktree is its own git root, so the first node started in a
fresh worktree shows the dialog and jmux escalates it to you ("waiting on a
prompt"). Answer it in the node's workspace; nothing else in the run is
affected.

jmux does **not** pre-answer it. The only supported way to do that
non-interactively is to write `projects["<path>"].hasTrustDialogAccepted` into
`~/.claude.json`, which is Claude Code's own live config file — it holds your
credentials and MCP setup and is rewritten wholesale by every running claude
session under a lock jmux can't join. Trading a one-time keypress for a risk
of corrupting that file is a bad deal, so jmux leaves it alone.

What does work, one time, with no config surgery: **trust the parent
directory**. Claude Code's trust check walks up the directory tree, so a
trusted `~/src` covers `~/src/myrepo` *and* every `~/src/myrepo-worktrees/<node>`
jmux ever creates. Run `claude` once in `~/src`, accept the dialog, and no
worktree asks again — at the price of trusting everything you ever put under
that directory, so pick the level you actually mean.

Guidance templates are user-overridable at
`~/.config/jmux/goal-guidance.md` (the per-iteration contract).
