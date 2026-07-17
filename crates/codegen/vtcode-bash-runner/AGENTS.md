# vtcode-bash-runner

[Root AGENTS.md](../AGENTS.md) | Cross-platform command runner with workspace-safe operations.

## Modules

`executor` CommandExecutor trait + backends | `runner` BashRunner | `policy` CommandPolicy + WorkspaceGuardPolicy | `pipe` async process spawning | `process` handles | `process_group` kill/cleanup | `background` long-running tasks | `stream` utilities

## Rules

- `CommandExecutor` trait = primary abstraction for new backends.
- `CommandPolicy` trait = execution gate. `WorkspaceGuardPolicy` enforces boundaries.
- Feature flags: `dry-run`, `pure-rust`, `exec-events`, `serde-errors`.
- `process_group` uses safe `nix` wrappers (`Pid::from_raw`, `signal::killpg`, `setpgid` in `pre_exec`); there is no `unsafe` here. Unsafe env mutation is centralized in `vtcode-commons::env_lock`, serialized by a process-wide mutex.

## Testing

`cargo nextest run -p vtcode-bash-runner` | pipe tests: `cargo nextest run -p vtcode-bash-runner -E 'binary(/pipe_tests/)'` | use `AllowAllPolicy` unless testing policy.

## Gotchas

- `BashRunner::new()` canonicalizes root — bails if missing.
- `path_cache` is LRU (256) — no fresh canonicalize every call.
- Unsafe env mutation (`set_var`/`remove_var`) is centralized in `vtcode-commons::env_lock`, serialized by a process-wide mutex, single-threaded startup only.
- `policy` containment delegates to `vtcode_commons::paths::ensure_path_within_workspace` — `..`-traversal paths are rejected (intentionally stricter than the old `starts_with`).
