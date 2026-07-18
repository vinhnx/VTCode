# vtcode-memory

Unified per-session state store: append-only `ThreadEvent` log plus derived
views, retention, and cross-session query. The single source of truth for an
agent session's state, context, and history.

## Conventions

- `events.jsonl` is canonical; `derived/` and `index/` are regenerated views —
  never persist session history anywhere else. `progress` (`ProgressLedger`,
  `derived/progress.json`) is the compaction-safe goal-progress view.
- `progress.rs` also hosts the `GoalTracker` state machine (ported from grok-build)
  with 8 goal states and forward-compat deserialization.
- Append-only: do not mutate historical events; new facts go through `append`.
- Off the hot path: never read the log back into agent context; use derived
  queries (`reconstruct_turn`, `query_facts`) only for revert/compaction/analytics.
- Public API must stay `anyhow::Result<T>` + `.context()`; no `unwrap`/`expect`
  in non-test code (CI uses `-D warnings`).

## Dependencies

- `vtcode-exec-events` — `ThreadEvent` / `VersionedThreadEvent` contract (do not
  reinvent event types here).
- `walkdir` — directory size / GC walks.
- `chrono`, `serde`, `serde_json` — metadata + persistence.
- `uuid` — verifier id generation for goal tracker.
