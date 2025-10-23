# `vtcode-bash-runner`

`vtcode-bash-runner` exposes the safe shell helpers that originally lived in
VTCode's TUI runtime as a reusable crate. It focuses on ergonomic filesystem
and search operations (`cd`, `ls`, `mkdir`, `rm`, `cp`, `mv`, `grep`) and wraps
them with workspace-aware safety checks so downstream projects can script
workspaces without accidentally escaping the configured root.

## Core Concepts

- **`BashRunner`** – high-level helper with path resolution, workspace
  validation, and command builders that emit platform-specific shell syntax.
- **`CommandExecutor`** – pluggable backend responsible for executing the
  resolved shell command. The default `ProcessCommandExecutor` relies on
  `std::process`, but consumers can swap in remote or in-memory adapters.
- **`CommandPolicy`** – guard interface that validates invocations before
  execution. The crate includes a permissive `AllowAllPolicy` and a
  workspace-aware `WorkspaceGuardPolicy`.

These building blocks live under `vtcode-bash-runner::runner`,
`vtcode-bash-runner::executor`, and `vtcode-bash-runner::policy`
respectively and are re-exported from the crate root for convenience.

## Shell Selection and Portability

`BashRunner` determines the default `ShellKind` at runtime. Unix targets use
`sh -c` (guarded by the `std-process` feature) while Windows targets emit
PowerShell commands when the `powershell-process` feature is enabled. Each
command helper sanitizes paths and patterns (via
[`shell_escape`](https://docs.rs/shell-escape)) so user-provided strings cannot
break out of quoting contexts.【F:vtcode-bash-runner/src/runner.rs†L1-L229】【F:vtcode-bash-runner/src/runner.rs†L344-L372】

Because execution is delegated through the `CommandExecutor` trait, downstream
crates can support additional shell families or simulation modes without
modifying the runner logic. The crate now ships with built-in executors for
process-backed shells, pure-Rust filesystem shims, dry-run logging, and
execution telemetry forwarding so adopters can pick the strategy that matches
their environment.【F:vtcode-bash-runner/src/executor.rs†L1-L470】

### Feature Flags

The crate exposes a handful of optional features to tailor the dependency
surface and runtime behaviour:

- `std-process` *(default)* – enables the `ProcessCommandExecutor` for
  Unix-like shells.
- `powershell-process` *(default)* – enables PowerShell execution support on
  Windows targets.
- `pure-rust` – activates the `PureRustCommandExecutor`, which handles `ls`,
  `mkdir`, `rm`, `cp`, and `mv` via the standard library without spawning
  external processes.
- `dry-run` – enables the `DryRunCommandExecutor` for log-only validation and
  activates the `dry_run` example.
- `serde-errors` – derives `Serialize`/`Deserialize` for `CommandInvocation`,
  `CommandStatus`, and `CommandOutput` so invocations can be persisted or
  logged as structured payloads.
- `exec-events` – provides the `EventfulExecutor` wrapper that converts command
  invocations into `vtcode-exec-events` telemetry.

Mix and match these features to keep builds lean or to instrument shell
commands for downstream dashboards.

## Policy Hooks

Policies run before every command execution and receive the
`CommandInvocation` with shell family, command string, working directory,
and the set of touched paths. `WorkspaceGuardPolicy` ensures invocations and
paths stay within a configured `WorkspacePaths` root and optionally enforces
an allowlist of permitted command categories.【F:vtcode-bash-runner/src/policy.rs†L1-L73】

This separation lets applications plug in auditing, logging, or approval
workflows while retaining the same runner API.

## Dry-Run and Testing Example

With the `dry-run` feature enabled the crate provides a ready-made
`DryRunCommandExecutor` that records invocations and emits synthetic output for
directory listings. The accompanying example shows how to plug the executor
into `BashRunner` so CI environments can validate command construction without
shell access.

```shell
cargo run -p vtcode-bash-runner --example dry_run
```

The example uses `tempfile` to create an isolated workspace and prints the
captured invocations and synthetic output.【F:vtcode-bash-runner/examples/dry_run.rs†L1-L55】

## Pure-Rust Execution

Enable the `pure-rust` feature to avoid spawning shell processes altogether.
The `PureRustCommandExecutor` implements core filesystem operations with the
standard library, respecting workspace boundaries enforced by the runner. When
combined with custom policies this mode is ideal for serverless or sandboxed
deployments that restrict shell access.【F:vtcode-bash-runner/src/executor.rs†L204-L356】

## Telemetry Integration

Downstream platforms can enable the `exec-events` feature to wrap any executor
with the `EventfulExecutor`. The wrapper emits `ThreadEvent` updates from the
`vtcode-exec-events` crate before and after each command, including the
aggregated output and exit code, so command activity can be replayed in
dashboards or persisted alongside other execution telemetry.【F:vtcode-bash-runner/src/executor.rs†L358-L470】

## Next Steps

- Expand the command surface with additional helpers (`stat`, `find`, `run`)
  before publishing a 1.0 release.
- Provide remote/RPC executor adapters for distributed runners.
- Backfill integration docs illustrating how VTCode's TUI wires
  `WorkspaceGuardPolicy` with the shared `WorkspacePaths` trait for
  application-level security.
