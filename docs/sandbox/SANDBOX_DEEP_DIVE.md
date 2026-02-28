# VT Code Sandbox Deep Dive

This document describes VT Code's sandboxing implementation, inspired by the [Codex sandbox model](https://pierce.dev/notes/a-deep-dive-on-agent-sandboxes) and the AI sandbox field guide.

## Design Philosophy

VT Code implements **configurable sandbox execution with selective escalation**. Sandboxing is opt-in via `[sandbox].enabled = true`; when enabled, commands flow through a centralized execution system that decides whether to run commands raw, under macOS Seatbelt, or through Linux Landlock + seccomp.

Runtime enforcement for shell execution is applied in the active tool path (`unified_exec` / `run_pty_cmd`) after loading `[sandbox]` from `vtcode.toml`, before PTY process launch.

Key design principles:
1. **Platform-specific implementations unified behind a common policy abstraction**
2. **Configurable sandbox execution with selective escalation when needed**
3. **Session-based trust lists that reduce approval fatigue**
4. **Debug tooling to understand sandbox behavior**

## Sandbox Policies

VT Code supports three main isolation levels (`SandboxPolicy`):

### ReadOnly
- Can read files and answer questions
- No file writes except `/dev/null`
- No network access
- Conservative resource limits (512MB memory, 64 PIDs)

### WorkspaceWrite (Auto mode)
- Read files everywhere
- Write only to workspace roots
- **`.git` directories are read-only** (agents can't corrupt git history)
- Optional network allowlist
- Sensitive paths blocked by default

### DangerFullAccess
- No restrictions
- Use with extreme caution

## Platform-Specific Implementations

### macOS Seatbelt

VT Code uses Apple's Seatbelt framework (`/usr/bin/sandbox-exec`) with dynamically generated SBPL profiles:

```scheme
(version 1)
(deny default)
(allow process-exec)
(allow process-fork)
(allow sysctl-read)
(allow mach-lookup)

; Block sensitive paths FIRST
(deny file-read* (subpath "/Users/user/.ssh"))
(deny file-write* (subpath "/Users/user/.ssh"))
; ... more sensitive paths

; Allow reading everywhere else
(allow file-read*)

; Protect .git directories from writes
(deny file-write* (subpath "/path/to/workspace/.git"))
(allow file-write* (subpath "/path/to/workspace"))
```

### Linux Landlock + seccomp

On Linux, VT Code uses:
- **Landlock** (Linux 5.13+): Capability-based filesystem access control
- **seccomp-BPF**: System call filtering with programmable filters

The sandbox helper receives:
1. `--sandbox-policy`: Landlock filesystem rules
2. `--seccomp-profile`: Blocked syscalls list
3. `--resource-limits`: Memory, PID, disk, CPU limits

## Security Features

### Environment Variable Sanitization

Following Codex: "Completely clear the environment and rebuild it with only the variables you actually want."

**Filtered variables** (removed from sandbox):
- API keys: `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GITHUB_TOKEN`, etc.
- Cloud credentials: `AWS_*`, `AZURE_*`, `GOOGLE_*`
- Dynamic linker: `LD_PRELOAD`, `DYLD_INSERT_LIBRARIES`
- Database passwords: `DATABASE_URL`, `PGPASSWORD`, `MYSQL_PWD`

**Preserved variables**:
- Shell basics: `PATH`, `HOME`, `USER`, `SHELL`, `TERM`
- Build tools: `CARGO_HOME`, `RUSTUP_HOME`, `GOPATH`
- Temp directories: `TMPDIR`, `TEMP`, `TMP`

### Sandbox Markers

Child processes receive environment markers so downstream tools know they're sandboxed:
- `VTCODE_SANDBOX_ACTIVE=1`
- `VTCODE_SANDBOX_NETWORK_DISABLED=1` (when network is blocked)
- `VTCODE_SANDBOX_TYPE=MacosSeatbelt` (or `LinuxLandlock`)
- `VTCODE_SANDBOX_WRITABLE_ROOTS=/path/to/workspace`

### Parent Death Signal (Linux)

Using `prctl(PR_SET_PDEATHSIG, SIGKILL)`:
> "Ensures sandboxed children die if the main process gets killed - you don't want orphaned processes running around."

### Sensitive Path Blocking

Default blocked paths:
- `~/.ssh` - SSH keys
- `~/.aws` - AWS credentials
- `~/.config/gcloud` - Google Cloud
- `~/.kube` - Kubernetes config
- `~/.docker` - Docker registry auth
- `~/.npmrc`, `~/.pypirc` - Package registry tokens
- `~/.gnupg` - GPG keys
- `~/.cargo/credentials.toml` - Cargo registry tokens
- `~/.git-credentials` - Git credentials

### .git Directory Protection

Following the Codex pattern:
> "Agents can modify your workspace but can't mess up your git history."

All `.git` directories within writable roots are explicitly denied write access in the Seatbelt profile.

### Seccomp Syscall Blocking

Dangerous syscalls blocked in sandbox:
- `ptrace` - Debugging/tracing (sandbox escape risk)
- `mount`, `umount` - Filesystem namespace changes
- `init_module`, `finit_module` - Kernel module loading
- `kexec_load` - Kernel replacement
- `bpf` - BPF programs (sandbox escape risk)
- `perf_event_open` - Information leakage
- `userfaultfd` - Race condition exploitation
- `reboot` - System reboot

### Resource Limits

Conservative limits for untrusted code:
- Memory: 512MB
- PIDs: 64 (prevents fork bombs)
- Disk: 1GB writes
- CPU: 60 seconds
- Wall clock: 120 seconds

## Debug Tooling

Test sandbox configurations without affecting production:

```rust
use vtcode_core::sandboxing::{debug_sandbox, sandbox_capabilities_summary, SandboxType, SandboxPolicy};

// Show sandbox capabilities
println!("{}", sandbox_capabilities_summary());

// Test a command through the sandbox
let result = debug_sandbox(
    SandboxType::platform_default(),
    &SandboxPolicy::workspace_write(vec!["/tmp/test".into()]),
    &["ls", "-la"].iter().map(|s| s.to_string()).collect::<Vec<_>>(),
    Path::new("/tmp/test"),
    None,
).await?;

// Test if a path is writable
let writable = test_path_writable(
    &policy,
    Path::new("/tmp/test"),
    Path::new("/tmp"),
    None,
).await?;

// Test if network is blocked
let blocked = test_network_blocked(&policy, Path::new("/tmp"), None).await?;
```

## Command Whitelisting

The `ExecPolicyManager` coordinates:
1. **Policy rules**: Prefix-based matching for known commands
2. **Trusted patterns**: Session-scoped amendments
3. **Heuristics**: Safe commands (`ls`, `cat`) auto-allowed, dangerous commands (`rm`, `sudo`) forbidden

```rust
// Known safe read-only commands - auto-allowed
let safe_commands = ["ls", "cat", "head", "tail", "grep", "find", "echo", "pwd", ...];

// Known dangerous commands - forbidden by default
let dangerous_commands = ["rm", "rmdir", "dd", "mkfs", "shutdown", "sudo", "su", ...];
```

## References

- [A deep dive on agent sandboxes](https://pierce.dev/notes/a-deep-dive-on-agent-sandboxes) by Pierce Freeman
- [OpenAI Codex CLI](https://github.com/openai/codex) - Sandbox implementation source
- [Landlock documentation](https://docs.kernel.org/userspace-api/landlock.html)
- [macOS Sandbox (Seatbelt)](https://reverse.put.as/wp-content/uploads/2011/09/Apple-Sandbox-Guide-v1.0.pdf)
