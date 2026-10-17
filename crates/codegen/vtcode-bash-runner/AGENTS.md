<!-- Compact maintainer rules retain the repository instruction line budget. -->
<!-- markdownlint-disable MD013 -->
# vtcode-bash-runner

[Root AGENTS.md](../../../AGENTS.md) | Cross-platform command runner with workspace-safe operations.

## Modules

`executor` CommandExecutor trait + backends | `runner` BashRunner | `policy` CommandPolicy + WorkspaceGuardPolicy | `pipe` async process spawning | `process` handles and drain-preserving termination | `process_group` kill/cleanup | `background` long-running tasks | `stream` utilities

## Rules

- `CommandExecutor` trait = primary abstraction for new backends. `CommandPolicy` trait = execution gate. `WorkspaceGuardPolicy` enforces boundaries. Preserve command shape through admission: direct argv executes without shell reconstruction; shell scripts require explicit validated syntax and never fall back from malformed argv. Feature flags: `dry-run`, `pure-rust`, `exec-events`, `serde-errors`. Every type reachable from a `serde-errors` payload must carry the same `cfg_attr` derive — a private helper field (for example `CommandForm` inside `CommandInvocation`) without it breaks `cargo check --all-features` while default-feature builds stay green. `process_group` uses safe `nix` wrappers (`Pid::from_raw`, `signal::killpg`, `setpgid` in `pre_exec`); there is no `unsafe` here. Unsafe env mutation is centralized in `vtcode-commons::env_lock`, serialized by a process-wide mutex.

## Testing

`cargo nextest run -p vtcode-bash-runner` | pipe tests: `cargo nextest run -p vtcode-bash-runner -E 'binary(/pipe_tests/)'` | use `AllowAllPolicy` unless testing policy.

## Gotchas

- `BashRunner::new()` canonicalizes root — bails if missing. Authorization resolves paths freshly; never cache symlink targets across operations. OS sandboxing or bound filesystem handles remain necessary against concurrent replacement. Unsafe env mutation (`set_var`/`remove_var`) is centralized in `vtcode-commons::env_lock`, serialized by a process-wide mutex, single-threaded startup only.
- `policy` containment delegates to `vtcode_commons::paths::ensure_path_within_workspace` — `..`-traversal paths are rejected (intentionally stricter than the old `starts_with`).
- Pipe spooling opts into `SpawnedProcess::reliable_output_rx`, a bounded lossless stream; legacy broadcast subscribers must remain independent of that backpressure path.
- Pipe session termination kills the complete process group before final draining; `wait_with_output` still bounds post-exit draining even when a descendant inherits the pipe, and must never become an unbounded wait.
