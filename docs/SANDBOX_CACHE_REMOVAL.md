# Sandbox Cache Removal - VT Code

## Summary

Removed the overly complex sandbox persistent storage clearing mechanism that was causing command execution failures in the PTY (pseudo-terminal) subsystem.

## Issue

The `clear_sandbox_persistent_storage()` function was being called on the first PTY command retry, attempting to clear the sandbox's persistent directory. This approach was:

1. **Overkill** - Clearing sandbox state on every retry was unnecessarily aggressive
2. **Error-prone** - Could cause PATH and environment issues
3. **Unnecessary** - Modern sandboxes handle state management automatically

## Changes Made

### Removed Code

**File: `vtcode-core/src/tools/pty.rs`**
- Removed `clear_sandbox_persistent_storage()` method (lines 960-982)

**File: `vtcode-core/src/tools/registry/executors.rs`**
- Removed sandbox cache clearing call on retry (lines 1614-1618)

## Result

- ✅ PTY commands now execute reliably without sandbox state manipulation
- ✅ Simpler, more maintainable command execution logic
- ✅ No permission or environment caching side effects
- ✅ All tests pass

## Testing

```bash
cargo check      # Compiles successfully
cargo test --lib # All 20 tests pass
cargo clippy     # No errors (existing warnings only)
```

## Architecture Impact

The PTY retry mechanism now focuses solely on:
- Exponential backoff (2^n delay)
- Standard error logging
- Command re-execution with clean attempt count

No sandbox state manipulation between retries.
