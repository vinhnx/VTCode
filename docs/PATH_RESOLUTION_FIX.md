# PATH Resolution Fix for VT Code Agent

## Problem

When running terminal commands through the VT Code agent, programs located in user-specific directories (like `~/.cargo/bin/`) were not being found, resulting in "command not found" errors.

### Example
```
$ vtcode exec "cargo fmt"
Error: zsh:1: command not found: cargo fmt
```

This occurred because:
1. The agent was trying to execute commands directly without using the shell
2. `std::env::vars()` was inheriting the parent process's PATH, but this didn't include user-installed tools
3. The command resolution was looking for the program in standard system locations only

## Solution

Implemented smart program path resolution with shell fallback in `/Users/vinh.nguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/tools/pty.rs`:

### Key Changes

1. **Added `resolve_program_path()` function** (line 1020)
   - Checks if program is absolute or relative path
   - Searches for program in PATH environment variable
   - Returns `Some(path)` if found, `None` otherwise

2. **Updated `run_command()` method** (line 443)
   - First checks if program can be resolved in PATH
   - If found, uses direct execution (no overhead)
   - If not found, wraps command in shell: `/bin/sh -c "program args"`

3. **Updated `create_session()` method** (line 656)
   - Applied the same logic for PTY session creation
   - Ensures consistent behavior across all command execution paths

### How It Works

```
Command: cargo fmt

┌─ Try to find cargo in PATH
│  ├─ Found at /Users/user/.cargo/bin/cargo
│  └─ Execute directly: /Users/user/.cargo/bin/cargo fmt
│
└─ Not found in PATH
   └─ Execute via shell: /bin/sh -c "cargo fmt"
      └─ Shell uses its own PATH resolution and finds cargo
```

## Benefits

- **Backward Compatible**: Programs in standard locations execute directly (no overhead)
- **Transparent**: Users don't need to know about the fallback mechanism
- **Efficient**: Direct execution when possible, shell fallback only when needed
- **Cross-Platform**: Uses `/bin/sh` which is available on all Unix-like systems

## Testing

All tests pass:
- ✓ `cargo test --lib` - Unit tests pass
- ✓ `cargo check` - No compilation warnings
- ✓ Integration test with `cargo --version`
- ✓ Integration test with `rustc --version`
- ✓ Integration test with `which cargo`

## Files Modified

- `vtcode-core/src/tools/pty.rs` - Added `resolve_program_path()` and updated both `run_command()` and `create_session()` methods

## Verification

To verify the fix works:

```bash
# Should now find and execute cargo
vtcode exec "cargo fmt"

# Should find rustc
vtcode exec "rustc --version"

# Should use shell resolution
vtcode exec "which cargo"
```

All three commands should succeed without "command not found" errors.
