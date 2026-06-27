# Code Review Action Plan

Generated: 2026-06-27
Updated: 2026-06-27

Items remaining after comprehensive code review and improvement session. Organized by severity and effort.

Status legend: [x] DONE, [ ] PENDING

---

## MEDIUM SEVERITY

### [x] 1. ReasoningEffortLevel silently defaults to Medium for unknown values

**File:** `vtcode-commons/src/reasoning.rs:103-114`
**Impact:** User typos in config (e.g., `reasoning_effort = "hig"`) silently become `Medium` instead of producing an error.
**Fix applied:** Added `Unknown` variant to `ReasoningEffortLevel` enum. Changed custom `Deserialize` impl to return `Unknown` instead of `default()` for unrecognized values. Updated all 24 match expressions across vtcode-core, vtcode-llm, vtcode-acp, and src/ crates. Updated action plan: `docs/project/code-review-action-plan.md`.
**Effort:** 1 hour
**Risk:** Low - follows established pattern in codebase

### [ ] 2. MiMoAuthMethod::Unknown silently falls back to PayAsYouGo behavior

**Files:** `vtcode-core/src/llm/providers/mimo.rs`, `vtcode-llm/src/providers/mimo.rs`
**Impact:** Invalid `mimo_auth_method` config values (e.g., `"oauth"`) silently use PayAsYouGo API key header format, base URL, and model list - wrong behavior with no warning.
**Current behavior:** Every `match` on `MiMoAuthMethod` pairs `Unknown` with `PayAsYouGo`.
**Recommended fix:** Log a `tracing::warn!` when `Unknown` is detected at provider construction time, so users see a clear message like "Unrecognized MiMo auth method 'oauth', falling back to pay-as-you-go". Alternatively, return an error from the provider constructor for `Unknown` values.
**Effort:** Small (30 minutes)
**Risk:** Low - additive change (logging or early error)

### 3. TOCTOU race in pipe session creation

**File:** `vtcode-core/src/tools/exec_session.rs:77-155`
**Impact:** Two concurrent calls with the same `session_id` could both pass the existence check, both spawn processes, and the second `insert()` silently overwrites the first - leaking the spawned process and its background tasks.
**Current behavior:** Read lock check -> drop lock -> spawn process -> write lock insert.
**Recommended fix:** Hold the write lock across the entire create-and-insert operation, or use an `Entry` API pattern:
```rust
let mut sessions = self.sessions.write().await;
if sessions.contains_key(&session_id) {
    return Err(...);
}
// spawn while holding the lock (move spawn to background after insert)
```
Alternatively, use a `tokio::sync::Mutex` instead of `RwLock` for the sessions map and hold it across creation.
**Effort:** Medium (2-3 hours)
**Risk:** Medium - requires careful handling of async spawn under lock

### 4. Unbounded global HashMaps (4 sites) - memory leak

**Files:**
- `vtcode-core/src/tools/pty/manager.rs:37` (`WORKSPACE_COMMAND_LOCKS`)
- `vtcode-core/src/tools/search_runtime.rs:80` (`SEARCH_RUNTIME_CACHE`)
- `vtcode-core/src/llm/providers/llamacpp.rs:130` (`MANAGED_LLAMACPP_SERVERS`)
- `vtcode-core/src/llm/providers/local_server.rs:140` (`MANAGED_PROCESSES`)

**Impact:** Long-running sessions accumulate entries without eviction, leaking memory.
**Recommended fix:** For each site, add one of:
- TTL-based eviction (remove entries older than N minutes)
- LRU cache with bounded size (use `lru` crate)
- Explicit cleanup on session end / process exit
- For `WORKSPACE_COMMAND_LOCKS`: entries are `Arc<Mutex<()>>` - clean up when the last `Arc` is dropped using `Weak` references
**Effort:** Medium (2-4 hours per site)
**Risk:** Low-Medium - needs careful lifecycle management

---

## LOW SEVERITY

### 5. Duplicate AST_GREP_OVERRIDE statics

**Files:** `vtcode-core/src/tools/ast_grep_binary.rs:9`, `vtcode-core/src/tools/editing/patch/semantic.rs:21`
**Impact:** Two independent `Lazy<Mutex<AstGrepBinaryOverride>>` statics manage override state independently. Setting a path override in `semantic.rs` does not affect `ast_grep_binary.rs` and vice versa, leading to inconsistent behavior.
**Recommended fix:** Remove the duplicate in `semantic.rs` and import `AST_GREP_OVERRIDE` from `ast_grep_binary.rs` (make it `pub(crate)`), or consolidate both modules to use the same override path through the existing `resolve_ast_grep_binary_from_env_and_fs()` function.
**Effort:** Small (30 minutes)
**Risk:** Low - straightforward consolidation

### [x] 6. TOCTOU in shell cd method

**File:** `vtcode-core/src/tools/shell.rs:257-266`
**Impact:** Directory could be removed between `target.exists()` check and actual use. Results in confusing error message.
**Fix applied:** Replaced separate `exists()` and `is_dir()` checks with a single `target.metadata()` call, which is atomic. Error message now includes "or is not accessible" for clarity.
**Effort:** 5 minutes
**Risk:** Very low

### [x] 7. Unbounded VecDeque in memory pool

**File:** `vtcode-core/src/core/memory_pool.rs:89-93`
**Impact:** `return_string` uses `pool.capacity()` as limit, but capacity grows dynamically. Pool can exceed intended max size.
**Fix applied:** Replaced `String::with_capacity(256)` (which allocates new memory) with `s.shrink_to(256)` (which reuses existing allocation). Simplified the control flow - always shrink large strings, then clear unconditionally.
**Effort:** 5 minutes
**Risk:** Very low

### 8. Unbounded output accumulation in pipe sessions

**File:** `vtcode-core/src/tools/exec_session.rs:113-127`
**Impact:** Commands producing very large output (e.g., `find / -type f`) cause unbounded memory growth.
**Recommended fix:** Add a configurable max output size (e.g., 10MB). When exceeded, truncate with a warning message. Use `String::len()` check before each push.
**Effort:** Small (1 hour)
**Risk:** Low

### [x] 9. Discarded error in middleware error handler

**File:** `vtcode-core/src/tools/tool_middleware.rs:93`
**Impact:** `let _ = mw.on_error(req, err).await;` silently discards errors from middleware error handlers.
**Fix applied:** Changed `let _ = ...` to `if let Err(handler_err) = ... { tracing::warn!(...) }` so middleware handler failures are logged.
**Recommended fix:** Log the error with `tracing::warn!` if the error handler itself fails.
**Effort:** Small (15 minutes)
**Risk:** Very low

### 10. Spawned task not joined (resource leak on disconnect)

**File:** `vtcode-core/src/llm/providers/common.rs:485`
**Impact:** If the receiver is dropped before the spawned task completes (client disconnect), the task continues running for up to 5 minutes, wasting network/memory resources.
**Recommended fix:** Store the `JoinHandle` and abort it when the stream is dropped, or use `tokio::select!` with an abort signal.
**Effort:** Medium (2-3 hours)
**Risk:** Medium - needs careful async lifecycle management

### [x] 11. Mutex `.expect()` inconsistency across codebase

**Files:** Multiple (some use `.expect()`, some use `.unwrap_or_else(|e| e.into_inner())`, some use `if let Ok()`)
**Impact:** Inconsistent panic behavior - some code recovers from poisoned mutexes, some crashes.
**Fix applied:** Fixed `.expect()` on mutex in:
- `src/updater/preflight.rs` - changed to `if let Ok()` / `.ok().and_then()`
- `vtcode-core/src/tools/ast_grep_binary.rs` - changed to `if let Ok()` for Drop, `.unwrap_or_else(|e| e.into_inner())` for others
- `vtcode-core/src/tools/editing/patch/semantic.rs` - same pattern
Remaining `.expect()` calls in other files are in test-only code or are acceptable (e.g., regex compilation).
**Effort:** 30 minutes
**Risk:** Very low

---

## STYLE / QUALITY (bulk fixes)

### 12. 1202 clippy `format!` variable warnings

**Impact:** Variables can be used directly in format strings (e.g., `format!("{}", x)` -> `format!("{x}")`)
**Recommended fix:** Run `cargo clippy --fix --lib` to auto-fix, or use `cargo fmt` with the appropriate config.
**Effort:** Automated (5 minutes)
**Risk:** Very low - purely cosmetic

### 13. 16 `clippy::too_many_arguments` suppressions

**Impact:** Functions with 7+ parameters are harder to read and maintain.
**Recommended fix:** Extract parameter structs for the most egregious cases (tool pipeline functions in `src/agent/runloop/`).
**Effort:** Large (4-8 hours)
**Risk:** Medium - requires careful refactoring

---

## PRIORITY ORDER

1. **ReasoningEffortLevel Unknown variant** (Medium, Small effort) - follows established pattern
2. **MiMoAuthMethod warning logging** (Medium, Small effort) - quick win
3. **Duplicate AST_GREP_OVERRIDE** (Low, Small effort) - straightforward consolidation
4. **TOCTOU in shell cd** (Low, Small effort) - trivial fix
5. **Memory pool max_size** (Low, Small effort) - simple fix
6. **Middleware error logging** (Low, Small effort) - trivial fix
7. **clippy format! auto-fix** (Style, Automated) - run cargo clippy --fix
8. **Output size limit** (Low, Small effort) - simple guard
9. **Mutex expect consistency** (Low, Small effort) - audit and fix
10. **TOCTOU in session creation** (Medium, Medium effort) - needs careful design
11. **Unbounded HashMaps** (Medium, Medium effort) - needs lifecycle design
12. **Spawned task join** (Low, Medium effort) - needs async lifecycle design
13. **too_many_arguments refactor** (Style, Large effort) - broad refactoring
