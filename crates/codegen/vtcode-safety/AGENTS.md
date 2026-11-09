<!-- markdownlint-disable MD013 -->
# vtcode-safety

[Root AGENTS.md](../../../AGENTS.md) | Command safety detection, execution policies, and sandboxing (Layer 1, depends on vtcode-commons).

## Modules

- `command_safety/`: dangerous-command detection, `shell_parser/` (bash AST, dynamic-syntax detection), `rm_approval.rs` (mounted-target `rm` approval), Windows cmdlet/COM/registry DBs.
- `exec_policy/`: policy manager, approval workflows, `parser.rs`, `command_validation/` (workspace containment via commons).
- `sandboxing/`: `SandboxPolicy`, `permissions.rs`, exec environments, Linux `linux.rs` + `linux_seccomp.rs`.
- `audit_log.rs` durable tool-invocation audit sink | `mcp_sandbox.rs` per-MCP-server derived sandbox.

## Rules

- `SandboxPermissions::resolve_request` is the canonical mode/additional/justification validation boundary and `derive_mcp_sandbox_policy` the canonical MCP narrowing boundary, so callers must not duplicate either rule.
- Keep the vtcode-core re-export facades (`command_safety/mod.rs`, `exec_policy/mod.rs`, `sandboxing/mod.rs`) in sync with this crate.
- Parsing must fail closed: malformed or unresolved scripts, `find` expansion, and cache hits in `parse_bash_tree` (which must recompute `has_error`) all reject or require approval rather than pass.
- Keep hard rejection (`command_might_be_dangerous`) separate from inline-code admission (`command_requires_approval`), and let interpreter `-c`/`-e` forms run only under enforceable sandbox policy or explicit approval.
- `shell_parser` must extract nested commands from loops/conditionals, stay quote-aware, and never blind-skip a heredoc opener line, because the suffix after the delimiter is live shell.
- Sandboxed pipe/PTY and MCP stdio launches rebuild allowlisted env vars after overrides, and macOS unenforceable hostname policies and Windows restrictions fail closed.
- Mounted-target `rm` approval reads the shell script text as well as argv, unwraps program runners (`env`, `sudo`, `exec`, ...), and requires approval for every recursive `rm` on hosts without a mount table.
- Zero-token execution-policy rules and trusted amendments never match and are rejected at insertion/load; keep ordered matching and atomic reloads.
- `classify_git_subcommand` is the single decision home for guarded git subcommands, and only destructive modes are preflight-blocked.

## Gotchas

- Linux sandbox code never compiles on macOS, so every `unsafe` block there needs its own `// SAFETY:` comment plus a `#[allow(unsafe_code, reason = ...)]`, and seccomp `syscall_number` mappings must stay in sync with `BLOCKED_SYSCALLS`.
- Windows-only `command_safety` DB builders are never compiled by Linux CI, so cross-check them with `cargo check --target x86_64-pc-windows-msvc`.
- Keep `exec_policy_command_validation` fuzzing and traversal/symlink regressions aligned with containment or validation changes.

Details: [maintainer notes](../../../docs/development/maintainer-notes/vtcode-safety.md) — read the section for the subsystem you change before editing it.
