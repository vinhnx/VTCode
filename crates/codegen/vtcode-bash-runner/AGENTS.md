# vtcode-bash-runner

[Root AGENTS.md](../../../AGENTS.md) | Cross-platform command runner with workspace-safe operations.

## Modules

`executor` CommandExecutor + backends | `runner` BashRunner | `policy` CommandPolicy, WorkspaceGuardPolicy |
`pipe` spawning | `process` handles, termination | `process_group` kill/cleanup | `process_owner` Unix lifetime helper |
`background` long-running tasks | `stream` utilities

## Rules

- New backends implement `CommandExecutor`; `CommandPolicy` is the execution gate.
- Preserve command shape: direct argv never becomes a shell string; malformed argv never falls back to a shell.
- Containment uses `vtcode_commons::paths::ensure_path_within_workspace`; `..` traversal is rejected.
- Types reachable from a `serde-errors` payload all carry the same `cfg_attr` derive, or `--all-features` breaks.
- Unix process groups use safe `nix` wrappers; Windows FFI stays in `process_group/windows.rs` with RAII handles.
- Env mutation goes through `vtcode-commons::env_lock`, at single-threaded startup only.

## Testing

`cargo nextest run -p vtcode-bash-runner` | pipe tests: `-E 'binary(/pipe_tests/)'` | use `AllowAllPolicy`
unless testing policy.

## Gotchas

- `BashRunner::new()` canonicalizes root and bails if it is missing; never cache symlink targets.
- Termination kills the whole process group before draining; `wait_with_output` stays bounded.
- `has_observed_exit` proves only the direct child exited, not its descendants.
- Treat `ESRCH` as already exited; do not rely on its `io::ErrorKind` mapping.
- `process_owner` closes inherited fds above stdio before exec; fork-child paths stay async-signal-safe.

Details: [maintainer notes](../../../docs/development/maintainer-notes/vtcode-bash-runner.md) — read before changing a
subsystem.
