# Granular Permissions

VT Code uses explicit permission decisions instead of session permission states. Primary agents and subagents carry
their own local policy, and global `[permissions]` rules can still provide workspace-wide ceilings and prompts.

## Agent Permission Schema

Every VT Code-native agent spec must provide `permissions.default`. Optional rule buckets are evaluated in this order:
`deny`, `ask`, `auto`, `allow`, then `default`.

```yaml
permissions:
  default: ask
  allow:
    - exec_command
    - code_search
  ask:
    - write_stdin
  auto:
    - apply_patch
  deny:
    - rm
```

Allowed default decisions are `ask`, `allow`, `auto`, and `deny`.

`permissions.auto` means classifier-backed review. Matching tool calls are reviewed by VT Code's permission reviewer;
they are not treated as unrestricted execution.

## Global Rules

The top-level `[permissions]` table in `vtcode.toml` defines workspace-wide rule ceilings and prompts:

```toml
[permissions]
ask = ["exec_command(git commit *)", "apply_patch(/docs/**)"]
allow = ["exec_command", "code_search", "mcp__context7__*"]
deny = ["exec_command(rm -rf *)", "apply_patch(/.git/**)"]
```

Global rules do not define a default decision. The active primary agent or active subagent supplies
`permissions.default` after global deny and ask checks.

## Classifier Review Settings

The classifier used by `permissions.auto` can be tuned from the permission review settings:

```toml
[permissions.auto]
model = ""
probe_model = ""
use_decisions_probe = false # Experimental; direct OpenAI API-key tool-output probe only
max_consecutive_denials = 3
max_total_denials = 20
drop_broad_allow_rules = true
block_rules = []
allow_exceptions = []

[permissions.auto.environment]
trusted_paths = []
trusted_domains = []
trusted_git_hosts = []
trusted_git_orgs = []
trusted_services = []
```

After repeated classifier denials, VT Code falls back to manual prompts where an interactive prompt is possible.

The experimental, default-off `use_decisions_probe` option is also available in `/settings` → **Approvals & Security**
for ordinary TUI sessions with either Build or Auto selected. It requires the built-in direct OpenAI API-key provider
at the standard endpoint, but no full-auto flag or acknowledgement profile. It classifies tool output after execution
and queues an advisory warning for `SUSPECT`; manual approvals and explicit deny rules continue to apply before execution.
Subscription access, gateways, custom endpoints, other providers, planning, and headless sessions outside full-auto
make no additional probe requests. The toggle persists across provider changes and settings reloads.

This uses your OpenAI API billing. Decisions gets four seconds and may use one configured generation fallback within
the shared eight-second deadline; absent usage leaves cost unknown. The TUI shows **Checking tool output...** during
both attempts and clears it on cancellation. See the [probe guide](../development/decisions-probe.md) for evidence
bounds, accounting, and experimental limitations.

## Rule Grammar

`allow`, `ask`, `auto`, and `deny` accept exact VT Code tool ids and richer rule grammar:

- `exec_command` or `exec_command(cargo check *)`
- `write_stdin`
- `apply_patch` or `apply_patch(/docs/**)`
- `code_search`
- `WebFetch(domain:example.com)`
- `mcp__context7`, `mcp__context7__*`, `mcp__context7__search-docs`
- Exact VT Code tool ids such as `exec_command` or `apply_patch`

## Path Matching

- `//path` matches an absolute filesystem path.
- `~/path` matches relative to the current user's home directory.
- `/path` matches relative to the workspace root.
- `path` or `./path` matches relative to the current working directory.

## Notes

- Global `deny` is a hard ceiling.
- Global `ask` can force a prompt even when the active agent would allow or auto-review a call.
- Agent-local `deny` wins within that agent's scope.
- `permissions.auto` uses classifier-backed review and does not bypass sandbox escalation prompts.
