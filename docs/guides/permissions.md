# Granular Permissions

VT Code uses explicit permission decisions instead of session permission states. Primary agents and subagents carry their own local policy, and global `[permissions]` rules can still provide workspace-wide ceilings and prompts.

## Agent Permission Schema

Every VT Code-native agent spec must provide `permissions.default`. Optional rule buckets are evaluated in this order: `deny`, `ask`, `auto`, `allow`, then `default`.

```yaml
permissions:
  default: ask
  allow:
    - read_file
    - list_files
    - unified_search
  ask:
    - unified_exec
  auto:
    - edit_file
    - write_file
    - apply_patch
  deny:
    - rm
```

Allowed default decisions are `ask`, `allow`, `auto`, and `deny`.

`permissions.auto` means classifier-backed review. Matching tool calls are reviewed by VT Code's permission reviewer; they are not treated as unrestricted execution.

## Global Rules

The top-level `[permissions]` table in `vtcode.toml` defines workspace-wide rule ceilings and prompts:

```toml
[permissions]
ask = ["unified_exec(git commit *)", "write_file(/docs/**)"]
allow = ["read_file", "list_files", "unified_search", "mcp__context7__*"]
deny = ["unified_exec(rm -rf *)", "edit_file(/.git/**)"]
```

Global rules do not define a default decision. The active primary agent or active subagent supplies `permissions.default` after global deny and ask checks.

## Classifier Review Settings

The classifier used by `permissions.auto` can be tuned from the permission review settings:

```toml
[permissions.auto]
model = ""
probe_model = ""
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

## Rule Grammar

`allow`, `ask`, `auto`, and `deny` accept exact VT Code tool ids and richer rule grammar:

- `unified_exec` or `unified_exec(cargo test *)`
- `read_file` or `read_file(/src/**/*.rs)`
- `edit_file` or `edit_file(/docs/**)`
- `write_file` or `write_file(./notes/*.md)`
- `WebFetch(domain:example.com)`
- `mcp__context7`, `mcp__context7__*`, `mcp__context7__search-docs`
- Exact VT Code tool ids such as `apply_patch` or `unified_search`

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
