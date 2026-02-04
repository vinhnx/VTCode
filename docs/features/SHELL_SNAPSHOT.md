# Shell Environment Snapshot

The shell snapshot feature captures a fully-initialized shell environment (after login scripts run) and reuses it for subsequent command executions, significantly reducing command startup time.

## Problem

Every time VT Code executes a shell command, it traditionally runs the user's login shell (`$SHELL -lc "command"`) which sources all login scripts (`~/.bashrc`, `~/.zshrc`, `~/.bash_profile`, etc.). This can add 100-500ms of overhead per command, depending on the complexity of the user's shell configuration.

## Solution

The shell snapshot feature:

1. **Captures** the shell environment once by running a login shell that dumps all environment variables
2. **Caches** this snapshot in memory with file fingerprints of shell config files
3. **Reuses** the cached environment for subsequent commands
4. **Invalidates** automatically when shell config files change or TTL expires

## Usage

### Using the SnapshotExecutor

```rust
use vtcode_core::tools::shell::{ShellRunner, SnapshotExecutor};

// Create a shell runner that uses snapshots
let runner = ShellRunner::<SnapshotExecutor>::with_snapshot(workspace_root);

// Commands now execute with the cached environment
let output = runner.exec("cargo build").await?;
```

### Using the Global Snapshot Manager

```rust
use vtcode_core::tools::shell_snapshot::global_snapshot_manager;

// Get or capture a snapshot
let snapshot = global_snapshot_manager().get_or_capture().await?;

// Check snapshot statistics
let stats = global_snapshot_manager().stats();
println!("Environment variables: {}", stats.env_count);
println!("Age: {} seconds", stats.age_secs);

// Manually invalidate if needed
global_snapshot_manager().invalidate();
```

### Applying Snapshot to Commands

```rust
use vtcode_core::tools::shell_snapshot::{apply_snapshot_env, global_snapshot_manager};
use tokio::process::Command;

let snapshot = global_snapshot_manager().get_or_capture().await?;

let mut cmd = Command::new("sh");
cmd.arg("-c").arg("echo $PATH");

// Apply snapshot environment (clearing existing env first)
apply_snapshot_env(&mut cmd, &snapshot, true);

let output = cmd.output().await?;
```

## Architecture

### Components

1. **ShellSnapshot**: The captured environment data
   - `env: HashMap<String, String>` - Environment variables
   - `shell_path: String` - The shell used for capture
   - `shell_kind: ShellKind` - Detected shell type (Bash, Zsh, etc.)
   - `captured_at: Instant` - When the snapshot was captured
   - `config_fingerprints: Vec<FileFingerprint>` - File mtimes for invalidation

2. **ShellSnapshotManager**: Manages snapshot lifecycle
   - Thread-safe with `RwLock` for reads, `Mutex` for captures
   - Prevents stampedes when multiple commands trigger capture simultaneously

3. **SnapshotExecutor**: A `CommandExecutor` implementation that uses snapshots
   - Drop-in replacement for `SystemExecutor`
   - Lazy snapshot capture on first use

### Capture Process

The snapshot is captured by running:

```sh
$SHELL -lc "printf '__VTCODE_ENV_BEGIN__\n'; env -0; printf '\n__VTCODE_ENV_END__\n'"
```

This:
1. Uses markers to delimit the environment dump (ignoring shell startup noise)
2. Uses NUL-delimited output (`env -0`) for reliable parsing
3. Filters out volatile variables (PWD, SHLVL, terminal-specific vars, etc.)

### Invalidation Strategy

Snapshots are invalidated when:

1. **TTL expires**: Default 24 hours
2. **Shell path changes**: Different shell binary detected
3. **Config files change**: Any monitored file's mtime or size differs

Monitored files depend on shell type:

**Bash:**
- `/etc/profile`, `~/.bash_profile`, `~/.bash_login`, `~/.profile`, `~/.bashrc`

**Zsh:**
- `/etc/zshenv`, `/etc/zprofile`, `/etc/zshrc`, `/etc/zlogin`
- `~/.zshenv`, `~/.zprofile`, `~/.zshrc`, `~/.zlogin`

**Fish:**
- `/etc/fish/config.fish`, `~/.config/fish/config.fish`

## Excluded Environment Variables

The following variables are not captured to avoid stale or session-specific data:

- Session identifiers: `TERM_SESSION_ID`, `SHELL_SESSION_ID`, `ITERM_SESSION_ID`
- Terminal info: `TERM`, `COLUMNS`, `LINES`, `COLORTERM`
- Working directory: `PWD`, `OLDPWD`
- Shell internals: `SHLVL`, `_`, `BASH_*`, `ZSH_*`
- SSH/multiplexer: `SSH_*`, `TMUX*`, `STY`

## Performance Impact

Typical improvements:

| Scenario | Without Snapshot | With Snapshot | Improvement |
|----------|-----------------|---------------|-------------|
| Simple command | 150-300ms | 10-50ms | 3-6x faster |
| Complex shell config | 300-800ms | 10-50ms | 6-16x faster |
| Multiple commands | N × startup time | Startup once | Linear savings |

## Limitations

1. **Aliases/Functions**: Shell aliases and functions are not captured (they are shell-internal state, not environment variables). Commands relying on aliases may need the full login shell.

2. **Indirect Changes**: Changes to files that are `source`d by the monitored config files won't trigger invalidation. Use manual invalidation or restart VT Code.

3. **Interactive-only Config**: Some shell configurations only apply to interactive shells and may not be captured.

4. **Platform**: Currently Unix-only. Windows shells are not supported.

## Troubleshooting

### Snapshot not capturing correctly

Check the global manager stats:

```rust
let stats = global_snapshot_manager().stats();
if !stats.has_snapshot {
    println!("No snapshot available");
} else {
    println!("Shell: {:?}", stats.shell_kind);
    println!("Env vars: {}", stats.env_count);
    println!("Config files monitored: {}", stats.config_files_monitored);
}
```

### Commands behaving differently

If a command works with `$SHELL -lc` but not with snapshots:

1. The command may rely on shell aliases/functions
2. Use the full login shell for that specific command
3. Or check if a required env var is in the excluded list

### Forcing a refresh

```rust
global_snapshot_manager().invalidate();
let _ = global_snapshot_manager().get_or_capture().await;
```

## Future Enhancements

- **Alias/Function Support**: Optionally capture and replay aliases/functions via a prelude script
- **Disk Persistence**: Cache snapshots to disk for faster cold starts
- **Per-workspace Snapshots**: Different projects may have different environment needs
- **Windows Support**: PowerShell profile caching
