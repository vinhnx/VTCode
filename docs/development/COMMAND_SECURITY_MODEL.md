# VT Code Command Security Model

Model-facing schemas preserve request intent (`sandbox_permissions`, `additional_permissions`, and `justification`)
without embedding the active approval policy or its current value. The execution gateway resolves those stable intent
fields against the live sandbox and approval configuration.

## Additional permission normalization

`additional_permissions` is an additive sandbox request. When it contains at least one filesystem permission, an omitted
`sandbox_permissions` value or an explicit `use_default` value is normalized to `with_additional_permissions`. The
public enum values remain unchanged; this normalization only admits a request shape that already expresses the same
intent.

Every requested path is normalized relative to the command working directory, then checked against the workspace and
configured temporary roots. Parent traversal, sensitive paths, paths outside those roots, and symlink escapes are
rejected before execution. Explicit `require_escalated` or `bypass_sandbox` cannot be combined with
`additional_permissions`, and both modes require a non-empty justification. These conflicts fail closed rather than
silently downgrading or broadening the request.

## Overview

VT Code implements a comprehensive, defense-in-depth command security system that enables non-powered users to run safe
commands by default while protecting against dangerous operations. This system helps the agent use system and build
tools properly via environment PATH configuration.

### Native plugin boundary

Opening a dynamic library can execute native initialization code before the library's metadata or ABI is validated.
Repository-controlled `.agents/plugins/` and `.vtcode/plugins/` directories therefore remain metadata-only; the
high-level skill loader does not add them to the native loader's trusted roots. The `load_skill` tool is
approval-required because executable-backed skill implementations must never inherit the read-only policy used for
ordinary skill instructions. Low-level native loading is reserved for callers that have already established plugin
provenance and explicit user consent.

## Design Philosophy

**Safe-by-default**: All known safe commands for development and system utilities are enabled without requiring user
confirmation or configuration.

**Layered Defense**: Multiple validation layers (allow_list, allow_glob, deny_list, deny_glob, allow_regex, deny_regex)
work together to prevent dangerous commands from executing.

**Deny-rules-first**: If a command matches any deny pattern, it is blocked regardless of allow patterns.

## Architecture

### Configuration Sources (Precedence Order)

1. **vtcode.toml** - User/project-level overrides (highest priority)
2. **crates/codegen/vtcode-config/src/core/commands.rs** - Code defaults (runtime)
3. **crates/codegen/vtcode-config/src/constants.rs** - System constants (backup)

### Command Validation Layers

Commands are validated against these layers in order:

```text
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

- **Basic**: `ls`, `pwd`, `cat`, `head`, `tail`, `echo`, `printf`
- **Search**: `grep`, `find`, `locate`
- **Analysis**: `wc`, `sort`, `uniq`, `cut`, `awk`, `sed`
- **Inspection**: `file`, `stat`, `diff`, `tree`, `du`, `df`

#### 2. Version Control (Git/Hg/SVN)

Inspect repository state and manage commits safely:

- **Inspection**: `git status`, `git log`, `git show`, `git diff`, `git branch`
- **Safe workflows**: `git fetch`, `git pull`, `git add`, `git commit`, `git stash`, `git tag`
- **Other VCS**: `hg`, `svn`, `git-lfs`

#### 3. Build Systems

Core compilation and build tool execution:

- **Make-based**: `make`, `cmake`, `ninja`, `meson`, `bazel`
- **Rust ecosystem**: `cargo`, `rustc`, `rustfmt`, `rustup`, `cargo test`
- **All major subcommands**: `cargo build`, `cargo test`, `cargo check`, `cargo run`, etc.

#### 4. Language Runtimes & Package Managers

Execution and dependency management for all major languages:

- **Python**: `python`, `python3`, `pip`, `pip3`, `virtualenv`, `pytest`, `black`, `flake8`, `mypy`, `ruff`
- **Node.js**: `npm`, `node`, `yarn`, `pnpm`, `bun`, `npx`
- **Go**: `go`, `gofmt`, `golint`
- **Java**: `java`, `javac`, `mvn`, `gradle`
- **C/C++**: `gcc`, `g++`, `clang`, `clang++`

#### 5. Compression & Archiving

Safe data compression without system-level access:

- `tar`, `zip`, `unzip`, `gzip`, `gunzip`, `bzip2`, `bunzip2`, `xz`, `unxz`

#### 6. Container Tools

Docker and container platforms:

- `docker`, `docker-compose` (with restrictions on `docker run`)
- **Note**: `docker run *` is denied; containers require careful review

#### 7. System Information

Safe read-only system monitoring:

- `ps`, `top`, `htop` - Process listing and monitoring
- `df`, `du` - Disk usage
- `whoami`, `hostname`, `uname` - System identity

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

- **Root deletion**: `rm -rf /`, `rm -rf /*`, `rm -rf /home`, `rm -rf /usr`, `rm -rf /etc`
- **Home deletion**: `rm -rf ~`
- **Filesystem tools**: `mkfs`, `mkfs.ext4`, `fdisk`, `dd if=/dev/*`

#### 2. System Shutdown/Reboot

- `shutdown`, `reboot`, `halt`, `poweroff`
- `systemctl poweroff`, `systemctl reboot`, `systemctl halt`
- `init 0`, `init 6`

#### 3. Privilege Escalation

- Any `sudo` command: `sudo rm`, `sudo chmod`, `sudo bash`, etc.
- Root switching: `su root`, `su -`
- Admin shells: `sudo -i`, `nohup bash -i`, `exec bash -i`

#### 4. Filesystem Mounting/Unmounting

- `mount`, `umount` - Prevent unauthorized filesystem manipulation

#### 5. Disk/Data Destruction

- `format`, `fdisk`, `mkfs`, `shred`, `wipe`
- `dd if=/dev/zero`, `dd if=/dev/random`, `dd if=/dev/urandom`

#### 6. Permission/Ownership Changes

- `chmod 777`, `chmod -R 777` - Make files world-writable (dangerous)
- `chown -R`, `chgrp -R` - Recursive ownership changes

#### 7. Shell Exploits

- **Fork bomb**: `:(){ :|:& };:`
- **Code evaluation**: `eval` - Prevents arbitrary code injection
- **Config sourcing**: `source /etc/bashrc`, `source ~/.bashrc`

#### 8. Sensitive Data Access

- **User databases**: `cat /etc/passwd`, `cat /etc/shadow`
- **SSH keys**: `cat ~/.ssh/id_*`, `rm ~/.ssh/*`, `rm -r ~/.ssh`
- **System logs**: `tail -f /var/log`, direct log access

#### 9. Process Control

- `kill`, `pkill` - Process termination
- **Note**: Allows monitoring (`ps`, `top`, `htop`) but not process killing

#### 10. Service Management

- `systemctl *` - System service manipulation (denied at glob level)
- `service *` - Legacy service management
- `crontab`, `at` - Task scheduling (dangerous for automation)

VT Code supports automation through internal scheduling primitives instead:

- `vtcode schedule` for durable local automation and reminders

#### 11. Container/Orchestration

- `kubectl *` - Kubernetes operations (admin access)
- `docker run *` - Container creation (requires careful review)

### Mode-Sensitive Git Operations

Preflight hard-denies only the destructive modes of guarded git subcommands; recoverable invocations pass preflight and
proceed through normal policy/approval routing (consistent with `exec_policy`'s `validate_git_reset`):

- **Blocked**: `git reset --hard/--merge/--keep`, working-tree `git rm` (any form without `--cached`), forced branch
  deletion (`git branch -D`, `--delete --force`, stacked `-dD`/`-df`), plus the pre-existing `git push --force` and
  `git clean --force` rules.
- **Pass preflight**: `git reset` (bare/`--soft`/`--mixed` — reflog-restorable), `git rm --cached` (index-only),
  `git branch -d`/`--delete` (refuses unmerged branches).
- `--` ends option parsing: for the option-only subcommands (`reset`/`rm`/`branch`/`clean`) classification scans only
  the pre-`--` arguments, so a literal `--cached`/`--hard`/`-D` **after** `--` is a path or ref name and cannot hide or
  carry a flag (`git rm -- --cached f` stays blocked as a working-tree delete). `push` scans all arguments: its
  dangerous payloads are refspecs, which legitimately occupy the post-`--` positional slot
  (`git push origin -- :refs/heads/x` deletes a remote branch).
- Rejections name the matched pattern and the remedy (for example, use `git stash` or `git reset --soft` instead of
  `git reset --hard`). Sudo/env-wrapped destructive forms stay blocked.

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

- Rust tools: `cargo`, `rustc`, `rustfmt`, `rustup`
- Python tools: `pytest`, `black`, `flake8`, `mypy`
- Node tools: `npm`, `yarn`, `node`
- Go tools: `go`, `gofmt`
- And all other build/development tools installed via package managers

## Environment Variables

Configuration for tool execution:

```toml
[commands.environment]
RUST_BACKTRACE = "1"
PATH = { append = ["$HOME/.cargo/bin"] }
HOME = "$HOME"
```

### Sandbox environment inheritance

Restrictive policies apply to active pipe and PTY sessions as well as direct command launches. The sandbox environment
builder removes credential, token, cloud-provider, linker, and dynamic-loader variables case-insensitively. Extra
environment entries supplied by a command override are filtered through the same rule, so an override cannot restore a
sensitive inherited variable. `PYTHONPATH` and `NODE_PATH` are not inherited wholesale; only canonical entries already
contained by the active workspace may cross a restrictive boundary.

Simple argv commands execute directly. A shell is used only for explicit shell syntax after the shared parser,
command-safety, redirection, policy, approval, and sandbox checks. Admission recursively unwraps `env`, assignments,
`sudo`, and explicit shell `-c`/`-lc` layers and recognizes executables by basename; dynamic executable names fail
closed. Interpreter inline-code flags—Python `-c`, Node/Ruby/Perl/AppleScript `-e`, PHP `-r`, and PowerShell command or
encoded-command forms—are code-execution boundaries and require an enforceable sandbox policy or approval.

MCP stdio launches use the same `SandboxManager` transformation through `McpSandboxContext`; the context is inherited by
initial connections, pool workers, and reconnects. `McpClient::new` remains a compatibility constructor for unsandboxed
library callers. MCP stderr is capped and secret-redacted before logging.

The platform contract is intentionally conservative. Linux restrictive policies are enforced by the built-in launcher
(below) when the kernel supports Landlock and fail closed otherwise. Windows restrictive policies return an explicit
unsupported error because native restricted-token isolation is not implemented. macOS keeps full-network and
blocked-network modes, but rejects hostname allowlists rather than widening them to port-wide access: Seatbelt is not a
documented, reliable third-party domain-filtering contract.

### Linux kernel enforcement (Landlock + seccomp)

Linux restrictive policies are enforced by the VT Code binary itself, which doubles as the sandbox helper (busybox
dispatch). Command transforms wrap sandboxed commands as
`vtcode sandbox-exec --sandbox-policy-cwd … --sandbox-policy … --seccomp-profile … --resource-limits … -- <command>`,
and the launcher applies the restrictions to itself before exec-ing the wrapped command.
`VTCODE_LINUX_SANDBOX_EXECUTABLE` still overrides the helper with an external binary that accepts the same protocol.

Enforcement (see `vtcode-safety/src/sandboxing/linux.rs`):

- **Landlock** (kernel 5.13+): reads granted everywhere except sensitive credential paths (including via symlinks whose
  target is or contains a sensitive path); writes granted only for writable roots (read-only policies may write
  `/dev/null` only). Execute paths and device ioctls stay unrestricted, matching the macOS profile: `Execute` and
  `IoctlDev` are excluded from the handled rights set (`from_read` includes `Execute`, and `from_write` includes
  `IoctlDev` from kernel ABI v5 on), rather than granted — handling `IoctlDev` would deny TTY ioctls to PTY-attached
  commands. Landlock has no deny rules, so `.git`/`.vtcode` write protection inside writable roots remains at the
  preflight layer on Linux (macOS enforces it in the kernel).
- **seccomp-BPF** (`PR_SET_NO_NEW_PRIVS` + filters): blocks dangerous syscalls (`ptrace`, `mount`, `bpf`, `unshare`,
  `setns`, …), rejects namespace-creating `clone` flags and blocks `clone3` with `ENOSYS` so libc falls back to filtered
  `clone`, and denies `AF_INET`/`AF_INET6` socket creation when the policy denies network. Unix sockets stay available.
- **rlimits**: applied only when a workspace-write policy explicitly configures them.
- Kernels without Landlock fail closed (`UnavailableSandboxType`), matching the Windows posture. Hostname network
  allowlists remain unsupported and fail closed on both platforms until the managed network proxy lands.

Tests: `vtcode-safety` unit tests cover grant computation and filter construction; `tests/sandbox_exec_integration.rs`
exercises the real binary on Linux and needs a Landlock-capable kernel (in containers, run with
`--security-opt seccomp=unconfined`).

### Rule-file layers

Rule files (`.rules`, TOML/JSON/simple formats — see `exec_policy::PolicyParser`) can declare prefix rules with
`allow`/`prompt`/`forbidden` decisions. VT Code discovers them in two layers, highest precedence first:

1. `<workspace>/.vtcode/rules/*.rules` — project rules
2. `~/.vtcode/rules/*.rules` — user rules

`exec_policy::rules_layers` enumerates the layers (sorted within each directory) and
`ExecPolicyManager::load_policy_layers` merges them so a pattern defined in a higher-precedence layer wins.
`exec_policy::shared_exec_policy_manager_with_rules` builds a manager with both layers auto-loaded.

### Provider diagnostics

All provider error metadata, user-facing messages, logs, OpenRouter fallback errors, OpenAI diagnostics, legacy provider
errors, and custom auth-command stderr use the bounded provider diagnostic sanitizer. HTTP error streams are capped at
16 KiB before parsing; the sanitizer is UTF-8-safe, caps exposed diagnostics at 8 KiB, and redacts API keys, bearer
tokens, cloud credentials, and generic secret assignments. Status codes, request IDs, retry metadata, classification,
and 401 refresh behavior are preserved separately from the diagnostic text.

Provider-owned child processes also receive a filtered inherited environment. Local model-server helpers and custom
provider auth commands exclude unrelated API keys, cloud credentials, tokens, linker overrides, and dynamic-loader
variables. Copilot and its optional `gh` probe receive only their documented GitHub authentication variables as explicit
exceptions.

### Workspace provider configuration trust boundary

The configuration loader treats workspace-root files, workspace `.vtcode/` files, and project profiles as
repository-controlled input. After layer merge, it rejects non-empty `custom_providers` values from those sources before
they can reach provider registration. This prevents repository configuration from introducing command-backed custom
authentication (`auth.command`).

It also rejects repository-controlled `provider_overrides.<name>.base_url` and `.api_key_env` values, which could
redirect model requests or select credentials from an environment variable. Origin checks still apply when
`workspace.use_root_config` discards lower layers. System/user config, explicitly selected config files, and explicit
runtime overrides remain trusted opt-in paths.

Normal startup and live reload can repair legacy repository files produced by older full-config writes: only the
protected provider fields are removed atomically before strict loading is retried. Malformed or symlinked files are
still rejected, and explicitly selected config files are never modified.

## Audit & Logging

The permission system logs all decisions for security and debugging:

```toml
[permissions]
enabled = true
audit_enabled = true
audit_directory = "<state>/audit"
log_allowed_commands = true
log_denied_commands = true
cache_ttl_seconds = 300
```

**Audit logs track:**

- Allowed commands executed
- Blocked/denied commands attempted
- Permission decision cache hits
- Command resolution paths

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

Accidental destructive commands Privilege escalation attempts Malicious shell exploits (forkbombs, eval injection)
Sensitive data exposure (SSH keys, password files) System shutdown/corruption Filesystem manipulation

### What This Does NOT Protect Against

Compromised agent LLM (if it's compromised, it can craft allowed commands to cause harm) Commands that are allowed but
have dangerous flags (e.g., `cargo build --offline` with missing dependencies) Zip bombs or other valid-but-malicious
allowed file operations Side effects of running safe commands in a bad state

### Best Practices

1. **Keep deny_list comprehensive** - Always block system-altering commands
2. **Use allow_glob sparingly** - More specific allow_list entries are safer
3. **Monitor audit logs** - Review the user state directory's `audit/` path regularly for suspicious patterns
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

- [docs/development/EXECUTION_POLICY.md](./EXECUTION_POLICY.md) - Overall execution policy
- [crates/codegen/vtcode-config/src/core/commands.rs](../../crates/codegen/vtcode-config/src/core/commands.rs) -
  Implementation

## Shared preflight and process boundaries

All providers use the same command preflight. Literal input/output redirection destinations are validated before command
classification removes shell plumbing; dynamic destinations are rejected. Ordinary log files, descriptor duplication,
and `/dev/null` remain supported. Filesystem authorization resolves current symlink targets for every operation;
previous resolutions are never reused as permission evidence. This preflight complements the OS sandbox and cannot alone
eliminate concurrent filesystem replacement races.

Restrictive sandbox launches reconstruct the environment from the central allowlist after caller overrides. Arbitrary
names are not inherited, including credentials that do not match familiar token/key suffixes. Explicit full-access and
externally managed sandbox policies preserve their existing environment semantics.

Child working directories are canonicalized as an error-producing operation, then bound by directory descriptor before
launch. In-process filesystem opens use descriptor-relative no-follow primitives. Restrictive execution fails closed
when the platform cannot establish its required confinement.
