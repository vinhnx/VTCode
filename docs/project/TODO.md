NOTE: use private relay signup codex free

---

idea: wrap a 'vtcode update' cli command to replace curl brew cargo install

---

NOTE: use deepwiki mcp to reference from codex https://deepwiki.com/openai/codex

---

Perform a comprehensive review and optimization of the vtcode agent harness, prioritizing execution speed, computational efficiency, context and token economy. Refactor the tool call architecture to minimize overhead and latency, while implementing robust error handling strategies to significantly reduce the agent's error rate and ensure reliable, effective performance.

---

Conduct a thorough, end-to-end performance audit and systematic optimization of the vtcode agent harness framework with explicit focus on maximizing execution velocity, achieving superior computational efficiency, and implementing aggressive token and context conservation strategies throughout all operational layers. Execute comprehensive refactoring of the tool invocation and agent communication architecture to eliminate redundant processing, minimize inter-process communication latency, and optimize resource utilization at every stage. Design and implement multilayered error handling protocols including predictive failure detection, graceful degradation mechanisms, automatic recovery procedures, and comprehensive logging to drive error occurrence to near-zero levels. Deliver measurable improvements in reliability, throughput, and operational stability while preserving all existing functionality and maintaining backward compatibility with current integration points.

---

extract and open source more components from vtcode-core

---

Review the unified_exec implementation and vtcode's tool ecosystem to identify token efficiency gaps. Analyze which components waste tokens through redundancy, verbosity, or inefficient patterns, and which are already optimized. Develop optimizations for inefficient tools and propose new tools that consolidate multiple operations into single calls to reduce token consumption in recurring workflows.

Specifically examine these known issues: command payloads for non-diff unified_exec still contain duplicated text (output and stdout fields), which wastes tokens across all command-like tool calls. Address this by ensuring unified_exec normalizes all tool calls to eliminate redundant information.

Identify and address these additional token waste patterns: remove duplicated spool guidance that reaches the model both through spool_hint fields and separate system prompts; trim repeated or unused metadata from model-facing tool payloads such as redundant spool_hint fields, spooled_bytes data, duplicate id==session_id entries, and null working_directory values; shorten high-frequency follow-up prompts for PTY and spool-chunk read operations, and implement compact structured continuation arguments for chunked spool reads.

Review each tool's prompt and response structure to ensure conciseness while maintaining effectiveness, eliminating unnecessary verbosity that increases token usage without adding functional value.

---

Perform a comprehensive analysis of the codebase to identify and eliminate all instances of duplicated code, following the DRY (Don't Repeat Yourself) and KISS (Keep It Simple, Stupid) principles. Conduct a systematic search across all modules, classes, and files to find similar code patterns, duplicate logic, redundant implementations, and opportunities for abstraction. Specifically examine rendering-related code such as diff previews and command output previews to determine if they can share unified rendering logic, styling, and common components. Audit all utility functions scattered throughout different modules and extract them into a centralized shared utility module with proper organization and documentation. Create a detailed report identifying each duplication found, the proposed refactoring strategy, and the expected benefits in terms of maintainability, reduced code complexity, and improved consistency. Ensure all refactored code maintains existing functionality while simplifying the overall architecture. Prioritize changes that provide the greatest reduction in duplication with minimal risk to existing functionality.

---

review any duplicated code in the codebase and refactor to remove duplication. For example, the logic for rendering the diff preview and the command output preview can be unified to use the same rendering logic and styling. This will make the codebase cleaner and easier to maintain. Additionally, any common utility functions that are duplicated across different modules can be extracted into a shared utility module. search across modules for similar code patterns and identify opportunities for refactoring to reduce duplication and improve code reuse.

DRY and KISS

---

CODEX plus

main account
kiweuro
writedownapp
humidapp
vtchat.io

--

build subagent (research deepwiki codex, pi-mono)

---

# Error Handling Enhancement in Agent Loop

Comprehensive audit of 15+ files found VT Code already has mature error infrastructure — `ErrorCategory` (16 variants), [RetryPolicy](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/retry.rs#13-21) (typed classification with backoff/jitter/retry-after), [CircuitBreaker](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/tools/circuit_breaker.rs#124-130) (per-tool with exponential backoff), [ErrorRecoveryManager](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/core/error_recovery.rs#83-88), and middleware layers. However, **7 concrete gaps** exist where this infrastructure is not fully utilized.

The theme: **unify all error handling paths through the canonical `ErrorCategory`/[RetryPolicy](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/retry.rs#13-21) system** instead of ad-hoc string matching or hardcoded retry delays.

## Proposed Changes

### Agent Runner — Tool Execution

#### [MODIFY] [tool_access.rs](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/core/agent/runner/tool_access.rs)

**Gap 1 — Replace hardcoded retry with [RetryPolicy](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/retry.rs#13-21)**: [execute_tool_internal](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/core/agent/runner/tool_access.rs#86-170) uses hardcoded `RETRY_DELAYS_MS: [u64; 3] = [200, 400, 800]` and a minimal [should_retry_tool_error](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/core/agent/runner/tool_access.rs#172-178) that only checks `Timeout | NetworkError`. Replace with:

```diff
-const RETRY_DELAYS_MS: [u64; 3] = [200, 400, 800];
-for (attempt, delay_ms) in RETRY_DELAYS_MS.iter().enumerate() {
+let policy = RetryPolicy::from_retries(2, Duration::from_millis(200), Duration::from_millis(2000), 2.0);
+for attempt in 0..policy.max_attempts {
     match registry.execute_public_tool_ref(resolved_tool_name, args).await {
         Ok(result) => return Ok(result),
         Err(e) => {
-            let should_retry = should_retry_tool_error(&e);
+            let decision = policy.decision_for_anyhow(&e, attempt, Some(resolved_tool_name));
             last_error = Some(e);
-            if should_retry && attempt < RETRY_DELAYS_MS.len().saturating_sub(1) {
-                tokio::time::sleep(Duration::from_millis(*delay_ms)).await;
+            if decision.retryable {
+                if let Some(delay) = decision.delay {
+                    tokio::time::sleep(delay).await;
+                }
                 continue;
             }
             break;
```

**Gap 2 — Circuit breaker pre-check**: Add circuit breaker check before tool execution to fail fast when a tool's circuit is open:

```rust
// Check circuit breaker before executing
if let Some(cb) = registry.circuit_breaker() {
    if !cb.allow_request_for_tool(resolved_tool_name) {
        let remaining = cb.remaining_backoff(resolved_tool_name)
            .map(|d| format!(" (retry in {}s)", d.as_secs()))
            .unwrap_or_default();
        return Err(anyhow!("Tool '{}' temporarily disabled by circuit breaker{}", resolved_tool_name, remaining));
    }
}
```

**Gap 6 — Remove stale [should_retry_tool_error](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/core/agent/runner/tool_access.rs#172-178)**: Delete the standalone function since `RetryPolicy::decision_for_anyhow` subsumes it completely (it handles all `ErrorCategory` variants including `RateLimit`, `ServiceUnavailable`, `CircuitOpen` — categories the old function missed).

---

#### [MODIFY] [tool_exec.rs](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/core/agent/runner/tool_exec.rs)

**Gap 3 — Replace ad-hoc string matching with `ErrorCategory`**: Both [execute_sequential_tool_calls](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/core/agent/runner/tool_exec.rs#333-586) and [execute_parallel_tool_calls](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/core/agent/runner/tool_exec.rs#131-332) use raw string checks like `err_lower.contains("rate limit")` and `err_lower.contains("denied by policy")`. Replace with the canonical classifier:

```diff
-let err_lower = err_msg.to_lowercase();
-if err_lower.contains("rate limit") {
+let category = vtcode_commons::classify_error_message(&err_msg);
+if matches!(category, ErrorCategory::RateLimit) {
     runtime.state.warnings.push(
         "Tool was rate limited; halting further tool calls this turn.".into(),
     );
     ...
-} else if err_lower.contains("denied by policy")
-    || err_lower.contains("not permitted while full-auto")
-{
+} else if matches!(category, ErrorCategory::PolicyViolation | ErrorCategory::PlanModeViolation) {
     runtime.state.warnings.push(
         "Tool denied by policy; halting further tool calls this turn.".into(),
     );
```

**Gap 7 — Structured error context**: Enrich tool failure error messages with `ErrorCategory` user label and recovery suggestions:

```rust
let category = vtcode_commons::classify_error_message(&err_msg);
let user_hint = format!("[{}] {}", category.user_label(), err_msg);
// Pass user_hint instead of raw err_msg for user-facing output
```

---

### Async Middleware

#### [MODIFY] [async_middleware.rs](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/tools/async_middleware.rs)

**Gap 4 — Make [AsyncRetryMiddleware](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/tools/async_middleware.rs#387-393) error-category-aware**: Currently retries on any failure regardless of category. Add `ErrorCategory` classification to skip retries for non-retryable errors:

```diff
 let result = next(request.clone()).await;
 if result.success {
     ...
     return result;
 }
+// Skip retry for non-retryable errors
+if let Some(ref error_msg) = result.error {
+    let category = vtcode_commons::classify_error_message(error_msg);
+    if !category.is_retryable() {
+        return result;
+    }
+}
```

---

### Resource Cleanup

#### [NEW] [tool_execution_guard.rs](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/core/agent/runner/tool_execution_guard.rs)

**Gap 5 — `ToolExecutionGuard` for partial operation cleanup**: A RAII-style guard that logs and records cleanup on drop when tool execution is interrupted mid-operation. Records the error in [ErrorRecoveryState](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/core/agent/error_recovery.rs#8-15) for diagnostics.

```rust
pub(super) struct ToolExecutionGuard<'a> {
    tool_name: &'a str,
    tool_call_id: &'a str,
    completed: bool,
}

impl<'a> ToolExecutionGuard<'a> {
    pub fn new(tool_name: &'a str, tool_call_id: &'a str) -> Self {
        Self { tool_name, tool_call_id, completed: false }
    }
    pub fn mark_completed(&mut self) { self.completed = true; }
}

impl Drop for ToolExecutionGuard<'_> {
    fn drop(&mut self) {
        if !self.completed {
            tracing::warn!(
                tool = %self.tool_name,
                tool_call_id = %self.tool_call_id,
                "Tool execution guard dropped without completion — possible resource leak"
            );
        }
    }
}
```

Wire into [execute_sequential_tool_calls](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/core/agent/runner/tool_exec.rs#333-586):

```rust
let mut guard = ToolExecutionGuard::new(&name, &call.id);
match self.execute_tool_internal(&name, &args).await {
    Ok(result) => { guard.mark_completed(); /* ... */ }
    Err(e) => { guard.mark_completed(); /* ... */ }
}
```

---

### Tests

#### [MODIFY] [tool_access tests](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/core/agent/runner/tool_access.rs)

Update [should_retry_tool_error](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/core/agent/runner/tool_access.rs#172-178) tests to validate the new [RetryPolicy](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/retry.rs#13-21)-based retry decision paths. The existing tests will be replaced with tests that verify `RetryPolicy::decision_for_anyhow` behavior for agent runner tool calls.

#### [MODIFY] [error_scenarios_integration.rs](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/tests/error_scenarios_integration.rs)

Add integration tests for:

- Circuit breaker integration in tool execution
- `ErrorCategory`-based halt decisions in sequential tool calls
- `ToolExecutionGuard` drop behavior

## Verification Plan

### Automated Tests

```bash
# Unit tests for modified crates
cargo nextest run -p vtcode-core -- tool_access
cargo nextest run -p vtcode-core -- error_recovery
cargo nextest run -p vtcode-core -- circuit_breaker
cargo nextest run -p vtcode-core -- middleware

# Integration tests
cargo nextest run --test error_scenarios_integration

# Full test suite
cargo nextest run

# Quality gate (clippy + fmt + build + test)
./scripts/check.sh
```

### Manual Verification

No manual testing needed — all changes are covered by existing + new automated tests. The changes are internal refactors that unify code paths through already-tested infrastructure (`ErrorCategory`, [RetryPolicy](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/retry.rs#13-21), [CircuitBreaker](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/tools/circuit_breaker.rs#124-130)).

---

auto-mode

https://code.claude.com/docs/en/permission-modes#eliminate-prompts-with-auto-mode

https://claude.com/blog/auto-mode

https://claude.com/product/claude-code#auto-mode
