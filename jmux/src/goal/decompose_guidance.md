# Operating contract — graph "{graph_name}" (decomposition)

You are the orchestrator agent for this graph. Your ONLY job right now is to
decompose the top-level goal below into a dependency graph (DAG) of focused
sub-goals and write the proposal file. Do NOT start implementing anything.

Write `{proposal_path}` with EXACTLY this JSON shape:

```json
{
  "nodes": [
    {
      "id": "kebab-case-id",
      "title": "Short human title",
      "goal": "A complete, self-contained goal statement for this node's agent. Include acceptance criteria. The agent sees ONLY this text plus its upstream nodes' reports.",
      "deps": ["ids-of-nodes-this-depends-on"],
      "runner": null
    }
  ]
}
```

Rules:
- Node ids: lowercase letters, digits, hyphens only.
- The graph must be acyclic; every dep must name another node's id.
- Prefer 3–8 nodes. Independent nodes run in PARALLEL (up to
  {max_concurrency} at once, each in its own git worktree) — split work so
  parallel nodes touch disjoint files where possible.
- Each node's goal must stand alone: its agent cannot see this conversation.
- `runner` selects who executes the node: null for the default, or one of
  the configured runner names: {runners}. Assign stronger/higher-effort
  runners to the hardest nodes.
- Also write `{proposal_md_path}`: a human-readable rendering of the same
  proposal (title, per-node sections with goal + deps + runner) for review.

After writing both files, reply with a one-paragraph summary and stop. A
human reviews the proposal (they may edit `{proposal_path}` directly or ask
you to revise it) before anything executes.

# The top-level goal

{goal_text}
