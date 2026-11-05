<!-- Compact maintainer rules retain the repository instruction line budget. -->
<!-- markdownlint-disable MD013 -->
# vtcode-bash-runner

[Root AGENTS.md](../../../AGENTS.md) | Cross-platform command runner with workspace-safe operations.

## Modules

`executor` CommandExecutor trait + backends | `runner` BashRunner | `policy` CommandPolicy + WorkspaceGuardPolicy | `pipe` async process spawning | `process` handles and drain-preserving termination | `process_group` kill/cleanup | `process_owner` Unix native lifetime helper | `background` long-running tasks | `stream` utilities

## Rules

- `CommandExecutor` trait = primary abstraction for new backends. `CommandPolicy` trait = execution gate. `WorkspaceGuardPolicy` enforces boundaries. Preserve command shape through admission: direct argv executes without shell reconstruction; shell scripts require explicit validated syntax and never fall back from malformed argv. Feature flags: `dry-run`, `pure-rust`, `exec-events`, `serde-errors`. Every type reachable from a `serde-errors` payload must carry the same `cfg_attr` derive — a private helper field (for example `CommandForm` inside `CommandInvocation`) without it breaks `cargo check --all-features` while default-feature builds stay green. Unix `process_group` operations use safe `nix` wrappers (`Pid::from_raw`, `signal::killpg`, `setpgid` in `pre_exec`); Windows FFI is isolated in `process_group/windows.rs` with documented ownership and RAII handles. Unsafe env mutation is centralized in `vtcode-commons::env_lock`, serialized by a process-wide mutex.

## Testing

`cargo nextest run -p vtcode-bash-runner` | pipe tests: `cargo nextest run -p vtcode-bash-runner -E 'binary(/pipe_tests/)'` | use `AllowAllPolicy` unless testing policy.

## Gotchas

- `BashRunner::new()` canonicalizes root — bails if missing. Authorization resolves paths freshly; never cache symlink targets across operations. OS sandboxing or bound filesystem handles remain necessary against concurrent replacement. Unsafe env mutation (`set_var`/`remove_var`) is centralized in `vtcode-commons::env_lock`, serialized by a process-wide mutex, single-threaded startup only. Pure-Rust recursive copy anchors both sides at canonical roots and rejects escaping descendant links; in-tree links keep copy-through behavior.
- PowerShell commands encode untrusted inputs as UTF-8 byte data and use literal path operations. Keep `create_parents` on the invocation so the pure-Rust executor does not infer it from script text.
- `policy` containment delegates to `vtcode_commons::paths::ensure_path_within_workspace` — `..`-traversal paths are rejected (intentionally stricter than the old `starts_with`).
- Pipe spooling opts into `SpawnedProcess::reliable_output_rx`, a bounded lossless stream; legacy broadcast subscribers must remain independent of that backpressure path.
- Pipe session termination kills the complete process group before final draining; `wait_with_output` still bounds post-exit draining even when a descendant inherits the pipe, and must never become an unbounded wait. `has_observed_exit` confirms only a successful direct-child wait, not descendant termination; `has_exited` also covers wait failures.
- Process-group cleanup treats `ESRCH` as already exited directly; do not rely on its `io::ErrorKind` mapping being `NotFound` on every host.
- Windows cleanup queries RAII-owned synchronization handles to distinguish exited processes from permission failures; capture helper output so cleanup cannot corrupt stdio protocols. Keep Windows-specific behavior in `process_group/windows.rs` and cross-check its tests when a Windows host is unavailable.
- Unix ownership wraps already-admitted argv and sandbox launchers; dispatch the native helper before agent environment hardening and threads. Fork-child watchdog paths must remain async-signal-safe and close stdio; only the watchdog signals its own pinned group. Keep command approval outside this helper. The helper closes all inherited fds above stdio before exec since ancestors may hold non-CLOEXEC descriptors; preserve that sweep and its drop-before-exec ordering.
