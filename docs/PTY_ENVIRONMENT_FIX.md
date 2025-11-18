# PTY Command Execution Environment Fix

## Problem

When running commands like `cargo fmt` via the agent, they fail with `command not found` (exit code 127), indicating the command is not in the PATH.

Root causes:
1. **Incomplete shell initialization** - Commands may not source shell startup files properly
2. **Removed overly aggressive cache clearing** - While overkill, the previous approach wasn't the root cause
3. **Sandbox vs non-sandbox PATH handling** - Different code paths for sandboxed vs non-sandboxed execution

## Current Architecture

### Non-Sandboxed Execution (lines 716-727 in pty.rs)
```rust
let shell = resolve_fallback_shell();
let full_command = join(...);
// Spawns: shell -lc "full_command"
```

**The `-lc` flags:**
- `-l` = Login shell (sources ~/.bashrc, ~/.bash_profile, ~/.zshrc, etc.)
- `-c` = Execute command

This SHOULD initialize the environment properly. However:
- The shell initialization happens inside the PTY session
- If it's not a login shell, startup files may not be sourced
- Parent process environment variables may not be inherited

### Sandboxed Execution (lines 702-715)
```rust
// Passes raw command to sandbox runtime
// Sandbox itself handles environment setup
```

## Solution

The `-lc` approach is correct, but we need to ensure:

1. **Always use login shell initialization** - Verify the shell is being invoked as a login shell
2. **Explicit PATH setup if needed** - If login shell initialization fails, have fallback
3. **Better error diagnostics** - Provide feedback when commands fail due to PATH issues
4. **Consistent shell behavior** - Ensure shell startup files are actually being sourced

## Changes Made

- Removed `clear_sandbox_persistent_storage()` call on retry (was overly aggressive)
- Kept the login shell initialization `-lc` approach (this is correct)
- Next: Add explicit PATH validation and diagnostics for missing commands

## Testing

```bash
# Test 1: Verify shell initialization
/bin/zsh -lc 'echo $PATH' 

# Test 2: Verify cargo is in PATH
/bin/zsh -lc 'which cargo'

# Test 3: Run actual command
/bin/zsh -lc 'cargo fmt'
```

## Files Involved

- `vtcode-core/src/tools/pty.rs` - PTY session creation and command execution
- `vtcode-core/src/tools/shell.rs` - Shell resolution logic
- `vtcode-core/src/tools/registry/executors.rs` - Tool execution pipeline
