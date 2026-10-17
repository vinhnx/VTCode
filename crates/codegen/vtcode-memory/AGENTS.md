<!-- Compact maintainer rules retain the repository instruction line budget. -->
<!-- markdownlint-disable MD013 -->
# vtcode-memory

[Root AGENTS.md](../../../AGENTS.md) | Per-session event log and derived state.

## Conventions

- `explanation/` projects retained canonical events only; stream snapshots, reduce stable identities, distinguish missing evidence, and validate references by digest. No persisted explanation database. See [execution explanations](../../../docs/development/execution-explanations.md).

- `events.jsonl` is canonical; `derived/` and `index/` are regenerated views — never persist session history elsewhere. `query::write_session_memory_view` is the live producer for `derived/memory.json` (envelope-compatible shape); `session_memory_facts` reads it (batch memory extraction depends on both); `migration.rs` backfills legacy history/trajectory stores, `manifest.rs` owns cap-rewrite intent records. `progress.rs` hosts the `GoalTracker` state machine and compaction-safe `ProgressLedger` view. Append-only: do not mutate historical events; new facts go through `append`. Off the hot path: never read the log back into agent context; use derived queries for revert/compaction/analytics. Public API uses `anyhow::Result<T>` + `.context()`; no `unwrap`/`expect` in non-test code. Keep ordinary appends buffered; flush at turn boundaries, reads, cap rewrites, and close. Return persistence errors to callers; do not silently discard cap-enforcement failures. Retention may remove only validated direct child session directories; preserve live active manifests and reject manifest-controlled paths or symlink entries. `mark_abandoned_active_sessions` flips idle `active` manifests past `max_age_days` to `completed` so crashed threads can be reclaimed (`max_age_days == 0` disables the sweep).
- `event_log.rs`: turn-lifecycle state machine is `LogState::apply_lifecycle_event` (single impl shared by `append` and `scan` via `LifecycleKind`). Serialization+rollback is `LogState::serialize_event`. Cap eviction planning is `LogState::plan_cap_eviction` (I/O stays in `enforce_event_cap`). Do not duplicate these state transitions inline.
- Session event bytes are synced before derived metadata; publish the turn index before the manifest, leave the pending cap-rewrite marker until both are durable, and rescan when metadata is malformed, inconsistent, or offsets exceed the canonical log.
- Session directories are `0700` and session files are `0600`; preserve the symlink-safe `vtcode-commons` filesystem primitives.
- `session.lock` is flock-held for the lifetime of a session's event-log handles (`event_log::acquire_liveness_lock`, best-effort: degrade to unlocked, never fail the open). `retention::session_dir_is_live` treats a held lock as a live session and skips marking/eviction; a missing or acquirable lock file means reclaimable. Never add another session-file deletion path that skips this check.
- `query::search_memory` uses BM25 (`k1=1.2`, `b=0.75`) with deterministic chunk-id ties and only the documented mild timestamp recency multiplier; invalidate the manifest LRU when atomic manifests change.
- `pack.rs` audit packs: SHA-256 manifests of the whole session dir; `audit-pack.json` excludes itself from walks, entry paths are traversal-validated (`SessionStoreError::InvalidPack`), and verification reports post-pack additions as informational `unaccounted`, not failures.
- Cap eviction invokes its summary hook before replacing `events.jsonl`; a failed summary keeps the canonical events intact. Explanation actions distinguish cancelled tool outcomes from errors; cancelled verification still requires a completed exit-zero result.
- `matrix.rs` projects complete canonical `matrix.updated` snapshots. Reserve slots, workspace leases, and named pools together; retain reservations until owned cleanup is confirmed. Persist launch intent before launch and fail closed on ambiguous resume. Cap rewrites preserve the latest matrix checkpoint and adjust retained turn offsets. Final success requires current-generation owned evidence plus a final runtime fingerprint comparison.

## Dependencies

- `vtcode-commons` owns symlink-safe private directories/files and atomic writes; `vtcode-exec-events` owns the `ThreadEvent` / `VersionedThreadEvent` contract, which must not be reinvented.
- `walkdir` handles directory-size and GC walks; `chrono`, `serde`, and `serde_json` handle persistence metadata; `uuid` supports verifier-id generation for the goal tracker.

## Testing

- Use `cargo nextest run -p vtcode-memory` covering ordering, reopen/index reconstruction, retention, and write boundaries; index rebuild reads the versioned envelope and `event.type`, with targeted full-shape validation for lifecycle events so malformed records cannot create phantom turns — broader decoding belongs to turn reconstruction.
