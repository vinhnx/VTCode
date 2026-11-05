<!-- markdownlint-disable MD013 -->
# vtcode-memory

[Root AGENTS.md](../../../AGENTS.md) | Per-session append-only event log and derived state.

## Modules

- `event_log/` canonical log, lifecycle state machine, cap eviction | `query.rs` derived views, BM25 search, live sessions | `snapshot.rs` read-only tail queries.
- `retention.rs` GC and liveness | `mailbox.rs` cross-process peer mail | `pack.rs` audit packs | `manifest.rs` cap-rewrite intents | `migration.rs` legacy backfill.
- `progress/` GoalTracker + ProgressLedger | `explanation/` projections over canonical events | `matrix.rs` matrix snapshot projection.

## Rules

- `events.jsonl` is canonical and `derived/`/`index/` are regenerated views, so never persist session history elsewhere.
- The log is append-only and off the hot path: never mutate historical events (add facts only through `append`) and never read it back into agent context.
- Sync event bytes before derived metadata, publish the turn index before the manifest, and keep the cap-rewrite marker until both are durable.
- Return persistence and cap-enforcement errors to callers instead of discarding them, and keep canonical events intact when the eviction summary hook fails.
- Never add a session-file deletion path that skips the `session.lock` liveness check, and retention may remove only validated direct child session directories (no manifest-controlled paths or symlinks), with `0700` directories and `0600` files via the symlink-safe `vtcode-commons` primitives.
- `vtcode-exec-events` owns the `ThreadEvent`/`VersionedThreadEvent` contract, which must not be reinvented here.
- Lifecycle transitions live only in `LogState::apply_lifecycle_event`, `serialize_event`, and `plan_cap_eviction`, so do not duplicate them inline.

## Gotchas

- Shared `open` degrades to unlocked, but `open_exclusive` fails closed with `SessionBusy`.
- Use `snapshot::read_event_tail` for external inspection and never open a writer to read archives.
- Matrix resume fails closed when ambiguous, and evicting old turns must not reset consumed planning-recovery requests.

## Testing

`cargo nextest run -p vtcode-memory` must cover ordering, reopen/index reconstruction, retention, and write boundaries.

Details: [maintainer notes](../../../docs/development/maintainer-notes/vtcode-memory.md) — read the section for the subsystem you change before editing it.
