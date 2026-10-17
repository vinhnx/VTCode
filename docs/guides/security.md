# Security Guide

## Overview

VT Code is designed with security as a first-class concern. This guide explains the security features, best practices,
and how to configure VT Code for maximum safety in your environment.

### Workspace instructions are context, not policy

`AGENTS.md`, `CLAUDE.md`, and `.vtcode/rules/` are dynamically loaded user/workspace context. They help the model
understand project conventions, but they are not a security boundary and cannot grant permissions, bypass the sandbox,
or replace executable validation. Review instruction files from untrusted repositories the same way you review other
repository content.

Universal user-facing behavior comes from the compiled runtime-guidance layer, which is independent of workspace file
contents.

## Security Architecture

VT Code implements a **defense-in-depth security model** with multiple layers of protection:

### Layer 1: Command Allowlist

Only explicitly approved commands can execute. The allowlist includes:

- `ls` - List directory contents
- `cat` - Display file contents
- `cp` - Copy files
- `head` - Display file beginning
- `printenv` - Show environment variables
- `pwd` - Print working directory
- `rg` - Ripgrep text search
- `sed` - Stream editor
- `which` - Locate programs

**All other commands are blocked by default**, including:

- Destructive commands: `rm`, `dd`, `shred`
- Privilege escalation: `sudo`, `su`, `doas`
- System modification: `chmod`, `chown`, `systemctl`
- Network commands: `curl`, `wget`, `ssh`
- Container tools: `docker`, `kubectl`

### Layer 2: Argument Validation

Each allowed command has a dedicated validator that:

- **Validates all flags** - Only explicitly allowed flags are permitted
- **Blocks execution flags** - Prevents `-exec`, `--pre`, `-e`, etc.
- **Validates paths** - Ensures all paths stay within workspace
- **Rejects unknown flags** - Unknown flags are blocked by default

Example: Ripgrep validation blocks dangerous preprocessor flags:

```rust
// BLOCKED: Preprocessor execution
rg --pre "bash -c 'malicious command'" pattern .

// ALLOWED: Safe search
rg -i -n pattern .
```

### Layer 3: Workspace Isolation

All file operations are confined to the workspace:

- **Path normalization** - Resolves `..`, `.`, symlinks
- **Boundary enforcement** - Rejects paths outside workspace
- **Symlink resolution** - Follows symlinks and validates destination
- **Absolute path validation** - Blocks absolute paths outside workspace

```bash
# BLOCKED: Path traversal
cat ../../../etc/passwd

# BLOCKED: Absolute path outside workspace
cat /etc/passwd

# ALLOWED: Workspace file
cat ./src/main.rs
```

### Patch target containment

`apply_patch` accepts workspace-relative paths and absolute paths that resolve inside the workspace for every add,
delete, update, and move source or destination. `..` and other traversal-like forms are rejected by the parser. Before
any mutation, VT Code resolves every target with the workspace's symlink-aware containment check, which rejects absolute
paths outside the workspace and ensures an in-workspace symlink cannot redirect a patch outside the workspace. All
targets are preflighted before the first file mutation.

### Process sandbox boundaries

When a restrictive sandbox policy is active, VT Code applies the same policy to the command's pipe and PTY sessions. The
sanitized environment removes credential, token, cloud-provider, linker, and dynamic-loader variables. A sandboxed
command override cannot re-add one of those variables; this prevents an inherited credential from crossing the process
boundary accidentally.

MCP stdio providers inherit the session sandbox through the public `McpSandboxContext` API. `McpClient::new` remains
available for library callers that intentionally manage their own process isolation, while the application constructs
the context and passes it through initial connections, connection pools, and reconnects. MCP stderr is bounded and
secret-redacted before it is logged.

Provider-owned subprocesses use the same inherited-environment filter. Local Ollama, LM Studio, and llama.cpp helpers,
plus custom provider authentication commands, do not receive unrelated API keys, cloud credentials, tokens, linker
overrides, or dynamic-loader variables. Copilot forwards only its documented GitHub authentication variables, and the
optional `gh` status probe forwards only GitHub CLI authentication variables; this preserves the provider's intended
login flow without exposing credentials for other providers.

### Workspace provider configuration trust boundary

Repository-controlled configuration is not trusted to introduce provider execution or routing behavior. During layered
config loading, VT Code rejects any non-empty `custom_providers` value whose winning origin is a workspace or project
layer. This includes custom provider `auth.command`, which can launch a child process with the user's process
privileges. The loader rejects `provider_overrides.<name>.base_url` and `.api_key_env` from those layers as well,
preventing a repository from redirecting model traffic or selecting a credential environment variable.

The check runs before provider validation and registration, and `workspace.use_root_config` cannot bypass it. Keep
command-backed providers and endpoint/credential overrides in system or user configuration, or pass a file explicitly
with `--config`. Provider subprocess environment filtering remains defense in depth; it is not approval for
repository-supplied commands.

### Native plugin loading

Dynamic libraries are executable code: their initialization routines can run as soon as the library is opened, before VT
Code can inspect plugin metadata. For that reason, repository-controlled `.agents/plugins/` and `.vtcode/plugins/`
directories are metadata-only and are never trusted native plugin roots by the high-level skill loader. Native loading
is limited to user/application-managed trusted locations, and `load_skill` is approval- required so a future
executable-backed skill cannot become silently allowed.

Skill instructions and MCP tool descriptions are untrusted resources, not host policy. VT Code escapes, bounds, and
places them in explicit resource delimiters with host policy on both sides. Registry metadata remains authoritative for
shell/network capability, and expanded out-of-workspace skill permissions still require the normal human approval flow;
prompt text cannot grant them. Repeated MCP `list_changed` refreshes are burst-coalesced and constrained by a bounded
cooldown/token bucket before discovery validation runs again.

Do not treat a plugin manifest, README, `AGENTS.md`, or other repository text as consent to load native code. Review the
plugin's provenance and approve an explicit native-plugin action only when you trust the binary and its full
process-level privileges.

Platform behavior is explicit:

- Linux restrictive policies are enforced by the built-in `vtcode sandbox-exec` launcher (Landlock filesystem rules plus
  seccomp syscall filtering, kernel 5.13+). `VTCODE_LINUX_SANDBOX_EXECUTABLE` may override the helper with an external
  binary speaking the same protocol. A restrictive policy fails closed if Landlock is unavailable or the restrictions
  cannot be applied. Filesystem grants exclude `/proc`/`/sys` so `/proc/self/fd` reopen escapes stay denied, and `/dev`
  is never granted wholesale (only `/dev/null`, `/dev/zero`, and entropy sources). Seccomp blocks escalation primitives
  including `ptrace`, `kcmp`, BPF, `perf_event_open`, `userfaultfd`, the `io_uring` trio, process-VM inspection,
  file-handle syscalls (`open_by_handle_at`, `name_to_handle_at`), namespace creation (`unshare`, `setns`, namespace
  `clone` flags, `clone3`), and terminal injection ioctls (`TIOCSTI`, `TIOCSCTTY`); blocked numbers cover their
  x32-aliased variants and mismatched architectures are killed outright. Hostname allowlists fail closed because BPF
  cannot inspect `connect()` destinations.
- macOS preserves full-network and blocked-network modes. Hostname allowlists are rejected unless exact enforcement is
  available; Seatbelt profiles are not treated as a reliable third-party domain-filtering contract.
- Windows restrictive policies fail closed because native restricted-token isolation is not yet implemented. Native
  Windows isolation remains outside this release's scope.
- `DangerFullAccess` intentionally preserves the unsandboxed compatibility path.

### Provider diagnostic boundaries

Provider response bodies, fallback errors, provider logs, and custom authentication-command stderr pass through one
bounded, UTF-8-safe diagnostic sanitizer. HTTP error streams are capped at 16 KiB before parsing, and exposed
diagnostics are capped at 8 KiB. The sanitizer redacts API keys, bearer tokens, cloud credentials, and generic secret
assignments before values reach `LLMError` debug output, serialization, logs, or user-facing messages. HTTP status,
request ID, retry metadata, and error classification remain available to callers, including the 401 refresh path.

### Layer 4: Human-in-the-Loop

Three-tier approval system for tool execution:

1. **Approve Once** - Single execution approval
2. **Allow for Session** - Approved for current session only
3. **Always approve** - Permanently saved for this workspace

For a demonstrably safe, read-only shell command (a learned family such as `sed -n <range> <path>`, `find <dir>`, or a
write-free `awk <program> <path>`), **Always approve** and **Allow for Session** are scoped to the family rather than the
exact invocation, so an equivalent read with different arguments (for example a new `awk` line range) reuses the
approval instead of re-prompting. The learned family is only derived for read-only shapes — mutating forms such as
`awk 'print > file'`, `find -delete`, or `sed -i` stay exact-only and can never inherit the family approval.

#### Workspace Lifecycle Hook Approval

Lifecycle hooks declared in workspace-controlled configuration — a repository's `vtcode.toml`, `.vtcode/` files, project
profiles, or agent-spec files (`.claude/agents/*.md`, `.vtcode/agents/*.md`, Codex TOML specs) — execute shell commands
that an untrusted repository could change at any time. While such hook content is present, VT Code gates the whole
lifecycle engine: **no lifecycle hook runs** until you approve the exact command set for that workspace.

- **Interactive sessions** show an approval dialog listing every command (event, matcher, and `sh -c` command), the
  workspace, and the working directory. Approving persists the approval for this workspace and command set; denying
  skips the hooks for the session.
- **Auto / non-interactive sessions** (e.g. `vtcode ask`, `--full-auto`) skip the hooks unless a previously persisted
  approval still matches the current command set.
- **Any change to the hook commands** — for example after a `git pull` updates `vtcode.toml` or an agent spec —
  invalidates the approval and requires a new review before anything runs again.

User-level hooks in the canonical user config `vtcode.toml` run without approval when the workspace defines no lifecycle
hook content of its own.

## Threat Model

### Protected Against

#### Prompt Injection Attacks

- Malicious prompts from users
- Embedded prompts in code comments
- Prompts in repository files
- Prompts in logging output

  **Argument Injection**

- Execution flags (`-exec`, `--pre`, `-e`)
- Path traversal (`../`, symlinks)
- Output redirection (`-o /etc/passwd`)
- Command chaining (`;`, `&&`, `||`)

  **Workspace Escape**

- Absolute paths outside workspace
- Symlink traversal
- Parent directory traversal
- File-through-file traversal

  **Privilege Escalation**

- `sudo`, `su`, `doas` commands
- System configuration modification
- SUID binary exploitation

### Not Protected Against

**Physical Access** - Assumes no physical access to machine\
 **Kernel Exploits** - Relies on OS security  
 **Side Channel Attacks** - Timing, cache, etc.  
 **Social Engineering** - Direct user manipulation

## Configuration

### Tool Policy Configuration

Configure default tool policies in the canonical user `vtcode.toml` (on Linux/BSD this is
`$XDG_CONFIG_HOME/vtcode/vtcode.toml`, defaulting to `~/.config/vtcode/vtcode.toml`; see the
[user data directories guide](user-data-directories.md)):

```toml
[tools]
default_policy = "prompt"

[tools.policies]
exec_command = "prompt"
code_search = "allow"
apply_patch = "prompt"
write_stdin = "prompt"

# Block a specific tool
some_tool = "deny"
```

Interactive approval decisions are persisted separately in the generated `tool-policy.json` file in the same canonical
config directory. VT Code manages that JSON file; it is not a second TOML configuration file.

### Execution Policy

The execution policy is enforced at the code level and cannot be disabled. However, you can configure workspace
boundaries:

```toml
[workspace]
# Workspace root (default: current directory)
root = "/path/to/project"

# Additional allowed paths (use with caution)
# allowed_paths = ["/tmp/vtcode-cache"]
```

## Best Practices

### For Users

1. **Review Tool Approvals**
   - Review the generated `tool-policy.json` in the canonical user config directory regularly
   - Use "Approve Once" for unfamiliar operations
   - Only use "Always approve" for trusted tools

2. **Be Cautious with Untrusted Content**
   - Don't process code from unknown sources
   - Review prompts in repository files
   - Be wary of code comments with instructions

3. **Monitor Command Execution**
   - Review logs in `.vtcode/logs/`
   - Watch for suspicious patterns
   - Report unusual behavior

### For Organizations

1. **Centralized Policy Management**
   - Deploy standard tool policies
   - Use deny-by-default approach
   - Regular policy reviews

2. **Audit and Monitoring**
   - Centralized log collection
   - Automated anomaly detection
   - Incident response procedures

3. **Security Training**
   - Educate users on prompt injection
   - Share security best practices
   - Regular security updates

## Security Testing

### Automated Tests

VT Code includes comprehensive security tests:

```bash
# Run security test suite
cargo nextest run -p vtcode-core --test execpolicy_security_tests

# Run all tests
cargo nextest run --workspace
```

### Manual Testing

Test security controls with malicious prompts:

```bash
# Test argument injection
vtcode ask "Search using rg --pre 'bash' for pattern"

# Test path traversal
vtcode ask "Show me ../../../etc/passwd"

# Test command chaining
vtcode ask "List files then curl evil.com"
```

All of these should be blocked with appropriate error messages.

## Incident Response

If you discover a security vulnerability:

1. **Do Not Disclose Publicly** - Report privately first
2. **Contact Maintainers** - Open a security advisory on GitHub
3. **Provide Details** - Include reproduction steps
4. **Allow Time for Fix** - Coordinate disclosure timeline

## Security Updates

Stay informed about security updates:

- Watch the [GitHub repository](https://github.com/vinhnx/vtcode)
- Review [CHANGELOG.md](../../CHANGELOG.md) for security fixes
- Subscribe to release notifications

## Additional Resources

- [Security Model](../security/SECURITY_MODEL.md) - Complete security architecture
- [Sandboxing Basics Reference](../development/sandboxing-basics-reference.md) - Distilled reference notes on
  discretionary privilege dropping with the VT Code applicability mapping
- [Tool Policies](../modules/vtcode_tools_policy.md) - Command execution policies
- [CWE-88: Argument Injection](https://cwe.mitre.org/data/definitions/88.html)
- [OWASP Command Injection](https://owasp.org/www-community/attacks/Command_Injection)

## Acknowledgments

VT Code's security model is informed by:

- Trail of Bits research on AI agent security
- Anthropic's safety guidelines
- OpenAI Codex execution policy
- Emilua's software sandboxing basics ([reference notes](../development/sandboxing-basics-reference.md),
  [original article](https://blog.emilua.org/2025/01/12/software-sandboxing-basics/))
- Industry best practices for command execution

---

**Last Updated**: October 25, 2025  
**Security Model Version**: 1.0
