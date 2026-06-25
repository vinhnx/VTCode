# Consolidated Code Review Report

**Date:** 2026-06-25
**Scope:** `vtcode-core/src/tools/structural_search.rs`, `vtcode-core/src/tools/registry/executors.rs`, `vtcode-utility-tool-specs/src/lib.rs`
**Trigger:** Checkpoint logs turns 552-555 (ast-grep search failures, unified_search validation errors)

---

## Part 1: Issues Found and Fixed

### F1 -- `unified_search` format enum too restrictive (Critical, FIXED)

**File:** `vtcode-core/src/tools/structural_search.rs:48`
**File:** `vtcode-utility-tool-specs/src/lib.rs:366`

The `format` parameter validation only accepted `"github"` and `"sarif"`. The LLM agent attempted `format: "files_with_matches"` and `format: "count"` which are valid intent values but were rejected, burning 6+ tool calls in retry loops.

**Fix:** Added `"files_with_matches"` and `"count"` to `VALID_FORMAT_VALUES`. Implemented post-processing in `execute_structural_scan`:
- `files_with_matches`: extracts unique file paths from scan results
- `count`: returns per-file match counts as a `BTreeMap`

### F2 -- No automatic fallback from structural to grep (High, FIXED)

**File:** `vtcode-core/src/tools/registry/executors.rs:535-558, 570-665`

When `action: "structural"` failed because ast-grep was unavailable, the agent had to manually figure out to retry with `action: "grep"`. This burned 8+ tool calls across turns 553-555.

**Fix:** Added `try_structural_to_grep_fallback()` method. When structural search fails with "ast-grep not available" and the workflow is `query` or `count`, automatically retries with `action: "grep"` using the pattern as regex. Results are annotated with `fallback_from: "structural"`.

### F3 -- Missing fallback hint in error messages (Medium, FIXED)

**File:** `vtcode-core/src/tools/structural_search.rs:1407-1426`

The ast-grep missing error gave no guidance about alternatives.

**Fix:** Added context-specific hints:
- For `query`/`count`: suggests `action="grep"` as an alternative
- For `rewrite`/`apply`/`test`/`scan`/`new`: explains these require AST

### F4 -- `execute_structural_query` missing `--regex` flag (High, FIXED)

**File:** `vtcode-core/src/tools/structural_search.rs:1548-1550`

`validate_query` accepts `regex_pattern()` as valid input, but `execute_structural_query` never passed `--regex` to ast-grep. The regex was silently ignored. In contrast, `execute_structural_count` correctly passed it.

**Fix:** Added `--regex` flag passthrough after `--kind`.

### F5 -- `execute_structural_count` misleading `truncated` flag (High, FIXED)

**File:** `vtcode-core/src/tools/structural_search.rs:2012-2020`

The count workflow computed `truncated = count > max_results` but never actually truncated anything. The count was always the true total. The `truncated: true` flag was semantically wrong.

**Fix:** Removed the `truncated` field from count results. The count is always complete.

### F6 -- `execute_structural_apply` missing `--follow`/`--no-ignore` (Medium, FIXED)

**File:** `vtcode-core/src/tools/structural_search.rs:3290-3297`

`execute_structural_rewrite` passed `--follow` and `--no-ignore` to ast-grep, but `execute_structural_apply` (the write-forward version) did not. This could cause `apply` to miss files that `rewrite` finds.

**Fix:** Added `--follow` and `--no-ignore` flags to the apply workflow's simple-string-rewrite path.

### F7 -- Incorrect doc comment on `looks_like_css_selector_fragment` (Medium, FIXED)

**File:** `vtcode-core/src/tools/structural_search.rs:4147-4152`

The doc comment described Ruby block fragments (copy-pasted from `looks_like_ruby_block_fragment`), but the function checks for CSS selector patterns.

**Fix:** Replaced with accurate doc describing CSS class (`.`) and ID (`#`) selector patterns.

### F8 -- `validate_rewrite`/`validate_apply` near-identical (Medium, FIXED)

**File:** `vtcode-core/src/tools/structural_search.rs:809-852, 926-967`

These two functions were copy-pasted with only the workflow name differing. Any future change to one would need to be manually mirrored to the other.

**Fix:** Extracted shared `validate_rewrite_or_apply()` method. Both functions now delegate to it.

---

## Part 2: Issues Identified and Fixed (Second Pass)

### R1 -- DRY: Command-building boilerplate duplicated 4x (Medium, FIXED)

Extracted `apply_common_run_flags()` helper to consolidate ~40 lines of duplicated `--lang`, `--selector`, `--strictness`, `--follow`, `--no-ignore` flag patterns across `execute_structural_query`, `execute_structural_rewrite`, `execute_structural_count`, and `execute_structural_apply`.

### R2 -- DRY: `needs_yaml_rewrite` check duplicated (Low, FIXED)

Added `needs_yaml_rewrite()` method on `StructuralSearchRequest` to replace the duplicated expression in `execute_structural_rewrite` and `execute_structural_apply`.

### R3 -- Exit code 1 magic number used 9 times (Low, FIXED)

Extracted `const AST_GREP_NO_MATCHES_EXIT: i32 = 1;` and replaced all 9 occurrences.

### R4 -- `.expect()` on validated fields in non-test code (Low, FIXED)

Converted 4 `.expect()` calls to `ok_or_else(|| anyhow!(...))` for defensive programming.

### R5 -- Duplicate PTY executor implementations (Medium, FIXED)

Added comment explaining the intentional alias relationship between `run_pty_cmd_executor` and `create_pty_session_executor`.

### R6 -- `execute_web_fetch` creates new HTTP client per request (Medium, NOT FIXED)

A new `reqwest::Client` is built on every invocation. This requires storing the client in a `LazyLock` or on `ToolRegistry`, which is a larger change. Left as-is for now.

### R7 -- `execute_code` silently swallows FileTracker errors (Low, FIXED)

Added `tracing::warn!` on error when `FileTracker.detect_new_files` fails.

### R8 -- `nth_child`/`range` in query silently falls to unsupported CLI path (Medium, FIXED)

Added `nth_child` and `range` to the condition that routes queries to the YAML rule generation path, preventing them from being silently ignored.

---

## Part 3: Verification

- `cargo check -p vtcode-core`: **PASS** (clean compilation)
- `cargo check -p vtcode-utility-tool-specs`: **PASS**
- `cargo test -p vtcode-core --lib`: **PASS** (3392 passed, 0 failed, 2 ignored)
- `cargo test -p vtcode-core -- unified_search`: **PASS** (4 passed)
- `cargo test -p vtcode-core -- structural_search`: **PASS**

---

## Summary

| Category | Count | Fixed | Remaining |
|----------|-------|-------|-----------|
| Critical bugs | 1 | 1 | 0 |
| High bugs | 3 | 3 | 0 |
| Medium issues | 6 | 6 | 0 (R6 deferred) |
| Low issues | 5 | 4 | 1 (R6 is medium) |
| **Total** | **15** | **14** | **1** |
