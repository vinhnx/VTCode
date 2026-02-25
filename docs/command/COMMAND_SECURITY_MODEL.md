# VT Code Command Security Model

## Overview

VT Code implements a comprehensive, defense-in-depth command security system that enables non-powered users to run safe commands by default while protecting against dangerous operations. This system helps the agent use system and build tools properly via environment PATH configuration.

## Design Philosophy

**Safe-by-default**: All known safe commands for development and system utilities are enabled without requiring user confirmation or configuration.

**Layered Defense**: Multiple validation layers (allow_list, allow_glob, deny_list, deny_glob, allow_regex, deny_regex) work together to prevent dangerous commands from executing.

**Deny-rules-first**: If a command matches any deny pattern, it is blocked regardless of allow patterns.

## Architecture

### Configuration Sources (Precedence Order)

1. **vtcode.toml** - User/project-level overrides (highest priority)
2. **vtcode-config/src/core/commands.rs** - Code defaults (runtime)
3. **vtcode-config/src/constants.rs** - System constants (backup)

### Command Validation Layers

Commands are validated against these layers in order:

```
Input: command → Check deny_list → Check deny_glob → Check deny_regex
                    ↓ MATCH = DENY
              Check allow_list → Check allow_glob → Check allow_regex
                    ↓ MATCH = ALLOW
              Otherwise → DENY (fail-closed)
```

## Safe Commands (Enabled by Default)

### Categories

#### 1. File System Utilities (Read-Only)

Safely query and display file information without modification:

-   **Basic**: `ls`, `pwd`, `cat`, `head`, `tail`, `echo`, `printf`
-   **Search**: `grep`, `find`, `locate`
-   **Analysis**: `wc`, `sort`, `uniq`, `cut`, `awk`, `sed`
-   **Inspection**: `file`, `stat`, `diff`, `tree`, `du`, `df`

#### 2. Version Control (Git/Hg/SVN)

Inspect repository state and manage commits safely:

-   **Inspection**: `git status`, `git log`, `git show`, `git diff`, `git branch`
-   **Safe workflows**: `git fetch`, `git pull`, `git add`, `git commit`, `git stash`, `git tag`
-   **Other VCS**: `hg`, `svn`, `git-lfs`

#### 3. Build Systems

Core compilation and build tool execution:

-   **Make-based**: `make`, `cmake`, `ninja`, `meson`, `bazel`
-   **Rust ecosystem**: `cargo`, `rustc`, `rustfmt`, `rustup`, `cargo test`
-   **All major subcommands**: `cargo build`, `cargo test`, `cargo check`, `cargo run`, etc.

#### 4. Language Runtimes & Package Managers

Execution and dependency management for all major languages:

-   **Python**: `python`, `python3`, `pip`, `pip3`, `virtualenv`, `pytest`, `black`, `flake8`, `mypy`, `ruff`
-   **Node.js**: `npm`, `node`, `yarn`, `pnpm`, `bun`, `npx`
-   **Go**: `go`, `gofmt`, `golint`
-   **Java**: `java`, `javac`, `mvn`, `gradle`
-   **C/C++**: `gcc`, `g++`, `clang`, `clang++`

#### 5. Compression & Archiving

Safe data compression without system-level access:

-   `tar`, `zip`, `unzip`, `gzip`, `gunzip`, `bzip2`, `bunzip2`, `xz`, `unxz`

#### 6. Container Tools

Docker and container platforms:

-   `docker`, `docker-compose` (with restrictions on `docker run`)
-   **Note**: `docker run *` is denied; containers require careful review

#### 7. System Information

Safe read-only system monitoring:

-   `ps`, `top`, `htop` - Process listing and monitoring
-   `df`, `du` - Disk usage
-   `whoami`, `hostname`, `uname` - System identity

### Glob Patterns for Workflows

Patterns like `git *`, `cargo *`, `npm run *` enable entire command families:

```toml
allow_glob = [
  "git *",              # All git subcommands
  "cargo *",            # All cargo workflows
  "cargo test *",    # Test execution
  "python *",           # Python with any flags
  "npm run *",          # NPM script execution
  "docker *",           # All docker (except restricted)
]
```

## Dangerous Commands (Always Denied)

### Categories

#### 1. Destructive Filesystem Operations

-   **Root deletion**: `rm -rf /`, `rm -rf /*`, `rm -rf /home`, `rm -rf /usr`, `rm -rf /etc`
-   **Home deletion**: `rm -rf ~`
-   **Filesystem tools**: `mkfs`, `mkfs.ext4`, `fdisk`, `dd if=/dev/*`

#### 2. System Shutdown/Reboot

-   `shutdown`, `reboot`, `halt`, `poweroff`
-   `systemctl poweroff`, `systemctl reboot`, `systemctl halt`
-   `init 0`, `init 6`

#### 3. Privilege Escalation

-   Any `sudo` command: `sudo rm`, `sudo chmod`, `sudo bash`, etc.
-   Root switching: `su root`, `su -`
-   Admin shells: `sudo -i`, `nohup bash -i`, `exec bash -i`

#### 4. Filesystem Mounting/Unmounting

-   `mount`, `umount` - Prevent unauthorized filesystem manipulation

#### 5. Disk/Data Destruction

-   `format`, `fdisk`, `mkfs`, `shred`, `wipe`
-   `dd if=/dev/zero`, `dd if=/dev/random`, `dd if=/dev/urandom`

#### 6. Permission/Ownership Changes

-   `chmod 777`, `chmod -R 777` - Make files world-writable (dangerous)
-   `chown -R`, `chgrp -R` - Recursive ownership changes

#### 7. Shell Exploits

-   **Fork bomb**: `:(){ :|:& };:`
-   **Code evaluation**: `eval` - Prevents arbitrary code injection
-   **Config sourcing**: `source /etc/bashrc`, `source ~/.bashrc`

#### 8. Sensitive Data Access

-   **User databases**: `cat /etc/passwd`, `cat /etc/shadow`
-   **SSH keys**: `cat ~/.ssh/id_*`, `rm ~/.ssh/*`, `rm -r ~/.ssh`
-   **System logs**: `tail -f /var/log`, direct log access

#### 9. Process Control

-   `kill`, `pkill` - Process termination
-   **Note**: Allows monitoring (`ps`, `top`, `htop`) but not process killing

#### 10. Service Management

-   `systemctl *` - System service manipulation (denied at glob level)
-   `service *` - Legacy service management
-   `crontab`, `at` - Task scheduling (dangerous for automation)

#### 11. Container/Orchestration

-   `kubectl *` - Kubernetes operations (admin access)
-   `docker run *` - Container creation (requires careful review)

## Validation Rules (Configuration Reference)

### `allow_list` - Explicit Commands

Exact command matches allowed without confirmation.

```toml
[commands]
allow_list = [
  "ls",
  "pwd",
  "git status",
  "cargo build",
]
```

### `deny_list` - Explicit Blocks

Exact command patterns that are always blocked.

```toml
deny_list = [
  "rm -rf /",
  "rm -rf ~",
  "sudo rm",
  ":(){ :|:& };:",
]
```

### `allow_glob` - Glob Patterns

Wildcard patterns for command families.

```toml
allow_glob = [
  "git *",        # All git commands
  "cargo *",      # All cargo commands
  "npm run *",    # NPM scripts
]
```

### `deny_glob` - Denied Patterns

Blocks entire command families.

```toml
deny_glob = [
  "rm *",         # All rm variations
  "sudo *",       # All sudo usage
  "chmod *",      # All chmod variations
]
```

### `allow_regex` - Regex Patterns

Regular expressions for complex allow rules.

```toml
allow_regex = [
  r"^cargo (build|test|run|check|clippy|fmt)\b",
  r"^git (status|log|show|diff|branch)\b",
]
```

### `deny_regex` - Regex Blocks

Regular expressions to block patterns.

```toml
deny_regex = [
  r"rm\s+(-rf|--force|--recursive)",
  r"sudo\s+.*",
  r"docker\s+run\s+.*--privileged",
]
```

## PATH Configuration

The system extends the shell PATH with safe, common locations:

```toml
[commands]
extra_path_entries = [
  "$HOME/.cargo/bin",           # Rust tools (rustup, cargo)
  "$HOME/.local/bin",           # User-installed binaries
  "$HOME/.nvm/versions/node/*/bin",  # Node.js versions
  "/opt/homebrew/bin",          # Homebrew (macOS)
]
```

This allows the agent to access:

-   Rust tools: `cargo`, `rustc`, `rustfmt`, `rustup`
-   Python tools: `pytest`, `black`, `flake8`, `mypy`
-   Node tools: `npm`, `yarn`, `node`
-   Go tools: `go`, `gofmt`
-   And all other build/development tools installed via package managers

## Environment Variables

Configuration for tool execution:

```toml
[commands.environment]
RUST_BACKTRACE = "1"
PATH = { append = ["$HOME/.cargo/bin"] }
HOME = "$HOME"
```

## Audit & Logging

The permission system logs all decisions for security and debugging:

```toml
[permissions]
enabled = true
audit_enabled = true
audit_directory = "~/.vtcode/audit"
log_allowed_commands = true
log_denied_commands = true
cache_ttl_seconds = 300
```

**Audit logs track:**

-   Allowed commands executed
-   Blocked/denied commands attempted
-   Permission decision cache hits
-   Command resolution paths

## Usage with the VT Code Agent

### Default Behavior (Non-Powered Users)

Out of the box:

1. Agent can execute all safe commands from `allow_list`
2. Agent can use pattern-based commands like `git *`, `cargo *`
3. Dangerous commands are automatically blocked with no prompt

### Adding Custom Commands

To enable additional safe commands:

```toml
[commands]
allow_list = [
  # ... existing commands ...
  "custom-build-tool",
  "my-deployment-script",
]
```

Or via globs:

```toml
allow_glob = [
  "my-tool *",
  "custom-build *",
]
```

### Requiring Confirmation for Destructive Operations

When using `run_pty_cmd`, set `confirm=true` for commands that need approval:

```python
# Example from agent
run_pty_cmd(
  command=["git", "reset", "--hard"],
  confirm=True  # Requires user confirmation despite being in allow_glob
)
```

## Customization Guide

### Restrictive Setup (High Security)

```toml
[commands]
# Only allow explicit commands
allow_list = [
  "ls", "pwd", "cat", "git", "cargo", "python"
]

# Block everything else
allow_glob = []
allow_regex = []

# Extensive deny lists
deny_glob = [
  "*",  # Deny everything by default
]
```

### Permissive Setup (Developer Productivity)

```toml
[commands]
# Allow development tools broadly
allow_glob = [
  "git *",
  "cargo *",
  "npm *",
  "python *",
  "*-cli *",     # CLI tools
  "*-build *",   # Build tools
]

# Still block dangerous patterns
deny_glob = [
  "rm *",
  "sudo *",
  "chmod *",
]
```

### Project-Specific Setup

```toml
[commands]
allow_list = [
  "ls", "pwd", "cat", "grep",
]

# Allow project-specific tools
allow_glob = [
  "make *",
  "./scripts/*",
  "docker compose *",
]

deny_glob = [
  "rm -rf",
  "sudo *",
]
```

## Security Considerations

### What This Protects Against

Accidental destructive commands
Privilege escalation attempts
Malicious shell exploits (forkbombs, eval injection)
Sensitive data exposure (SSH keys, password files)
System shutdown/corruption
Filesystem manipulation

### What This Does NOT Protect Against

Compromised agent LLM (if it's compromised, it can craft allowed commands to cause harm)
Commands that are allowed but have dangerous flags (e.g., `cargo build --offline` with missing dependencies)
Zip bombs or other valid-but-malicious allowed file operations
Side effects of running safe commands in a bad state

### Best Practices

1. **Keep deny_list comprehensive** - Always block system-altering commands
2. **Use allow_glob sparingly** - More specific allow_list entries are safer
3. **Monitor audit logs** - Review `~/.vtcode/audit/` regularly for suspicious patterns
4. **Test configurations** - Validate with `cargo test` before deploying
5. **Avoid eval-like patterns** - Never allow `eval`, `source`, dynamic command construction
6. **Isolate workspaces** - Consider separate configurations for different project types

## Examples

### Example 1: Rust Development Project

```toml
[commands]
allow_list = [
  "ls", "pwd", "cat", "grep", "find",
  "git", "cargo", "rustc", "rustfmt",
]

allow_glob = [
  "git *",
  "cargo *",
  "cargo test *",
]

deny_glob = [
  "rm *", "sudo *", "chmod *", "kill *",
]
```

### Example 2: Python Data Science Project

```toml
[commands]
allow_list = [
  "ls", "pwd", "cat", "grep", "find",
  "python", "python3", "pip", "pip3",
  "jupyter", "git",
]

allow_glob = [
  "python *",
  "python3 *",
  "pip *",
  "git *",
  "conda *",
]

deny_glob = [
  "rm *", "sudo *", "chmod *",
]
```

### Example 3: Full-Stack Development (Node + Backend)

```toml
[commands]
allow_list = [
  "ls", "pwd", "cat", "grep", "find",
  "git", "npm", "node", "python",
  "docker", "docker-compose",
]

allow_glob = [
  "git *",
  "npm *",
  "npm run *",
  "node *",
  "python *",
  "docker *",
  "docker-compose *",
]

deny_glob = [
  "rm *", "sudo *", "chmod *", "kill *",
  "docker run *",  # Explicit deny for container creation
]
```

## Testing & Validation

Test command permissions using VT Code's built-in validation:

```bash
# Build and test
cargo build
cargo test

# Check configuration
cargo run -- ask "list safe commands"

# Run with debug logging
RUST_LOG=debug cargo run
```

## See Also

-   [docs/EXECUTION_POLICY.md](./EXECUTION_POLICY.md) - Overall execution policy
-   [docs/PERMISSION_SYSTEM_INTEGRATION.md](./PERMISSION_SYSTEM_INTEGRATION.md) - Permission system details
-   [docs/environment/ALLOWED_COMMANDS_REFERENCE.md](./environment/ALLOWED_COMMANDS_REFERENCE.md) - Complete command reference
-   [vtcode-config/src/core/commands.rs](../vtcode-config/src/core/commands.rs) - Implementation
-   [vtcode.toml.example](../vtcode.toml.example) - Configuration examples
