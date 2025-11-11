# Shell Auto-Detection & Fallover Logic

## Overview

VT Code now automatically detects and uses the user's preferred shell without requiring manual configuration. The shell resolution system uses a intelligent fallback chain to ensure commands always execute in an appropriate shell environment.

## Shell Resolution Priority

The shell is resolved in this order:

1. **Explicit parameter** - Shell specified in the command payload (e.g., `shell: "/bin/zsh"`)
2. **Configuration** - `preferred_shell` setting in `vtcode.toml`
3. **Environment variable** - `$SHELL` environment variable
4. **Auto-detection** - Scans for available shells:
   - `/bin/zsh` (preferred)
   - `/usr/bin/zsh`
   - `/bin/bash`
   - `/usr/bin/bash`
   - `/bin/sh`
   - `/usr/bin/sh`
5. **System default fallback**:
   - **POSIX (macOS/Linux)**: `/bin/sh`
   - **Windows**: `powershell.exe` (if available) or `cmd.exe`

## Configuration

### Default Behavior (Recommended)

Leave `preferred_shell` commented out in `vtcode.toml` to enable auto-detection:

```toml
# preferred_shell is commented - auto-detection enabled
# preferred_shell = "/bin/zsh"
```

This allows the system to:
- Respect your `$SHELL` environment variable
- Auto-detect available shells if `$SHELL` is not set
- Fall back to system defaults gracefully

### Explicit Configuration

If you want to force a specific shell:

```toml
[pty]
preferred_shell = "/bin/zsh"
```

## Implementation Details

### Code Changes

The shell resolution is implemented in `vtcode-core/src/tools/registry/executors.rs`:

- `resolve_shell_preference()` - Implements the priority chain
- `resolve_shell_candidate()` - Provides aggressive fallback detection
- `detect_posix_shell_candidate()` - Scans for available POSIX shells

### Automatic Fallback

When executing PTY commands (`prepare_ephemeral_pty_command`, `send_pty_input`), if `resolve_shell_preference()` returns `None`, the code automatically falls back to `resolve_shell_candidate()`:

```rust
let shell = resolve_shell_preference(...)
    .or_else(|| {
        // Fallback: if preference resolution fails, use aggressive shell detection
        Some(resolve_shell_candidate().display().to_string())
    });
```

This ensures that shell wrapping always occurs, preventing commands from executing without proper shell environment setup.

## Benefits

- **Works out of the box**: No configuration needed for typical setups
- **Respects user preferences**: Uses `$SHELL` when available
- **Handles edge cases**: Falls back gracefully on systems with unusual shell locations
- **Cross-platform**: Intelligent defaults for Windows and POSIX systems
- **Cargo/Development tools work automatically**: Shell initialization files (e.g., `.zshrc` with cargo environment setup) are sourced correctly

## Troubleshooting

If commands still fail to find tools like `cargo`:

1. **Check `$SHELL`**: Verify your environment variable is set correctly
   ```bash
   echo $SHELL
   ```

2. **Verify shell initialization**: Ensure tool paths are set in your shell config:
   ```bash
   # For zsh
   cat ~/.zshrc | grep cargo
   
   # For bash
   cat ~/.bashrc | grep cargo
   ```

3. **Force a shell**: Set `preferred_shell` in `vtcode.toml` if auto-detection fails:
   ```toml
   [pty]
   preferred_shell = "/bin/zsh"
   ```

4. **Check shell paths**: Verify the shell binary exists:
   ```bash
   which zsh
   which bash
   ```
