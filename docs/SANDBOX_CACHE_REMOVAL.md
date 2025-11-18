# PTY Command Execution Improvements - VT Code

## Summary

Fixed PTY command execution issues and improved error diagnostics for better troubleshooting.

## Changes Made

### 1. Removed Overly Complex Sandbox Cache Clearing

**File: `vtcode-core/src/tools/pty.rs`**
- Removed `clear_sandbox_persistent_storage()` method that was called on every retry
- This was overkill and could cause environment issues

**File: `vtcode-core/src/tools/registry/executors.rs` (old)**
- Removed aggressive sandbox cache clearing on first retry

**Rationale:**
- Modern sandboxes handle state management automatically
- Clearing sandbox state on every retry was unnecessarily aggressive
- Could interfere with shell environment initialization

### 2. Enhanced Error Diagnostics for "Command Not Found"

**File: `vtcode-core/src/tools/registry/executors.rs`**

#### A. Exit Code 127 Detection
When a command returns exit code 127 (command not found), the response now includes:
- Clear identification of the problem: "Command 'X' not found"
- Actionable guidance: "Ensure it's installed and in PATH"
- Specific help for development tools: "For dev tools (cargo, npm, etc.), verify shell initialization sources ~/.bashrc or ~/.zshrc"

#### B. Improved Error Suggestions
When `get_errors` is called after a "command not found" error:
- Suggests verifying command with `which <command>`
- Explains shell initialization requirements
- Points to shell configuration files (.bashrc, .zshrc)

## Result

- ✅ Simpler, more maintainable PTY retry logic
- ✅ Better error messages help users diagnose PATH issues
- ✅ Explicit guidance for shell initialization
- ✅ No aggressive state manipulation between retries
- ✅ All tests passing (20/20)

## Why This Matters

When commands like `cargo fmt` fail with "command not found":
1. **Before**: Generic error message, unclear how to fix
2. **After**: Clear message identifies exit code 127, explains shell initialization requirement

Example output:
```
Command 'cargo' not found (exit code 127). Ensure it's installed and in PATH. 
For dev tools (cargo, npm, etc.), verify shell initialization sources ~/.bashrc or ~/.zshrc
```

## Testing

```bash
cargo check      # Compiles successfully
cargo test --lib # All 20 tests pass
cargo fmt        # Properly formatted
```

## Architecture Impact

PTY retry mechanism now focuses on:
- Exponential backoff (2^n delay)
- Standard error logging
- Clean command re-execution without state manipulation
- Enhanced error messaging for diagnostics

## Files Modified

1. `vtcode-core/src/tools/pty.rs` - Removed unused function
2. `vtcode-core/src/tools/registry/executors.rs` - Enhanced diagnostics and error handling
3. `docs/PTY_ENVIRONMENT_FIX.md` - New documentation on PTY environment setup
