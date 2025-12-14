# PTY Command Execution Stability Fixes

## Date: November 17, 2025

## Summary

Fixed critical stability issues in the PTY (pseudo-terminal) session management system that caused unreliable command execution and resource leaks.

## Issues Identified

### 1. Session Reference Counting Mismatch
**Problem**: The PTY session management used a simple atomic counter that was incremented in `start_session()` and manually decremented in `end_session()`. If an error occurred during tool execution, the `end_session()` call might be skipped, leading to:
- Permanent session count increment
- Eventually hitting the max_sessions limit
- New commands unable to execute
- No error indication to the user

**Root Cause**: No automatic cleanup mechanism. The counter was manually managed across multiple code paths with no guarantees of balance.

### 2. Missing `which` Command Support
**Problem**: The `which` command was not included in the list of recognized development toolchain commands, making it unavailable to agents.

## Solutions Implemented

### 1. RAII Session Guard Pattern
Introduced `PtySessionGuard` - an RAII (Resource Acquisition Is Initialization) guard that automatically decrements the session count when dropped, even if errors occur.

**Changes**:
- **File**: `vtcode-core/src/tools/registry/pty.rs`
- **Changes**:
  - Added `PtySessionGuard` struct with `Drop` trait implementation
  - Modified `start_session()` to return `Result<PtySessionGuard>` instead of `Result<()>`
  - The guard holds a reference to the atomic session counter
  - Automatically decrements counter in `Drop::drop()` regardless of how the function exits

**Benefits**:
- Guarantees session count is always decremented
- Works even if the tool panics or returns early
- No manual cleanup needed
- Prevents resource leaks

### 2. Automatic Cleanup in Tool Execution
Integrated the guard into the main tool execution path to ensure automatic cleanup.

**File**: `vtcode-core/src/tools/registry/mod.rs`
- Modified `execute_tool()` to use the RAII guard
- PTY session is automatically cleaned up when the guard goes out of scope
- Works seamlessly with error paths and cancellations

### 3. Added `which` Command Support
**File**: `vtcode-core/src/tools/pty.rs`
- Added `"which"` to the `is_development_toolchain_command()` function
- Now recognized as a standard utility command
- Can be invoked via terminal and PTY tools

## Code Changes

### PtySessionGuard Implementation
```rust
#[derive(Debug)]
pub struct PtySessionGuard {
    active_sessions: Arc<AtomicUsize>,
}

impl Drop for PtySessionGuard {
    fn drop(&mut self) {
        let current = self.active_sessions.load(Ordering::SeqCst);
        if current > 0 {
            self.active_sessions.fetch_sub(1, Ordering::SeqCst);
        }
    }
}
```

### Updated Session Management
```rust
pub fn start_session(&self) -> Result<PtySessionGuard> {
    if !self.can_start_session() {
        return Err(anyhow!("Maximum PTY sessions exceeded..."));
    }

    self.active_sessions.fetch_add(1, Ordering::SeqCst);
    Ok(PtySessionGuard {
        active_sessions: Arc::clone(&self.active_sessions),
    })
}
```

## Testing

Created comprehensive test suite: `vtcode-core/tests/pty_session_guard_test.rs`

### Tests Added
1. **auto_cleanup**: Verifies guard automatically decrements count
2. **multiple_sessions**: Tests multiple concurrent guards
3. **max_sessions**: Validates session limit enforcement

All tests pass  

## Impact

### Before Fix
- Unstable command execution
- Session count could leak permanently
- Eventually hitting max_sessions and blocking new commands
- No clear error message when limit reached
- Risk of resource exhaustion

### After Fix
- Guaranteed session count cleanup
- Stable, reliable command execution
- No possibility of counter leaks
- Clear error messages when limits are reached
- Proper resource management

## Files Modified

1. **vtcode-core/src/tools/registry/pty.rs**
   - Added `PtySessionGuard` with RAII semantics
   - Updated `start_session()` return type
   - Made `PtySessionManager` public for testing

2. **vtcode-core/src/tools/registry/mod.rs**
   - Updated `execute_tool()` to use RAII guard
   - Removed manual `end_pty_session()` calls
   - Exported `PtySessionGuard` and `PtySessionManager`

3. **vtcode-core/src/tools/pty.rs**
   - Added `"which"` to `is_development_toolchain_command()`

4. **vtcode-core/tests/pty_session_guard_test.rs** (new)
   - Comprehensive unit tests for guard functionality

5. **vtcode-core/tests/pty_tests.rs**
   - Fixed test to include `max_tokens` field

## Backward Compatibility

  Fully backward compatible. The change is internal:
- `end_pty_session()` still exists for direct PTY session tools
- Public API remains unchanged
- Only affects internal session tracking reliability

## Verification

-   `cargo check` passes
-   `cargo clippy` passes (no new warnings)
-   All new tests pass
-   Existing PTY tests pass
-   `cargo fmt` applied

## Future Improvements

1. Consider adding metrics for session tracking
2. Add logging for session lifecycle events
3. Implement session timeout cleanup
4. Add per-session resource limits
