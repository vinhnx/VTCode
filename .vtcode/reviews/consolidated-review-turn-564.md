# Consolidated Code Review Report

**Date**: 2026-06-25
**Session**: Continuation from turn 555 review + critical runtime fixes

---

## Executive Summary

This session addressed **5 critical/high-severity issues** and **4 medium-severity improvements** across the vtcode runtime and structural search subsystem. All fixes pass the full test suite (3392 + 1752 tests, 0 failures).

---

## Critical Fixes (Applied)

### R1: Loop detector leaking `loop_detected` flag to model
- **File**: `src/agent/runloop/unified/turn/tool_outcomes/response_content.rs:110-114`
- **Severity**: Critical
- **Problem**: The `compact_model_tool_payload` function included `loop_detected: true` in model output. This internal control logic flag was visible to the LLM, causing it to see stale/reused results with confusing metadata and get stuck in infinite retry loops.
- **Fix**: Changed the skip condition so `loop_detected` is always stripped from model output. Related metadata keys (`reused_recent_result`, `loop_detected_note`, `next_action`) are gated on `loop_detected` being true elsewhere and still appear correctly.
- **Tests updated**: 2 tests updated to expect `loop_detected` absent from compacted output.

### R2: Shell policy blocking compound commands
- **File**: `vtcode-core/src/tools/registry/shell_policy.rs`
- **Severity**: Critical
- **Problem**: The shell policy checker validated the entire compound command string against deny patterns. When an agent ran `rm -f /tmp/file && sg run ...`, the `rm *` glob matched the `rm` sub-command and blocked the entire command, even though `sg run` was safe.
- **Fix**: Added `split_compound_command()` that splits commands on `&&`, `||`, and `;` operators, then validates each sub-command independently. A denied sub-command still blocks the whole command, but safe sub-commands are no longer blocked by a denied peer.

### R3: Blocking I/O in async context (structural_search.rs)
- **File**: `vtcode-core/src/tools/structural_search.rs`
- **Severity**: High
- **Problem**: Two blocking filesystem calls in async context:
  1. `std::fs::canonicalize` in `build_resolved_workspace_path` (called from async functions)
  2. `std::fs::read_to_string` in `extract_language_injections` (all other `extract_*` functions use async variant)
- **Fix**:
  1. Made `build_resolved_workspace_path` async with `tokio::fs::canonicalize`. Extracted `build_resolved_workspace_path_inner` for callers that already have a canonicalized path (avoids redundant canonicalize in `resolve_config_path`).
  2. Changed `extract_language_injections` to use `afs::read_to_string`.

### R4: Redundant canonicalization in resolve_config_path
- **File**: `vtcode-core/src/tools/structural_search.rs:4377-4411`
- **Severity**: Medium
- **Problem**: `resolve_config_path` canonicalized workspace root with `tokio::fs::canonicalize`, then passed it to `build_resolved_workspace_path` which canonicalized again.
- **Fix**: `resolve_config_path` now calls `build_resolved_workspace_path_inner` (no canonicalize) since it already has the canonicalized path.

---

## Medium Fixes (Applied)

### R5: DRY violation - extract_rule_dirs / extract_util_dirs
- **File**: `vtcode-core/src/tools/structural_search.rs`
- **Severity**: Medium
- **Problem**: `extract_rule_dirs` and `extract_util_dirs` were structurally identical functions differing only in the YAML key name.
- **Fix**: Extracted `extract_string_list_from_yaml(config_path, key)` helper. Both functions now delegate to it.

### R6: Grep fallback discards multiple globs
- **File**: `vtcode-core/src/tools/registry/executors.rs:635-640`
- **Severity**: Medium
- **Problem**: When falling back from structural search to grep, only the first glob pattern was forwarded. Multiple globs (e.g. `["*.rs", "*.toml"]`) were silently dropped.
- **Fix**: When multiple globs are simple extension patterns, combine them using ripgrep's brace expansion syntax (`*.{rs,toml}`). Falls back to first glob for mixed patterns.

### R7: Silent error swallowing in grep fallback
- **File**: `vtcode-core/src/tools/registry/executors.rs:680-683`
- **Severity**: Low
- **Problem**: `try_structural_to_grep_fallback` used `.ok()?` which silently discarded all errors, making debugging difficult.
- **Fix**: Replaced with explicit match arms that log errors at `tracing::debug` level before returning `None`.

---

## Deferred Findings (Not Fixed - Lower Priority)

| # | Severity | Finding | Reason Deferred |
|---|----------|---------|-----------------|
| D1 | Medium | Repeated temp-dir + YAML-rule boilerplate in 4 execute_* functions | Requires larger refactor; low regression risk |
| D2 | Medium | No overlap validation for byte offsets in execute_structural_apply | Edge case; ast-grep findings rarely overlap |
| D3 | Medium | Silent fallback to "javascript" when lang is missing | Already guarded by validation in most workflows |
| D4 | Medium | effective_max_results silently clamps 0 to 1 | Semantic mismatch but low user impact |
| D5 | Medium | Fragile error-message-based fallback detection | Acknowledged in comments; low change frequency |
| D6 | Low | DRY violation in normalize_match / normalize_rewrite_match | Would require trait or closure refactor |
| D7 | Low | normalized_globs re-computed on every call | Performance optimization, not correctness |
| D8 | Low | yaml_escape_scalar doesn't handle newlines | Patterns unlikely to contain newlines |
| D9 | Low | Incomplete language-to-type mapping in grep fallback | Coverage improvement, not bug fix |

---

## Test Results

```
vtcode-core:  3392 passed, 0 failed, 2 ignored
vtcode:       1752 passed, 0 failed, 0 ignored
Total:        5144 passed, 0 failed
```

The pre-existing flaky test `evicts_least_recently_used` passed in this run (race condition under parallel load, not related to any changes).

---

## Files Changed

| File | Changes |
|------|---------|
| `src/agent/runloop/unified/turn/tool_outcomes/response_content.rs` | Strip `loop_detected` from model output |
| `vtcode-core/src/tools/registry/shell_policy.rs` | Split compound commands for policy validation |
| `vtcode-core/src/tools/structural_search.rs` | Async I/O fixes, DRY refactoring, redundant canonicalize removal |
| `vtcode-core/src/tools/registry/executors.rs` | Multiple glob forwarding, fallback error logging |
| `src/agent/runloop/unified/turn/tool_outcomes/execution_result/tests.rs` | Updated test expectations |
| `src/agent/runloop/unified/turn/tool_outcomes/handlers/tests/fallbacks.rs` | Updated test expectations |
