# Unified Permissions

VT Code supports a top-level `[permissions]` table in `vtcode.toml` for authored approval rules.

```toml
[permissions]
default_mode = "default"
deny = ["Edit(/.git/**)"]
ask = ["Bash(git commit *)", "Write(/docs/**)"]
allow = ["Read", "Bash(cargo check)", "mcp__context7__*"]
```

## Modes

- `default`: prompt when a matching rule or fallback policy requires it.
- `accept_edits`: auto-allow built-in file mutations for the session.
- `plan`: keep plan-mode behavior.
- `dont_ask`: deny anything that is not explicitly allowed.
- `bypass_permissions`: skip prompts except protected writes and sandbox escalation prompts.

Legacy aliases remain accepted for compatibility: `ask`, `suggest`, `auto-approved`, `full-auto`, and `plan`.

## Rule grammar

- `Bash` or `Bash(cargo test *)`
- `Read` or `Read(/src/**/*.rs)`
- `Edit` or `Edit(/docs/**)`
- `Write` or `Write(./notes/*.md)`
- `WebFetch(domain:example.com)`
- `mcp__context7`, `mcp__context7__*`, `mcp__context7__search-docs`
- Exact VT Code tool ids such as `apply_patch` or `unified_exec`

Rules are evaluated in order: `deny`, then `ask`, then `allow`. The first matching tier wins.

## Path matching

- `//path` matches an absolute filesystem path.
- `~/path` matches relative to the current user’s home directory.
- `/path` matches relative to the workspace root.
- `path` or `./path` matches relative to the current working directory.

## Notes

- Matching `[permissions]` rules take precedence over legacy `[tools]`, `[commands]`, and persisted tool-policy fallback behavior.
- Shell approvals still respect sandbox escalation prompts.
- In `bypass_permissions` mode, writes under `.git`, `.vtcode`, `.vscode`, and `.idea` still prompt, except for `.vtcode/commands`, `.vtcode/agents`, and `.vtcode/skills`.
