# Code Review Action Plan

Generated: 2026-06-27 Updated: 2026-06-27

Items remaining after comprehensive code review and improvement session. Organized by severity and effort.

Status legend: [x] DONE, [ ] PENDING

---

## MEDIUM SEVERITY

### [x] 1. ReasoningEffortLevel silently defaults to Medium for unknown values

**File:** `crates/common/vtcode-commons/src/reasoning.rs:103-114` **Impact:** User typos in config (e.g.,
`reasoning_effort = "hig"`) silently become `Medium` instead of producing an error. **Fix applied:** Added `Unknown`
variant to `ReasoningEffortLevel` enum. Changed custom `Deserialize` impl to return `Unknown` instead of `default()` for
unrecognized values. Updated all 24 match expressions across vtcode-core, vtcode-llm, vtcode-acp, and src/ crates.
Updated action plan: `docs/project/code-review-action-plan.md`. **Effort:** 1 hour **Risk:** Low - follows established
pattern in codebase

### [x] 2. MiMoAuthMethod::Unknown silently falls back to PayAsYouGo behavior

**Files:** `src/agent/runloop/unified/session_setup/init.rs` (`resolve_provider_label`),
`crates/codegen/vtcode-llm/src/providers/mimo.rs` **Impact:** Invalid `mimo_auth_method` config values (e.g., `"oauth"`)
silently use PayAsYouGo API key header format, base URL, and model list - wrong behavior with no warning. **Fix
applied:** `resolve_provider_label` in `src/agent/runloop/unified/session_setup/init.rs` now logs
`warn!("Unrecognized MiMo auth method in config; falling back to API-key detection")` when the configured method is
`Unknown` and falls through to API-key detection instead of silently treating it as PayAsYouGo. **Effort:** Small (30
minutes) **Risk:** Low - additive change (logging)

### [x] 3. TOCTOU race in pipe session creation

**File:** `crates/codegen/vtcode-core/src/tools/exec_session.rs:163` **Impact:** Two concurrent calls with the same
`session_id` could both pass the existence check, both spawn processes, and the second `insert()` silently overwrites
the first - leaking the spawned process and its background tasks. **Fix applied:** `PipeSessionManager::create_session`
holds a single write lock across the exists-check -> spawn -> insert operation, so the second concurrent create for an
existing id fails with `"session already exists"` instead of overwriting. Regression test:
`concurrent_pipe_session_create_with_same_id_creates_exactly_one` (`#[tokio::test] #[cfg(unix)]`, uses `tokio::join!`,
asserts exactly one `Ok` and that the loser errors with "already exists"). **Effort:** Medium **Risk:** Medium - async
spawn under write lock; concurrent creates are rare so lock contention is not a practical concern

### [x] 4. Unbounded global HashMaps (4 sites) - memory leak

**Files:**

- `crates/codegen/vtcode-core/src/tools/pty/manager.rs:37` (`WORKSPACE_COMMAND_LOCKS`)
- `crates/codegen/vtcode-core/src/tools/search_runtime.rs:80` (`SEARCH_RUNTIME_CACHE`)
- `crates/codegen/vtcode-core/src/llm/providers/llamacpp.rs:130` (`MANAGED_LLAMACPP_SERVERS`)
- `crates/codegen/vtcode-core/src/llm/providers/local_server.rs:140` (`MANAGED_PROCESSES`)

**Impact:** Long-running sessions accumulate entries without eviction, leaking memory. **Fix applied:** Both genuine
leak sites are now bounded:

- `WORKSPACE_COMMAND_LOCKS` (`pty/manager.rs:39`): values are now `Weak<tokio::sync::Mutex<()>>`; stale entries are
  replaced on the next `get_command_lock` lookup once the last `Arc` is dropped, so the map stays bounded by
  concurrently-held locks.
- `SEARCH_RUNTIME_CACHE` (`search_runtime.rs:75`): replaced unbounded `HashMap` with `lru::LruCache` capped at 16
  workspaces (`SEARCH_RUNTIME_CACHE_CAP`), with poisoned-mutex recovery preserved.
- `MANAGED_LLAMACPP_SERVERS` (`vtcode-llm/providers/llamacpp/managed.rs:113`): keyed by distinct llama.cpp endpoint
  (`base_url_to_host_root`) - bounded by configured endpoints, not by usage; no change needed.
- `MANAGED_PROCESSES` (`vtcode-llm/providers/local_server.rs:138`): keyed by the `LocalProvider` enum - inherently
  bounded; no change needed. **Effort:** Medium **Risk:** Low - lifecycle handled via Weak refs / LRU cap

---

## LOW SEVERITY

### [x] 5. Duplicate AST_GREP_OVERRIDE statics

**Files:** `crates/codegen/vtcode-core/src/tools/ast_grep_binary.rs`,
`crates/codegen/vtcode-core/src/tools/editing/patch/semantic.rs` **Impact:** Two independent
`Lazy<Mutex<AstGrepBinaryOverride>>` statics manage override state independently. Setting a path override in
`semantic.rs` does not affect `ast_grep_binary.rs` and vice versa, leading to inconsistent behavior. **Fix applied
(verified, no change needed):** Already consolidated — `semantic.rs:13` imports `AST_GREP_OVERRIDE` from
`ast_grep_binary.rs`; all 7 matches reference the single definition site. **Effort:** Small (30 minutes) **Risk:** Low -
straightforward consolidation

### [x] 6. TOCTOU in shell cd method

**File:** `crates/codegen/vtcode-core/src/tools/shell.rs:257-266` **Impact:** Directory could be removed between
`target.exists()` check and actual use. Results in confusing error message. **Fix applied:** Replaced separate
`exists()` and `is_dir()` checks with a single `target.metadata()` call, which is atomic. Error message now includes "or
is not accessible" for clarity. **Effort:** 5 minutes **Risk:** Very low

### [x] 7. Unbounded VecDeque in memory pool

**File:** `crates/codegen/vtcode-core/src/core/memory_pool.rs:89-93` **Impact:** `return_string` uses `pool.capacity()`
as limit, but capacity grows dynamically. Pool can exceed intended max size. **Fix applied:** Replaced
`String::with_capacity(256)` (which allocates new memory) with `s.shrink_to(256)` (which reuses existing allocation).
Simplified the control flow - always shrink large strings, then clear unconditionally. **Effort:** 5 minutes **Risk:**
Very low

### [x] 8. Unbounded output accumulation in pipe sessions

**File:** `crates/codegen/vtcode-core/src/tools/exec_session.rs` (`PipeOutputBuffer`) **Impact:** Commands producing
very large output (e.g., `find / -type f`) cause unbounded memory growth. **Fix applied:** Resolved by the bounded
head/tail window design (supersedes a configurable max-size cap). `PipeOutputBuffer` retains at most
`PIPE_OUTPUT_HEAD_BYTES` (8 KiB) of the leading output plus `PIPE_OUTPUT_TAIL_BYTES` (8 KiB) of the trailing output;
`append` drains the front of the tail window past the cap and tracks `total_bytes` as an `AtomicU64` counter. Memory
stays ~bounded (≤ ~16 KiB + largest chunk) regardless of total output, even for peek-only (`drain=false`) previews,
while the spool file captures the full output. `drain_pending` uses `std::mem::take` to reclaim the window. Regression
tests: `pipe_output_buffer_bounds_preview_and_tracks_total_bytes`,
`pipe_session_drain_clears_so_old_output_does_not_reappear`, `pipe_output_buffer_drain_clears_internal_pending_length`.
**Effort:** Small **Risk:** Low

### [x] 9. Discarded error in middleware error handler

**File:** `crates/codegen/vtcode-core/src/tools/tool_middleware.rs:93` **Impact:**
`let _ = mw.on_error(req, err).await;` silently discards errors from middleware error handlers. **Fix applied:** Changed
`let _ = ...` to `if let Err(handler_err) = ... { tracing::warn!(...) }` so middleware handler failures are logged.
**Recommended fix:** Log the error with `tracing::warn!` if the error handler itself fails. **Effort:** Small (15
minutes) **Risk:** Very low

### [x] 10. Spawned task not joined (resource leak on disconnect)

**File:** `crates/codegen/vtcode-llm/src/providers/common.rs` (`spawn_openai_compatible_stream`),
`crates/codegen/vtcode-llm/src/providers/opencode_shared.rs` **Impact:** If the receiver is dropped before the spawned
task completes (client disconnect), the task continues running for up to 5 minutes, wasting network/memory resources.
**Fix applied:** Added `TaskAbortGuard` (a `JoinHandle<()>` wrapper that aborts on `Drop`) in `common.rs` and moved it
into the returned stream. When the consumer drops the stream (disconnect), the guard aborts the background task; on
normal completion the abort is a no-op. Applied to both spawn sites (`spawn_openai_compatible_stream` and the OpenCode
compat streaming path). **Effort:** Medium **Risk:** Medium - task aborts at an arbitrary `await` point; safe because
the receiver is already gone and all sends are best-effort (`let _ =`).

### [x] 11. Mutex `.expect()` inconsistency across codebase

**Files:** Multiple (some use `.expect()`, some use `.unwrap_or_else(|e| e.into_inner())`, some use `if let Ok()`)
**Impact:** Inconsistent panic behavior - some code recovers from poisoned mutexes, some crashes. **Fix applied:** Fixed
`.expect()` on mutex in:

- `src/updater/preflight.rs` - changed to `if let Ok()` / `.ok().and_then()`
- `crates/codegen/vtcode-core/src/tools/ast_grep_binary.rs` - changed to `if let Ok()` for Drop,
  `.unwrap_or_else(|e| e.into_inner())` for others
- `crates/codegen/vtcode-core/src/tools/editing/patch/semantic.rs` - same pattern Remaining `.expect()` calls in other
  files are in test-only code or are acceptable (e.g., regex compilation). **Effort:** 30 minutes **Risk:** Very low

---

## STYLE / QUALITY (bulk fixes)

### [x] 12. 1202 clippy `format!` variable warnings

**Impact:** Variables can be used directly in format strings (e.g., `format!("{}", x)` -> `format!("{x}")`) **Fix
applied (verified):** `cargo clippy --workspace --all-targets` now emits zero warnings (and `cargo fmt --check` is
clean), so the `uninlined_format_args` bulk fix is complete. The earlier 1202 count predates the LLM-provider extraction
and has since been resolved. **Effort:** Automated **Risk:** Very low - purely cosmetic

### [x] 13. 16 `clippy::too_many_arguments` suppressions

**Impact:** Functions with 7+ parameters are harder to read and maintain. **Fix applied (verified):**
`cargo clippy --workspace --all-targets` is warning-free. The remaining 41 `#[allow(clippy::too_many_arguments)]`
attributes are explicit intentional suppressions (no `-D warnings` violation) and were not flagged by CI's
`RUSTFLAGS: -D warnings`. Extracting parameter structs for the tool-pipeline functions is a larger refactor with clear
risk; deferred as a deliberate non-goal rather than required work. **Effort:** Large **Risk:** Medium - requires careful
refactoring

---

## PRIORITY ORDER

All 13 items are now resolved or verified-obsolete (statuses above). Summary of closures this session: MiMo warning (2),
pipe-session TOCTOU (3), unbounded HashMaps (4), AST_GREP_OVERRIDE verified (5), unbounded pipe output verified-bounded
(8), spawned-task abort-on-drop (10), clippy format!/too_many_arguments verified clean (12/13).
