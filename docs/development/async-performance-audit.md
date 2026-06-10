# VT Code Async Performance Audit

Date: 2026-03-04
Scope: Runtime-critical paths first (`vtcode-core`, `vtcode-tools`, `vtcode-bash-runner`)

## Audit Rubric

Each module was reviewed for:

- Blocking calls on async/runtime threads (`std::thread::sleep`, blocking I/O in async paths)
- Awaiting external work while holding locks
- Cancellation and timeout propagation behavior
- Scheduling fairness hazards (`select!`/`join!` use and long critical sections)
- Sync primitives in async-facing hot paths

## Findings (Prioritized)

### Critical

1. Awaiting observer hooks while cache write locks are held
- File: `vtcode-tools/src/cache.rs`
- Impact: lock contention amplification, potential stall chains under load
- Status: fixed in this batch

### High

1. Async-facing notification manager used std sync locks in hot path
- File: `vtcode-core/src/notifications/mod.rs`
- Impact: unnecessary poisoning/recovery branches and slower lock path
- Status: improved in this batch (`parking_lot::{RwLock, Mutex}`)

### Medium (Deferred)

1. Graceful process-group termination uses polling sleeps in synchronous loop
- File: `vtcode-bash-runner/src/process_group.rs`
- Note: currently called from synchronous PTY cleanup paths and `spawn_blocking` paths, so runtime risk is lower than the above critical/high items
- Deferred action: evaluate async-aware termination path only where call sites are async-sensitive

2. Deprecated synchronous retry middleware uses blocking sleep
- File: `vtcode-core/src/tools/middleware.rs`
- Note: type is marked deprecated in favor of `AsyncRetryMiddleware`
- Deferred action: keep behavior stable; avoid churn unless deprecated path is removed or reactivated in runtime-critical paths

## Implemented Batch (Runtime-Critical)

### 1) Cache lock/await safety remediation

Updated `vtcode-tools/src/cache.rs`:

- `insert_arc`:
  - removed `await` while `entries/access_order` locks are held
  - moved observer eviction callback and stats update after lock release
- `remove`:
  - release locks before awaiting `observer.on_evict`
- `prune_expired`:
  - collect/remove expired keys under lock
  - release locks
  - then run observer callbacks

Outcome: no external async callback is awaited while cache write locks are held.

### 2) Notification lock path optimization

Updated `vtcode-core/src/notifications/mod.rs`:

- switched lock types from `std::sync::{RwLock, Mutex}` to `parking_lot::{RwLock, Mutex}`
- removed poisoning recovery branches (not applicable to parking_lot)
- preserved public behavior and API surface

Outcome: lower-overhead lock path and simpler critical sections in notification flow.

### 3) KISS/DRY follow-up pass

Updated `vtcode-tools/src/cache.rs`:

- simplified `insert_arc` lock scope using a single inner block (removed explicit `drop(...)`)
- emit manual-eviction observer events only when an entry was actually removed
- added early-return in `prune_expired` for empty expired set

Updated `vtcode-core/src/notifications/mod.rs`:

- async config methods now delegate to sync methods (`update_config` -> `update_config_sync`, `get_config` -> `get_config_sync`) to remove duplicated lock logic

### 4) Async-safe process termination helper + cache lock scope tightening

Updated `vtcode-bash-runner/src/process_group.rs` and `vtcode-bash-runner/src/lib.rs`:

- added `graceful_kill_process_group_default_async(pid)` that runs graceful kill in `spawn_blocking`
- exported the async helper from crate root
- added async unit test for nonexistent PID behavior

Updated `src/agent/runloop/unified/tool_pipeline/execution_runtime.rs`:

- moved JSON serialization before write-lock acquisition in success-cache path
- keeps lock hold time minimal and avoids extra work inside critical section

### 5) Async runtime safety for PTY session close path

Updated `vtcode-core/src/tools/registry/executors.rs`:

- `execute_close_pty_session` now executes `PtyManager::close_session` in `tokio::task::spawn_blocking`
- prevents synchronous PTY shutdown/wait logic from running on async runtime worker threads
- keeps error propagation and response shape unchanged

### 6) Final KISS/DRY + hot-path cleanup pass

Updated `vtcode-tools/src/cache.rs`:

- `insert_arc` now removes prior key occurrence from LRU order before re-inserting
  - avoids duplicate key entries in access queue and keeps eviction order tight
- `prune_expired` now:
  - uses `entries.retain(...)` to collect and remove expired entries in one pass
  - prunes access-order with a single `retain(...)` using a set of expired keys
  - reduces repeated `retain` scans and simplifies flow

Updated `vtcode-bash-runner/src/process_group.rs`:

- removed duplicate cfg-specific `graceful_kill_process_group_default` wrappers
- kept one unified default wrapper calling cfg-specific `graceful_kill_process_group`

### 7) Async-safe PTY bulk termination in runloop timeout/guard paths

Updated `vtcode-core/src/tools/registry/pty.rs` and `vtcode-core/src/tools/registry/pty_facade.rs`:

- added `PtySessionManager::terminate_all_async()` using `tokio::task::spawn_blocking`
- added `ToolRegistry::terminate_all_pty_sessions_async()` facade method
- preserved existing synchronous methods for compatibility

Updated async call sites:

- `src/agent/runloop/unified/turn/session_loop_runner.rs`
- `src/agent/runloop/unified/turn/tool_outcomes/handlers.rs`

Changes:

- replaced direct `terminate_all_pty_sessions()` calls in async paths with awaited async-safe variant
- added warning logs when blocking-pool join/termination fails
- ensured UI status cleanup still executes in the same flow

### 8) Cancellation-safety cleanup for UI redraw auto-flush task

Updated `src/agent/runloop/unified/turn/utils.rs`:

- added `Drop` for `UIRedrawBatcher` that aborts `auto_flush_task` when batcher is dropped
- prevents leaked background auto-flush task from outliving session/UI lifetime
- keeps implementation minimal (no behavior changes to redraw batching while active)

### 9) Runloop task lifecycle tightening (signal + progress updaters)

Updated `src/agent/runloop/unified/session_setup/signal.rs`:

- replaced raw `JoinHandle<()>` return with RAII `SignalHandlerGuard`
- `SignalHandlerGuard` aborts the background signal-listener task on drop
- keeps existing cancel-token behavior and call-site usage unchanged

Updated `src/agent/runloop/unified/progress.rs`:

- elapsed-time updater now exits once `ProgressState::is_complete()` is true
- avoids unnecessary periodic wakeups after completion even before guard drop/abort

### 10) UI redraw state correctness fix (KISS)

Updated `src/agent/runloop/unified/turn/utils.rs`:

- fixed `UIRedrawBatcher::force_redraw()` to actually reset batching state:
  - set `pending_redraws` to `0` (best-effort `try_lock`)
  - update `last_redraw_time` to `Instant::now()` (best-effort `try_lock`)
- avoids stale pending state after forced redraws

### 11) Background task ownership for file palette indexing

Updated:

- `src/agent/runloop/unified/session_setup/types.rs`
- `src/agent/runloop/unified/session_setup/ui.rs`
- `src/agent/runloop/unified/turn/session_loop_runner.rs`

Changes:

- added `BackgroundTaskGuard` (abort-on-drop) for session-scoped background tasks
- wrapped file-palette indexing `tokio::spawn` in `BackgroundTaskGuard`
- stored guard in `SessionUISetup` and retained it through session loop lifetime
- prevents indexing task from outliving session teardown

### 12) Duplicate MCP initialization spawn guard

Updated `src/agent/runloop/unified/async_mcp_manager.rs`:

- `start_initialization()` now returns early when an existing init task is still running
- avoids spawning duplicate background init tasks and detaching older handles
- added focused unit test: `test_start_initialization_skips_when_task_already_running`

### 13) Detached approval-pattern writes: explicit bounded policy + DRY

Updated `src/agent/runloop/unified/tool_routing.rs`:

- introduced `spawn_approval_record_task(...)` helper for approval-pattern writes
- centralized timeout bound (`APPROVAL_RECORD_TIMEOUT = 500ms`)
- kept these tasks intentionally detached because they are non-critical side effects
- added debug logs for timeout/write errors so detached failures are observable

### 14) Cancellation hardening for PTY stream runtime drop-path

Updated `src/agent/runloop/unified/tool_pipeline/pty_stream.rs`:

- added `Drop` for `PtyStreamRuntime` that:
  - marks stream inactive
  - drops sender
  - aborts background render task if still present
- ensures no background PTY stream task leaks if the execution future is cancelled before explicit `shutdown().await`
- added focused unit test: `pty_stream_runtime_drop_aborts_background_task`

### 15) Cancellation-safe progress callback restoration in tool execution runtime

Updated `src/agent/runloop/unified/tool_pipeline/execution_runtime.rs`:

- added RAII `ProgressCallbackGuard` for temporary PTY progress callback overrides
- guarantees callback restoration on normal return and on future cancellation/drop
- removed manual post-await restoration path in favor of drop-based restoration
- added focused unit test `progress_callback_guard_restores_previous_on_drop`

### 16) Async state-machine bloat patterns (Tweede Golf, May 2026)

Reference: <https://tweedegolf.nl/en/blog/237/async-rust-never-left-the-mvp-state>
Upstream Project Goal: <https://rust-lang.github.io/rust-project-goals/2026/async-statemachine-optimisation.html>

The article identifies four sources of bloat in the futures that `rustc` generates today, all rooted in the MIR `coroutine_resume` lowering:

1. The `Returned` state always panics on re-poll (overhead even when callers are well-behaved).
2. Async blocks with no `.await` still receive a 3-state machine and discriminant switch.
3. Pure-delegation futures (`async fn bar() { foo(blah).await }`) are not inlined; `bar` gets its own state machine that wraps `foo`'s.
4. `match` arms that each end in `.await` produce one duplicated suspend state per arm even when the saved type is identical.

The article's measured wins (2-5% binary size on embedded; ~3% perf on x86 with `smol`) are from compiler-side hacks. Source-level workarounds are explicitly characterized as ugly noise the compiler should obviate. We therefore adopt the following policy rather than mass rewrites:

#### Policy

- Track the upstream Project Goal in this audit; revisit on each `rustup` toolchain bump.
- For NEW code in `vtcode-core` and `vtcode-tools` runtime hot paths:
  - Do not write `async fn` for a body that contains no `.await` unless required by a trait signature; use a plain `fn` instead.
  - For pure single-step delegations (`async fn x(a) { y(a).await }`) on free or inherent functions, prefer `fn x(a) -> impl Future<Output = T> + use<'_, ...>` so the wrapper state machine is elided. Do not apply this to `#[async_trait]` impls, ACP/Codex protocol handlers, or any caller that spawns the future on a multi-thread runtime where `Send` inference would regress.
  - When a `match` chooses between calls of the same async fn that differ only in arguments (article's "Collapsing states" example), hoist the differing argument into a `let` and `.await` once after the match.
- Do not rewrite existing code purely for these patterns. The compiler fix is the right intervention; opportunistic rewrites are acceptable when a file is already being edited for another reason.

#### Scan summary (snapshot)

A scoped scan (`async fn` whose body contains no `.await`, excluding `#[test]`/`#[tokio::test]`/`async_trait` macros) found 214 candidates. The vast majority are trait method implementations whose `async` keyword is mandated by the trait signature and cannot be removed. Genuine pure-delegation candidates (single inner `.await`, free or inherent fn, not a trait impl) cluster in:

- [vtcode-core/src/tools/registry/builder.rs](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/tools/registry/builder.rs) — five `ToolRegistry::new*` constructors all delegate to `Self::build_with_policy(...).await`.
- [vtcode-core/src/tools/registry/pty_facade.rs](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/tools/registry/pty_facade.rs#L43-L49) — `terminate_all_pty_sessions_async`, `terminate_all_exec_sessions_async`.
- [vtcode-core/src/tools/registry/harness_facade.rs](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/tools/registry/harness_facade.rs#L82-L88) — `harness_exec_session_completed`, `terminate_harness_exec_session`.
- [vtcode-core/src/tools/registry/file_helpers.rs](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/tools/registry/file_helpers.rs) — `read_file`, `write_file`, `create_file`, `delete_file` thin wrappers around `execute_tool`.
- [vtcode-core/src/tools/registry/execution_facade.rs](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/tools/registry/execution_facade.rs#L266) — `execute_public_tool_request`, `execute_tool`.
- [vtcode-core/src/tools/cache.rs](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/tools/cache.rs) — `put_file`, `put_directory` arc wrappers.
- [vtcode-core/src/llm/providers/openai/provider/provider_impl.rs](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/llm/providers/openai/provider/provider_impl.rs#L101-L115) — `stream`, `stream_normalized`, `generate` delegations.
- [vtcode-core/src/project_doc.rs](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/project_doc.rs#L112-L125) — `get_user_instructions`, `build_instruction_appendix`.
- [vtcode-core/src/tools/file_ops/path_policy.rs](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/tools/file_ops/path_policy.rs#L267) — `normalize_user_path`.
- [vtcode-core/src/tools/file_ops/write.rs](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/tools/file_ops/write.rs#L48) — `write_file` wrapper around `write_file_internal`.

Pattern 4 ("Collapsing states") yielded one match in `vtcode-core/src/mcp/cli.rs:164`, but each arm calls a *different* function (`run_list`, `run_get`, `run_add`, …) so the saved suspend types differ and there is nothing to collapse. No source change is warranted.

#### Status

- Documented and applied opportunistically in touched runtime files (per policy above), focused on pure-delegation wrappers in `vtcode-core`.
- Re-scan after the next `rust-toolchain.toml` bump to see how many candidates the upstream `coroutine_resume` work has obsoleted.
- If the upstream Project Goal lands an `unwind = abort` / `panic = abort` switch that drops the `Panicked` state, evaluate whether `release-profile-strict` should opt in for binary-size sensitive embeds (tracked here, not actioned).

## Validation

Executed:

- `cargo test -p vtcode-tools cache -- --nocapture`
- `cargo check -p vtcode-tools`
- `cargo test -p vtcode-core notifications::tests:: -- --nocapture`
- `cargo check -p vtcode-core`
- `RUSTC_WRAPPER= cargo test -p vtcode-bash-runner graceful_kill -- --nocapture`
- `RUSTC_WRAPPER= cargo test -p vtcode-tools cache -- --nocapture` (re-run after final cache changes)
- `RUSTC_WRAPPER= cargo test -p vtcode-bash-runner graceful_kill -- --nocapture` (re-run after process-group cleanup)
- `RUSTC_WRAPPER= cargo check -p vtcode-bash-runner`
- `RUSTC_WRAPPER= cargo check -p vtcode`
- `RUSTC_WRAPPER= cargo check -p vtcode-core` (after async PTY bulk termination migration)
- `RUSTC_WRAPPER= cargo check -p vtcode` (after async PTY bulk termination migration)
- `RUSTC_WRAPPER= cargo check -p vtcode` (after redraw batcher cancellation fix)
- `RUSTC_WRAPPER= cargo check -p vtcode` (after signal/progress lifecycle tightening)
- `RUSTC_WRAPPER= cargo test -p vtcode-core pty_test -- --nocapture`
- `RUSTC_WRAPPER= cargo test -p vtcode-core pty_tests -- --nocapture`
- `RUSTC_WRAPPER= cargo test -p vtcode --bin vtcode turn::utils -- --nocapture`
- `RUSTC_WRAPPER= cargo test -p vtcode --bin vtcode progress::tests -- --nocapture`
- `RUSTC_WRAPPER= cargo test -p vtcode --bin vtcode turn::utils -- --nocapture` (re-run after `force_redraw` fix)
- `RUSTC_WRAPPER= cargo test -p vtcode --bin vtcode session_setup -- --nocapture`
- `RUSTC_WRAPPER= cargo test -p vtcode --bin vtcode async_mcp_manager::tests -- --nocapture`
- `RUSTC_WRAPPER= cargo test -p vtcode --bin vtcode async_mcp_manager::tests -- --nocapture` (re-run after duplicate-init guard test)
- `RUSTC_WRAPPER= cargo test -p vtcode --bin vtcode tool_routing -- --nocapture`
- `RUSTC_WRAPPER= cargo test -p vtcode --bin vtcode tool_pipeline -- --nocapture`
- `RUSTC_WRAPPER= cargo test -p vtcode --bin vtcode tool_pipeline::execution_runtime -- --nocapture`
- `RUSTC_WRAPPER= cargo test -p vtcode --bin vtcode tool_pipeline::pty_stream -- --nocapture`
- `rustfmt --check vtcode-core/src/tools/registry/pty.rs vtcode-core/src/tools/registry/pty_facade.rs src/agent/runloop/unified/turn/session_loop_runner.rs src/agent/runloop/unified/turn/tool_outcomes/handlers.rs`
- `rustfmt --check src/agent/runloop/unified/turn/utils.rs`
- `rustfmt --check src/agent/runloop/unified/session_setup/signal.rs src/agent/runloop/unified/progress.rs`
- `rustfmt --check src/agent/runloop/unified/session_setup/types.rs src/agent/runloop/unified/session_setup/ui.rs src/agent/runloop/unified/async_mcp_manager.rs`
- `rustfmt --check src/agent/runloop/unified/tool_routing.rs`
- `rustfmt --check src/agent/runloop/unified/tool_pipeline/pty_stream.rs`
- `rustfmt --check src/agent/runloop/unified/tool_pipeline/execution_runtime.rs`
- `./scripts/perf/baseline.sh baseline`
- `./scripts/perf/baseline.sh latest`
- `./scripts/perf/compare.sh`

Result: all commands completed successfully for the touched areas.

Note on strict clippy:

- `RUSTC_WRAPPER= cargo clippy --workspace --all-targets -- -D warnings` currently fails due pre-existing unrelated lint debt in other crates (`vtcode-ui`, `vtcode-config`, `vtcode-core`, `vtcode` tests)
- touched packages were validated with focused checks/tests and format checks

Performance sample output was written to `.vtcode/perf/diff.md` (single local sample; interpret as directional only).

## Next Batch (Recommended)

1. Cancellation/fairness pass
- prioritize tool pipeline and runloop `select!` sites for cancellation-safety review
- verify long-running work always yields or is delegated to `spawn_blocking`

2. Optional benchmark pass
- run `./scripts/perf/baseline.sh` before/after targeted lock-path changes in cache-heavy flows
