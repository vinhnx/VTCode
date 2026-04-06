# Unified Permissions

VT Code supports a top-level `[permissions]` table in `vtcode.toml` for authored approval rules.

```toml
[permissions]
default_mode = "default"
ask = ["Bash(git commit *)", "Write(/docs/**)"]
allow = ["read_file", "Read", "Bash(cargo check)", "mcp__context7__*"]
deny = ["unified_exec", "Edit(/.git/**)"]

  [permissions.auto_mode]
  model = ""
  probe_model = ""
  max_consecutive_denials = 3
  max_total_denials = 20
  drop_broad_allow_rules = true
  block_rules = []
  allow_exceptions = []

    [permissions.auto_mode.environment]
    trusted_paths = []
    trusted_domains = []
    trusted_git_hosts = []
    trusted_git_orgs = []
    trusted_services = []
```

## Modes

- `default`: prompt when a matching rule or fallback policy requires it.
- `accept_edits`: auto-allow built-in file mutations for the session.
- `auto`: allow read-only tools and in-workspace edits immediately, then route everything else through the background classifier. After 3 consecutive or 20 total classifier denials, VT Code falls back to manual prompts.
- `plan`: keep plan-mode behavior.
- `dont_ask`: deny anything that is not explicitly allowed.
- `bypass_permissions`: skip prompts except protected writes and sandbox escalation prompts.

Legacy aliases remain accepted for compatibility: `ask`, `suggest`, `auto-approved`, `full-auto`, `trusted_auto`, and `plan`.

## Rule grammar

- `allow` / `ask` / `deny` accept exact VT Code tool ids and richer rule grammar.

- `Bash` or `Bash(cargo test *)`
- `Read` or `Read(/src/**/*.rs)`
- `Edit` or `Edit(/docs/**)`
- `Write` or `Write(./notes/*.md)`
- `WebFetch(domain:example.com)`
- `mcp__context7`, `mcp__context7__*`, `mcp__context7__search-docs`
- Exact VT Code tool ids such as `apply_patch` or `unified_exec`

Rules are evaluated by tier: deny first, then ask, then allow. Exact tool ids feed the deny/allow tiers alongside VT Code's richer authored rules.

## Path matching

- `//path` matches an absolute filesystem path.
- `~/path` matches relative to the current user’s home directory.
- `/path` matches relative to the workspace root.
- `path` or `./path` matches relative to the current working directory.

## Notes

- Matching `[permissions]` rules take precedence over legacy `[tools]`, `[commands]`, and persisted tool-policy fallback behavior.
- In `auto` mode, VT Code filters out broad shell/interpreter allow rules like `Bash(*)` before evaluating authored allow rules so risky commands still reach the classifier.
- In `auto` mode, shell approval history and session approval caches are ignored for classifier-reviewed actions.
- Shell approvals still respect sandbox escalation prompts.
- In `bypass_permissions` mode, writes under `.git`, `.vtcode`, `.vscode`, and `.idea` still prompt, except for `.vtcode/commands`, `.vtcode/agents`, and `.vtcode/skills`.
