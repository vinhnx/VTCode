# vtcode-bash-runner

Cross-platform shell execution helpers extracted from VT Code.

The crate exposes a trait-based executor so downstream applications can swap
the underlying process strategy (system shell, pure-Rust emulation, or dry-run
logging) while reusing higher-level helpers for workspace-safe filesystem
manipulation.

## Modules

- `executor` – Command execution strategies (process, dry-run, pure-rust)
- `runner` – High-level `BashRunner` for workspace-safe operations
- `background` – Background task management
- `pipe` – Async pipe-based process spawning with unified handles
- `process` – Process handle types for PTY and pipe backends
- `process_group` – Process group management for reliable cleanup
- `stream` – Stream utilities for reading output
- `policy` – Command allow/deny policies (`AllowAllPolicy`, `WorkspaceGuardPolicy`)

## Public entrypoints

| Export | Description |
|---|---|
| `CommandExecutor` | Core trait for executing commands |
| `ProcessCommandExecutor` | Default system-shell executor |
| `CommandOutput`, `CommandStatus` | Result types returned by executors |
| `CommandInvocation` | Describes a single command to run |
| `ShellKind`, `CommandCategory` | Shell and command classification enums |
| `BashRunner` | High-level runner wrapping an executor with workspace guards |
| `BackgroundCommandManager`, `BackgroundTaskHandle` | Spawn and manage background tasks |

### Feature-gated exports

| Export | Feature flag |
|---|---|
| `DryRunCommandExecutor` | `dry-run` |
| `PureRustCommandExecutor` | `pure-rust` |
| `EventfulExecutor` | `exec-events` |

## Usage

```rust
use vtcode_bash_runner::{ProcessCommandExecutor, CommandExecutor, CommandInvocation};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let executor = ProcessCommandExecutor::default();
    let invocation = CommandInvocation::new("echo hello");
    let output = executor.execute(&invocation).await?;
    println!("{}", output.stdout);
    Ok(())
}
```

## API reference

<https://docs.rs/vtcode-bash-runner>
