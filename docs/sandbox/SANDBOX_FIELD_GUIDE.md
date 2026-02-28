# VT Code Sandbox Field Guide

This document describes VT Code's sandboxing architecture, following the principles from the AI sandbox field guide. The system implements defense-in-depth through multiple layers of isolation.

## The Three-Question Model

When evaluating any sandbox, ask these three questions:

1. **What is shared between this code and the host?** (Boundary)
2. **What can the code touch?** (Policy)
3. **What survives between runs?** (Lifecycle)

VT Code's sandbox system is designed around these questions.

## Boundaries

### Local OS Sandboxes (Current Implementation)

VT Code implements kernel-enforced sandboxing using platform-native mechanisms:

| Platform | Mechanism | Description |
|----------|-----------|-------------|
| macOS | Seatbelt | SBPL profiles with deny-default semantics |
| Linux | Landlock + Seccomp | Filesystem rules + syscall filtering |
| Windows | Restricted Tokens | (Planned) Job objects and restricted tokens |

These are **local OS sandboxes** - they share the host kernel but enforce policy through kernel-level security primitives.

```
Container < gVisor < MicroVM < Wasm
     ^
     |
VT Code uses kernel-enforced isolation at this level
```

### Syscall Surface

The boundary determines which syscalls reach the host kernel:

- **macOS Seatbelt**: Profile rules filter operations before kernel dispatch
- **Linux Landlock**: Filesystem access rules at kernel level
- **Linux Seccomp**: BPF program filters syscalls by number and arguments

VT Code blocks dangerous syscalls by default:
- `ptrace` - debugger attachment
- `mount/umount` - filesystem namespace changes
- `kexec_load` - kernel replacement
- `bpf` - eBPF program loading
- `perf_event_open` - performance monitoring (info leak risk)

## Policy

### SandboxPolicy Enum

VT Code defines four policy levels:

```rust
enum SandboxPolicy {
    ReadOnly,           // No filesystem writes, no network
    WorkspaceWrite,     // Write within workspace, configurable network
    DangerFullAccess,   // No restrictions (requires approval)
    ExternalSandbox,    // Caller manages isolation
}
```

### Network Egress Allowlist

Following the field guide: "Default-deny outbound network, then allowlist."

```toml
[sandbox.network]
allow_all = false
block_all = false

[[sandbox.network.allowlist]]
domain = "api.github.com"
port = 443

[[sandbox.network.allowlist]]
domain = "*.npmjs.org"
port = 443
```

### Sensitive Path Blocking

Prevents credential leakage by blocking access to:

- `~/.ssh` - SSH keys
- `~/.aws` - AWS credentials
- `~/.kube` - Kubernetes config
- `~/.docker` - Docker auth
- `~/.gnupg` - GPG keys
- `~/.git-credentials` - Git credentials
- And 15+ more sensitive locations

```toml
[sandbox.sensitive_paths]
use_defaults = true
additional = ["~/.custom/secrets"]
exceptions = []
```

### Resource Limits

Prevents fork bombs and resource exhaustion:

```toml
[sandbox.resource_limits]
preset = "moderate"  # or: unlimited, conservative, generous, custom

# Custom limits (when preset = "custom")
max_memory_mb = 2048
max_pids = 256
max_disk_mb = 4096
cpu_time_secs = 300
timeout_secs = 600
```

Presets:
| Preset | Memory | PIDs | CPU Time | Timeout |
|--------|--------|------|----------|---------|
| conservative | 512 MB | 64 | 60s | 120s |
| moderate | 2 GB | 256 | 300s | 600s |
| generous | 8 GB | 1024 | unlimited | 3600s |

## Lifecycle

VT Code uses session-scoped lifecycle:

- **Fresh run**: Default for most operations
- **Session approvals**: Commands approved once remain approved for the session
- **No persistence**: Approvals don't persist across sessions

## Platform-Specific Implementation

### macOS Seatbelt

VT Code generates SBPL profiles dynamically:

```lisp
(version 1)
(deny default)
(allow process-exec)
(allow process-fork)
(allow sysctl-read)
(allow mach-lookup)

; Block sensitive paths FIRST
(deny file-read* (subpath "/Users/alice/.ssh"))
(deny file-read* (subpath "/Users/alice/.aws"))

; Then allow general reads
(allow file-read*)

; Write only to workspace
(allow file-write* (subpath "/path/to/workspace"))

; Network allowlist
(allow network-outbound (remote tcp (require-any (port 443))))
```

### Linux Landlock + Seccomp

VT Code passes policy to a sandbox helper that applies:

1. **Landlock rules** for filesystem access
2. **Seccomp-BPF** for syscall filtering
3. **Resource limits** via cgroups/rlimits

```toml
[sandbox.seccomp]
enabled = true
profile = "strict"  # Blocks ~30 dangerous syscalls
additional_blocked = []
log_only = false
```

## External Sandboxes (Advanced)

For multi-tenant or hostile code scenarios, VT Code supports external sandboxes:

### Docker Containers

```toml
[sandbox.external]
sandbox_type = "docker"

[sandbox.external.docker]
image = "ubuntu:22.04"
memory_limit = "512m"
network_mode = "none"
```

### MicroVMs

For stronger isolation boundaries:

```toml
[sandbox.external]
sandbox_type = "microvm"

[sandbox.external.microvm]
vmm = "firecracker"
memory_mb = 512
vcpus = 1
```

## Decision Matrix

| Scenario | Recommended Boundary | Policy |
|----------|---------------------|--------|
| Trusted internal code | OS sandbox | WorkspaceWrite + moderate limits |
| User-submitted code | OS sandbox + strict seccomp | ReadOnly + conservative limits |
| Multi-tenant SaaS | MicroVM or Docker | WorkspaceWrite + network allowlist |
| Plugin execution | OS sandbox | ReadOnly + no network |

## Configuration Reference

Full configuration in `vtcode.toml`:

```toml
[sandbox]
enabled = false # opt-in; set true to enforce sandboxing
default_mode = "read_only"

[sandbox.network]
allow_all = false
block_all = false

[sandbox.sensitive_paths]
use_defaults = true
additional = []
exceptions = []

[sandbox.resource_limits]
preset = "moderate"

[sandbox.seccomp]
enabled = true
profile = "strict"

[sandbox.external]
sandbox_type = "none"
```

## Security Considerations

### What the Sandbox Does

- Blocks access to credentials and sensitive files
- Limits filesystem writes to workspace
- Controls network egress
- Prevents dangerous syscalls
- Limits resource consumption

### What the Sandbox Does NOT Do

- Protect against kernel exploits (shared kernel boundary)
- Prevent all side-channel attacks
- Guarantee isolation against targeted attacks
- Replace code review or trust decisions

### Threat Model

In scope:
- Prompt injection leading to file access attempts
- Accidental credential exposure
- Resource exhaustion (fork bombs, memory)
- Unintended network access

Out of scope:
- Kernel 0-days
- Hardware side channels
- Physical access

## Related Documentation

- [Security Model](security/SECURITY_MODEL.md) - Overall security architecture
- [Command Allowlist](environment/ALLOWED_COMMANDS_REFERENCE.md) - Permitted commands
- [Process Hardening](development/PROCESS_HARDENING.md) - Pre-main security measures
