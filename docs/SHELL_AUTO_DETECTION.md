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

- `resolve_shell_preference()` - Infallible function that implements the full priority chain and always returns a valid shell path
- `resolve_shell_candidate()` - Aggressive shell detection with platform-specific fallbacks
- `detect_posix_shell_candidate()` - Scans for available POSIX shells in standard locations

### Design: Infallible Shell Resolution

`resolve_shell_preference()` is designed as an **infallible** function that always returns `String` (not `Option<String>`):

```rust
fn resolve_shell_preference(explicit: Option<&str>, config: &PtyConfig) -> String {
    explicit
        .and_then(sanitize_shell_candidate)
        .or_else(|| config.preferred_shell.as_deref().and_then(sanitize_shell_candidate))
        .or_else(|| env::var("SHELL").ok().and_then(|value| sanitize_shell_candidate(&value)))
        .or_else(detect_posix_shell_candidate)
        .unwrap_or_else(|| resolve_shell_candidate().display().to_string())
}
```

The `.unwrap_or_else()` at the end guarantees a shell is **always** found, even if all other options fail. This eliminates the need for `.or_else()` calls at usage sites and ensures shell wrapping always occurs.

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
