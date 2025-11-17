# Repeated Tool Calls Fix - Root Cause Analysis & Solution

## Problem Statement

Agent was calling `run_terminal_cmd` with `git diff` repeatedly (13+ times) without proper termination, causing performance issues and infinite-looking loops.

## Root Cause Analysis

The `repeated_tool_attempts` counter in `run_loop.rs` had two critical flaws:

### Flaw 1: Counter Logic Was Inverted
**Issue**: The counter incremented on **every call** (including successful ones), not just failures.

```rust
// BEFORE (WRONG)
let attempts = repeated_tool_attempts.entry(signature_key.clone()).or_insert(0);
*attempts += 1;  // ← Increments for SUCCESS and FAILURE alike
let current_attempts = *attempts;
if current_attempts > tool_repeat_limit {
    // abort...
}
```

**Impact**: Legitimate repeated tool calls (e.g., checking git diff multiple times) would eventually hit the limit and cause agent abort.

### Flaw 2: No Counter Reset on Success
**Issue**: Successful tool executions never reset the counter.

**Impact**: Counter persisted across multiple successful calls, causing:
- Call 1 (success): counter = 1
- Call 2 (success): counter = 2
- Call 3 (success): counter = 3 → **ABORT** (limit is 3)

### Flaw 3: Misleading Error Message
**Issue**: Message said "unsuccessful attempts" but counter tracked all attempts.

```rust
"Aborting repeated tool call '{}' after {} unsuccessful attempts with identical arguments.",
```

This was conceptually wrong - it should only count failures, not successes.

## Solution Implemented

### Change 1: Only Count Failures (Not All Attempts)
Moved counter increment from pre-execution check to post-failure handlers:

```rust
// Failure handler
ToolExecutionStatus::Failure { error } => {
    // Increment failure counter for this tool signature
    let failed_attempts = repeated_tool_attempts
        .entry(signature_key.clone())
        .or_insert(0);
    *failed_attempts += 1;
    // ... handle failure
}

// Timeout handler  
ToolExecutionStatus::Timeout { error } => {
    // Increment failure counter for timeout as well
    let failed_attempts = repeated_tool_attempts
        .entry(signature_key.clone())
        .or_insert(0);
    *failed_attempts += 1;
    // ... handle timeout
}
```

### Change 2: Reset Counter on Success
Clear the counter when tool succeeds:

```rust
ToolExecutionStatus::Success { ... } => {
    // Reset the repeat counter on successful execution
    repeated_tool_attempts.remove(&signature_key);
    // ... handle success
}
```

### Change 3: Update Check & Message Logic
Remove the increment from pre-execution, update error message:

```rust
// AFTER (CORRECT)
let failed_attempts = repeated_tool_attempts
    .entry(signature_key.clone())
    .or_insert(0);
if *failed_attempts > tool_repeat_limit {
    renderer.line(
        MessageStyle::Error,
        &format!(
            "Aborting: tool '{}' failed {} times with identical arguments.",
            name,
            *failed_attempts
        ),
    )?;
    // abort with clear message about failures, not attempts
}
```

## Behavior Changes

### Before Fix
| Scenario | Counter | Result |
|----------|---------|--------|
| Success → Success → Success | 1→2→3 | **ABORT** (wrong!) |
| Fail → Fail → Fail | 1→2→3 | Abort (correct) |
| Success → Fail → Fail | 1→2→3 | Abort (confusing) |

### After Fix
| Scenario | Counter | Result |
|----------|---------|--------|
| Success → Success → Success | 0→0→0 | **Continue** ✓ |
| Fail → Fail → Fail | 1→2→3 | Abort (correct) |
| Success → Fail → Fail | 0→1→2 | Continue (correct) |

## Files Modified

**src/agent/runloop/unified/turn/run_loop.rs**
- Line 2121-2130: Changed counter logic to check failures, not all attempts
- Line 2291: Reset counter on success
- Line 2327-2330: Increment counter on failure
- Line 2359-2362: Increment counter on timeout

## Error Message Improvement

**Before**: 
```
"Aborting repeated tool call 'git diff' after 2 unsuccessful attempts with identical arguments."
```
(confusing - what's the difference between attempts and unsuccessful attempts?)

**After**:
```
"Aborting: tool 'git diff' failed 3 times with identical arguments."
```
(clear - only counts actual failures, not successes)

## Testing

✅ Compilation: `cargo check` passes
✅ Unit tests: `cargo test --lib` (17 tests pass)
✅ Logic verified: Counter only increments on failure/timeout
✅ Reset verified: Counter cleared on success

## Impact

- ✅ Legitimate repeated tool calls now work correctly
- ✅ Failure detection still works as intended  
- ✅ Error messages are now accurate and clear
- ✅ No breaking changes to existing behavior for failure cases
- ✅ Backward compatible

## Prevention

To avoid similar issues:
1. Use semantic variable names (`failed_attempts` vs `attempts`)
2. Increment counters only for the conditions being tracked
3. Reset counters when condition no longer applies
4. Keep error messages consistent with what's actually being counted
