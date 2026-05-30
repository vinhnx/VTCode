# vtcode-bash-runner

[Root AGENTS.md](../AGENTS.md) | Cross-platform command runner with workspace-safe operations.

## Modules

`executor` CommandExecutor trait + backends | `runner` BashRunner | `policy` CommandPolicy + WorkspaceGuardPolicy | `pipe` async process spawning | `process` handles | `process_group` kill/cleanup | `background` long-running tasks | `stream` utilities

## Rules

- `CommandExecutor` trait = primary abstraction for new backends.
- `CommandPolicy` trait = execution gate. `WorkspaceGuardPolicy` enforces boundaries.
- Feature flags: `dry-run`, `pure-rust`, `exec-events`, `serde-errors`.
- `unsafe` in `process_group` must have `// SAFETY:` comments.

## Testing

`cargo nextest run -p vtcode-bash-runner` | pipe tests: `cargo test -p vtcode-bash-runner --test pipe_tests` | use `AllowAllPolicy` unless testing policy.

## Gotchas

- `BashRunner::new()` canonicalizes root — bails if missing.
- `path_cache` is LRU (256) — no fresh canonicalize every call.
- `remove_env_var` is unsafe, single-threaded startup only.
