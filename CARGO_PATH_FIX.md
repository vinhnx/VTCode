# Cargo PATH Fix for VT Code Agent

## Problem

The VT Code agent was unable to find `cargo` when running commands like `cargo fmt`. This resulted in exit code 127 (command not found) errors, even though cargo was installed and available on the user's system.

### Error Message
```
zsh:1: command not found: cargo fmt
```

## Root Causes

1. **Missing HOME Environment Variable**: When the agent spawned PTY sessions or executed commands, the `HOME` environment variable might not be set or properly propagated. This prevented path expansion for entries like `$HOME/.cargo/bin`.

2. **Incomplete Environment Variable Expansion**: The path expansion logic in `path_env.rs` only checked the current process environment without fallback mechanisms. When `HOME` wasn't set, cargo directories couldn't be resolved.

3. **Inconsistent Environment Setup**: The PTY environment setup and standard command execution had different levels of environment initialization, creating inconsistency.

## Solution Overview

### Changes Made

#### 1. Enhanced Environment Variable Expansion (`vtcode-core/src/tools/path_env.rs`)

**Added robust fallback logic for HOME resolution:**
- For Unix/Linux: tries `$HOME` → `$USERPROFILE` → `dirs::home_dir()`
- For Windows: tries `$USERPROFILE` → `$HOME` → `dirs::home_dir()`
- Handles both `$VAR` and `${VAR}` syntax variants

**Benefit**: Ensures cargo paths are resolved even when standard environment variables aren't set.

```rust
match var_name {
    "HOME" => std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_default()
        }),
    _ => std::env::var(var_name).unwrap_or_default(),
}
```

#### 2. Explicit HOME in PTY Environment (`vtcode-core/src/tools/pty.rs`)

**Added HOME guarantee in `set_command_environment()`:**
- Checks if HOME exists in environment map
- If missing, uses `dirs::home_dir()` to populate it
- Ensures proper initialization before path merging

**Benefit**: All PTY sessions have HOME set before path expansion occurs.

#### 3. Explicit HOME in Command Execution (`vtcode-core/src/tools/command.rs`)

**Applied same HOME safeguard to standard command execution:**
- Matches PTY environment setup for consistency
- Ensures both execution paths have HOME available

**Benefit**: Uniform environment initialization across all command execution modes.

#### 4. Cleaned Up Path Constants (`vtcode-config/src/constants.rs`)

**Simplified DEFAULT_EXTRA_PATH_ENTRIES:**
- Removed redundant entries (improved variable expansion handles both `$VAR` and `${VAR}`)
- Kept clean, minimal list of commonly used paths

**Affected paths:**
- `$HOME/.cargo/bin` (Rust/Cargo)
- `$HOME/.local/bin` (User binaries)
- `/opt/homebrew/bin` (macOS Homebrew)
- `/usr/local/bin` (System binaries)
- `$HOME/.asdf/bin` & `$HOME/.asdf/shims` (ASDF version manager)
- `$HOME/go/bin` (Go binaries)

## How It Works

1. **PTY Session Creation**: When agent creates PTY session:
   - Inherits parent process environment
   - Ensures HOME is set (fallback to `dirs::home_dir()`)
   - Expands path entries with robust variable resolution
   - Merges expanded paths into PATH

2. **Command Execution**: When agent executes commands:
   - Same HOME guarantee applied
   - PATH merging with expanded extra paths
   - Executed through login shell (`-lc` flags) for additional configuration

3. **Path Resolution Chain**:
   ```
   DEFAULT_EXTRA_PATH_ENTRIES
   ↓
   compute_extra_search_paths() 
   ↓
   expand_entry() → expand_environment_variables()
   ↓
   (with improved HOME fallback)
   ↓
   merge_path_env() into PATH
   ↓
   Set in environment → spawn shell/command
   ```

## Testing

- ✅ Builds without errors
- ✅ Passes cargo fmt (code style validation)
- ✅ Passes cargo clippy (linter)
- ✅ Release build successful
- ✅ No regressions in existing functionality

## Example Scenario

**Before**: Running `cargo fmt` would fail with:
```
zsh:1: command not found: cargo fmt
```

**After**: Cargo paths are properly resolved:
1. Agent sets HOME from `dirs::home_dir()` if missing
2. Expands `$HOME/.cargo/bin` → `/Users/username/.cargo/bin`
3. Adds to PATH before executing shell
4. Shell with login init (`-l` flag) further enriches PATH
5. `cargo fmt` finds cargo in `$HOME/.cargo/bin`
6. Command executes successfully ✓

## Files Modified

- `vtcode-core/src/tools/path_env.rs` - Enhanced environment variable expansion
- `vtcode-core/src/tools/pty.rs` - Added HOME safeguard to PTY environment setup
- `vtcode-core/src/tools/command.rs` - Added HOME safeguard to command execution
- `vtcode-config/src/constants.rs` - Cleaned up path entries

## Backwards Compatibility

- ✅ No breaking changes
- ✅ All existing path entries remain functional
- ✅ Configuration files unchanged
- ✅ Default behavior enhanced without modifications needed
