# VT Code Environment Setup and PATH Visibility Guide

## Overview

This guide explains how VT Code manages environment variables when executing commands, how the PATH visibility fix works, and how to ensure your tools are accessible to the agent.

## Quick Start

If you just installed VT Code and want your custom tools (cargo, npm, python, etc.) to work:

1. **Ensure PATH is set**: Run `echo $PATH` in your shell to verify your environment is configured
2. **Rebuild VT Code**: Use `cargo build --release` to get the latest version with PATH inheritance
3. **Test availability**: Run `which cargo` to verify your tools are in PATH
4. **Use the agent**: Custom tools should now be accessible

## How VT Code Manages Environment Variables

### Environment Inheritance Model

VT Code now inherits the parent shell's environment variables and applies strategic overrides:

```

 Parent Shell Environment (inherited)

 • PATH (preserved)
 • HOME (preserved)
 • SHELL (preserved)
 • Custom env vars (preserved)

            ↓
        VT Code Process
            ↓

 VT Code Environment (modified)

 • All inherited variables PLUS:
 • PAGER → "cat"
 • GIT_PAGER → "cat"
 • LESS → "R"
 • TERM → "xterm-256color"
 • Color vars → disabled
 • Workspace vars added

            ↓
        Command Execution
```

### Preserved Environment Variables

The following variables are inherited from the parent environment:

**Critical for Command Discovery**:

-   `PATH` - Command search paths (enables finding tools in ~/.cargo/bin, ~/.local/bin, etc.)
-   `HOME` - User home directory
-   `SHELL` - Current shell program

**Important for Workflows**:

-   `USER`, `LOGNAME` - User identity
-   `PWD` - Current working directory
-   `EDITOR`, `VISUAL` - Default text editors
-   `LANG`, `LC_*` - Locale settings
-   `GOPATH`, `GOROOT` - Go environment
-   `RUSTUP_HOME`, `CARGO_HOME` - Rust environment
-   `PYTHON*` variables - Python environment
-   `NODE_*` variables - Node.js environment
-   Custom user-defined variables

**VT Code-Added Variables**:

-   `WORKSPACE_DIR` - Path to the workspace root
-   `VT_SANDBOX_*` - Sandbox-related variables (if configured)

### Environment Overrides (for Consistency)

The following variables are **always overridden** for consistent behavior:

| Variable         | Value            | Reason                                           |
| ---------------- | ---------------- | ------------------------------------------------ |
| `PAGER`          | `cat`            | Prevent interactive pagers blocking agent output |
| `GIT_PAGER`      | `cat`            | Prevent git from waiting for user input          |
| `LESS`           | `R`              | ANSI color escape handling                       |
| `TERM`           | `xterm-256color` | Standard terminal capabilities                   |
| `CLICOLOR`       | `0`              | Disable automatic color output                   |
| `CLICOLOR_FORCE` | `0`              | Prevent color forcing                            |
| `LS_COLORS`      | `` (empty)       | Consistent output formatting                     |
| `NO_COLOR`       | `1`              | Standard color disable protocol                  |

## Command Execution Paths

### Non-PTY Command Execution (run_pty_cmd)

Used for simple, non-interactive commands:

1. Inherits parent environment: `std::env::vars_os().collect()`
2. Overrides specific variables for consistency
3. Validates command against allow/deny lists
4. Executes via `AsyncProcessRunner`

### PTY Session Execution (create_pty_session, run_pty_cmd)

Used for interactive terminal sessions:

1. Inherits parent environment via loop: `for (key, value) in std::env::vars()`
2. Overrides specific variables for consistency and TTY handling
3. Sets terminal dimensions (COLUMNS, LINES)
4. Enables full interactive capabilities
5. Preserves scrollback and history

## Verifying Your Environment Setup

### Check if Tools Are Discoverable

```bash
# Verify PATH is set
echo $PATH | tr ':' '\n' | head -10

# Check if specific tools are in PATH
which cargo
which npm
which python3
which git

# Use 'type' for shell builtins and functions
type ls
type cd
```

### Common Tool Locations

| Tool                | Common Paths                                                          |
| ------------------- | --------------------------------------------------------------------- |
| Rust (cargo, rustc) | `~/.cargo/bin`                                                        |
| Python (pip, pipx)  | `~/.local/bin`, `~/.pyenv/shims`                                      |
| Node.js             | `/usr/local/bin`, `/opt/homebrew/bin`                                 |
| Homebrew            | `/opt/homebrew/bin` (macOS), `/home/linuxbrew/.linuxbrew/bin` (Linux) |
| Go                  | `~/go/bin`, `/usr/local/go/bin`                                       |
| Local scripts       | `~/bin`, `~/.local/bin`                                               |

### Debug PATH Issues

If a tool isn't found:

```bash
# 1. Verify it's installed
which cargo
cargo --version

# 2. Check if PATH includes the tool's directory
echo $PATH | grep ".cargo"

# 3. See all available commands in a directory
ls -la ~/.cargo/bin/

# 4. Test if the tool is executable
stat -f "%A %N" ~/.cargo/bin/cargo  # macOS
stat --format="%A %n" ~/.cargo/bin/cargo  # Linux

# 5. Test invocation directly
/Users/username/.cargo/bin/cargo --version
```

## Configuring Allowed Commands

### Default Behavior

By default, VT Code allows a comprehensive set of safe commands. See `ALLOWED_COMMANDS_REFERENCE.md` for the full list.

### Custom Configuration (vtcode.toml)

```toml
[commands]
# Explicitly allow commands
allow_list = [
    "my-custom-tool",
    "special-script"
]

# Allow command patterns (glob)
allow_glob = [
    "my-tool *",
    "custom-*.sh"
]

# Block specific commands or patterns
deny_list = [
    "dangerous-command"
]

deny_glob = [
    "rm -rf *",
    "sudo *"
]

# Advanced: Regex patterns
allow_regex = [
    "^my-project-.*"
]

deny_regex = [
    ".*-delete$"
]
```

### Tool-Level Policies

```toml
[tools.policies]
run_pty_cmd = "allow"      # Execute without confirmation
apply_patch = "prompt"           # Ask before applying patches
write_file = "allow"             # Allow file writes
edit_file = "allow"              # Allow file edits
```

## Troubleshooting

### Issue: "command not found: cargo"

**Symptoms**: Agent can't find cargo even though `which cargo` works in your shell

**Solutions**:

1. **Rebuild VT Code** - Ensure you have the latest version with the PATH fix:

    ```bash
    cargo build --release
    ```

2. **Verify PATH in shell**:

    ```bash
    echo $PATH
    which cargo
    ```

3. **Check installation**:

    ```bash
    ~/.cargo/bin/cargo --version
    ```

4. **Test environment inheritance**:
    ```bash
    # This should show cargo is available
    cargo --version
    ```

### Issue: Command works in shell but not in VT Code agent

**Possible causes**:

1. **Command not in allow-list**

    - Check `ALLOWED_COMMANDS_REFERENCE.md`
    - Add to `allow_list` or `allow_glob` in `vtcode.toml`

2. **Command in deny-list**

    - Check `deny_list` or `deny_glob` in `vtcode.toml`
    - Remove from deny list or check policy enforcement

3. **PATH difference**

    - Verify: `echo $PATH | grep ~/.cargo/bin`
    - Should show your tool's directory

4. **Environment variable needed**
    - Some tools require env vars (e.g., `GOPATH` for Go)
    - Check if the variable is inherited: `echo $VAR_NAME`

### Issue: Colors are disabled in output

**This is expected** - VT Code disables colors for consistent parsing:

```toml
[tools.policies]
allow_tool_ansi = true  # Enable ANSI escape sequences (optional)
```

## Environment Variables by Use Case

### Rust Development

```
CARGO_HOME=/Users/username/.cargo
RUSTUP_HOME=/Users/username/.rustup
PATH includes ~/.cargo/bin
```

### Python Development

```
PYENV_ROOT=/Users/username/.pyenv
PATH includes ~/.pyenv/shims
VIRTUAL_ENV=/path/to/venv (when activated)
```

### Node.js Development

```
NVM_HOME=/Users/username/.nvm (if using nvm)
PATH includes /opt/homebrew/bin or ~/.nvm/versions/node/vXX/bin
```

### Go Development

```
GOPATH=/Users/username/go
GOROOT=/usr/local/go
PATH includes $GOPATH/bin
```

## Best Practices

1. **Keep PATH minimal** - Only include necessary directories
2. **Use version managers** - rbenv, nvm, pyenv, rustup provide clean PATH management
3. **Test in shell first** - Verify commands work before debugging in VT Code
4. **Check allow-lists** - Ensure custom commands are in vtcode.toml if needed
5. **Use absolute paths** - For debugging: `/full/path/to/command --version`
6. **Enable tool policies** - Use `allow`, `prompt`, or `deny` appropriately

## Advanced: Custom Environment Scripts

You can set up custom environment initialization via hooks:

```toml
[hooks.lifecycle]
session_start = [
    { hooks = [
        { command = "$HOME/.vtcode/setup-env.sh", timeout_seconds = 30 }
    ] }
]
```

Example `~/.vtcode/setup-env.sh`:

```bash
#!/bin/bash
# Initialize environment for the session
export RUST_BACKTRACE=1
export PYTHONUNBUFFERED=1
export NODE_OPTIONS="--max-old-space-size=4096"
```

## See Also

-   `docs/environment/PATH_VISIBILITY_FIX.md` - Technical details of the fix
-   `docs/environment/ALLOWED_COMMANDS_REFERENCE.md` - Complete list of allowed commands
-   `docs/development/EXECUTION_POLICY.md` - Detailed execution policy documentation
-   `docs/guides/security.md` - Security best practices
-   `vtcode.toml` - Configuration file reference

## Environment Inheritance Changelog

### v0.43.3+ (Latest)

-   PATH environment variable properly inherited
-   All parent environment variables preserved
-   User-installed tools (~/.cargo/bin, etc.) now accessible
-   Custom environment variables preserved
-   Security model maintained with strategic overrides

### Earlier Versions

-   PATH not inherited - tools in custom locations not found
-   Environment was mostly empty - only key variables set
-   User-installed tools inaccessible

## Contributing Improvements

If you discover environment-related issues or improvements:

1. Test in your shell first: `which command && command --version`
2. Check `echo $RELEVANT_VAR` for environment variables
3. File an issue with reproduction steps
4. Consider submitting a PR to improve environment handling

## Questions?

-   Check the examples in this directory
-   Review test cases in `tests/integration_path_env.rs`
-   Look at actual source in `vtcode-core/src/tools/command.rs` and `vtcode-core/src/tools/pty.rs`
