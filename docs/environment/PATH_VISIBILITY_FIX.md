# PATH Visibility Fix for VT Code Agent

## Problem

The VT Code agent was unable to execute commands that were in the system PATH, such as `cargo`, even though they were installed and accessible from the shell. This occurred because the environment variable initialization was not inheriting the parent process's environment variables, particularly `PATH`.

## Root Cause

Two functions responsible for environment setup were creating fresh environments without preserving the parent process's environment:

1. **`vtcode-core/src/tools/command.rs`** - `execute_terminal_command()`:

    - Created an empty `HashMap` and only set `PAGER`, `GIT_PAGER`, and `LESS`
    - Did not inherit `PATH` or other system environment variables

2. **`vtcode-core/src/tools/pty.rs`** - `set_command_environment()`:
    - Set TTY-specific variables but did not inherit parent environment
    - This prevented commands from finding executables in user paths (e.g., `~/.cargo/bin`, `/opt/homebrew/bin`)

## Solution

### 1. Non-PTY Command Execution (`command.rs`)

Changed from creating an empty environment to inheriting all parent environment variables:

```rust
// Before
let mut env = HashMap::new();

// After
let mut env: HashMap<OsString, OsString> = std::env::vars_os()
    .collect();
```

Then override specific variables as needed:

```rust
env.insert(OsString::from("PAGER"), OsString::from("cat"));
env.insert(OsString::from("GIT_PAGER"), OsString::from("cat"));
env.insert(OsString::from("LESS"), OsString::from("R"));
```

### 2. PTY Command Execution (`pty.rs`)

Updated `set_command_environment()` to preserve parent environment before setting TTY-specific overrides:

```rust
// Inherit environment from parent process to preserve PATH and other important variables
for (key, value) in std::env::vars() {
    builder.env(&key, &value);
}

// Override or set specific environment variables for TTY
builder.env("TERM", "xterm-256color");
builder.env("PAGER", "cat");
// ... other overrides
```

## Affected Commands

This fix enables the agent to access all commands listed in `ALLOWED_COMMANDS` in `vtcode-config/src/constants.rs`, including:

### Development Tools

-   **Rust**: `cargo`, `rustc`, `rustfmt`, `rustup`, `clippy`
-   **Node.js**: `npm`, `yarn`, `pnpm`, `bun`, `node`, `npx`
-   **Python**: `python`, `python3`, `pip`, `pip3`, `conda`, `pytest`
-   **Build Systems**: `make`, `cmake`, `ninja`, `meson`, `bazel`

### Version Control

-   `git`, `hg`, `svn`

### Container & Cloud

-   `docker`, `docker-compose`, `podman`
-   `aws`, `gcloud`, `az`, `kubectl`, `helm`

### System Utilities

-   `ls`, `cat`, `grep`, `find`, `which`, `type`, `file`, `stat`
-   Text processing: `awk`, `sed`, `grep`, `cut`, `sort`, `uniq`
-   Archives: `tar`, `zip`, `gzip`, `bzip2`

## Testing

The fix has been verified to work with common commands:

```
 cargo: /Users/vinh.nguyenxuan/.cargo/bin/cargo
 rustc: /Users/vinh.nguyenxuan/.cargo/bin/rustc
 npm: /opt/homebrew/bin/npm
 node: /opt/homebrew/bin/node
 python: /Users/vinh.nguyenxuan/.pyenv/shims/python
 git: /usr/bin/git
 docker: /opt/homebrew/bin/docker
```

## Files Modified

-   `vtcode-core/src/tools/command.rs` - Line 44-56
-   `vtcode-core/src/tools/pty.rs` - Line 921-934

## Security Considerations

The fix preserves the existing security model:

-   Command validation via `validate_command()` still enforces allow/deny lists
-   Sandbox profiles still apply restrictions if configured
-   The agent only inherits the environment of the parent shell, which is expected behavior
-   Color output remains disabled for consistency in PTY mode

## Related Configuration

Users can further customize allowed commands via `vtcode.toml`:

```toml
[commands]
allow_list = ["ls", "pwd", "echo", ...]
allow_glob = ["cargo *", "git *", "python *", ...]
deny_list = ["rm -rf /", "shutdown", ...]
deny_glob = ["rm -rf *", "sudo *", ...]
```

See `docs/development/EXECUTION_POLICY.md` for complete details.
