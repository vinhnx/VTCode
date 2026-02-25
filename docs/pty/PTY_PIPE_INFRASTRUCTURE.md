# PTY and Pipe Infrastructure

This document describes the process spawning infrastructure in VT Code, inspired by the [codex-rs](https://github.com/openai/codex) PTY utilities.

## Overview

VT Code provides two main process spawning backends:

1. **PTY (Pseudo Terminal)** - For interactive processes that need TTY features
2. **Pipe** - For non-interactive processes using standard pipes

Both backends share a unified `ProcessHandle` interface for consistent interaction.

## Module Structure

```
vtcode-bash-runner/
├── pipe.rs              # Async pipe-based process spawning
├── process.rs           # ProcessHandle and SpawnedProcess types
├── process_group.rs     # Process group management utilities
└── lib.rs               # Re-exports for public API
```

## Usage Examples

### Spawning a Pipe Process

```rust
use vtcode_bash_runner::{spawn_pipe_process, collect_output_until_exit};
use std::collections::HashMap;
use std::path::Path;

async fn run_command() -> anyhow::Result<()> {
    let env: HashMap<String, String> = std::env::vars().collect();
    let spawned = spawn_pipe_process(
        "echo",
        &["hello".to_string()],
        Path::new("."),
        &env,
        &None,
    ).await?;

    // Write to stdin
    let writer = spawned.session.writer_sender();
    writer.send(b"input\n".to_vec()).await?;

    // Collect output until exit
    let (output, code) = collect_output_until_exit(
        spawned.output_rx,
        spawned.exit_rx,
        5_000, // timeout ms
    ).await;

    println!("Exit code: {}, Output: {:?}", code, String::from_utf8_lossy(&output));
    Ok(())
}
```

### Spawning with Options

```rust
use vtcode_bash_runner::{PipeSpawnOptions, PipeStdinMode, spawn_pipe_process_with_options};

async fn run_with_options() -> anyhow::Result<()> {
    let opts = PipeSpawnOptions::new("cat", "/tmp")
        .args(["-n"])
        .stdin_mode(PipeStdinMode::Piped);

    let spawned = spawn_pipe_process_with_options(opts).await?;
    // ... interact with process
    Ok(())
}
```

### Process Handle Interface

The `ProcessHandle` provides a unified interface:

```rust
// Write to stdin
handle.writer_sender().send(b"data".to_vec()).await?;

// Read stdout/stderr (merged)
let mut rx = handle.output_receiver();
while let Ok(chunk) = rx.recv().await {
    // process chunk
}

// Check status
if handle.has_exited() {
    if let Some(code) = handle.exit_code() {
        println!("Exited with code: {}", code);
    }
}

// Terminate process
handle.terminate();
```

## Process Group Management

The `process_group` module provides utilities for reliable process cleanup:

### Kill Signal Types

The `KillSignal` enum provides control over termination behavior:

```rust
use vtcode_bash_runner::KillSignal;

// Interrupt signal (SIGINT / Ctrl+C) - for interactive processes
KillSignal::Int

// Graceful shutdown (SIGTERM) - allows cleanup handlers
KillSignal::Term

// Immediate termination (SIGKILL) - default
KillSignal::Kill
```

### Graceful Termination Result

The `GracefulTerminationResult` enum indicates how termination completed:

```rust
use vtcode_bash_runner::GracefulTerminationResult;

match result {
    GracefulTerminationResult::GracefulExit => {
        // Process exited after SIGTERM/SIGINT
    }
    GracefulTerminationResult::ForcefulKill => {
        // Process required SIGKILL
    }
    GracefulTerminationResult::AlreadyExited => {
        // Process wasn't running
    }
    GracefulTerminationResult::Error => {
        // Failed to check or terminate
    }
}
```

### Functions

| Function                                     | Platform | Description                              |
| -------------------------------------------- | -------- | ---------------------------------------- |
| `set_parent_death_signal(pid)`               | Linux    | Child receives SIGTERM when parent dies  |
| `detach_from_tty()`                          | Unix     | Detach from controlling terminal         |
| `set_process_group()`                        | Unix     | Start own process group                  |
| `kill_process_group(pgid)`                   | Unix     | Kill entire process group (SIGKILL)      |
| `kill_process_group_with_signal(pgid, sig)`  | Unix     | Kill group with specific signal          |
| `kill_process_group_by_pid(pid)`             | Unix     | Resolve PGID and kill group              |
| `kill_process_group_by_pid_with_signal(...)` | Unix     | Resolve PGID and kill with signal        |
| `kill_child_process_group(child)`            | Unix     | Kill group for tokio Child               |
| `kill_child_process_group_with_signal(...)`  | Unix     | Kill child group with signal             |
| `graceful_kill_process_group(pid, sig, dur)` | All      | SIGTERM → wait → SIGKILL pattern         |
| `graceful_kill_process_group_default(pid)`   | All      | Graceful kill with 500ms default timeout |
| `kill_process(pid)`                          | Windows  | Terminate process by PID                 |

### Pre-exec Pattern (Unix)

```rust
use vtcode_bash_runner::process_group;

#[cfg(unix)]
unsafe {
    command.pre_exec(move || {
        process_group::detach_from_tty()?;
        #[cfg(target_os = "linux")]
        process_group::set_parent_death_signal(parent_pid)?;
        Ok(())
    });
}
```

### Unified Graceful Shutdown

The recommended approach uses the unified `graceful_kill_process_group` function:

```rust
use vtcode_bash_runner::{
    graceful_kill_process_group, graceful_kill_process_group_default,
    GracefulTerminationResult, KillSignal, DEFAULT_GRACEFUL_TIMEOUT_MS,
};
use std::time::Duration;

// With default settings (SIGTERM, 500ms timeout)
let result = graceful_kill_process_group_default(pid);

// With custom settings
let result = graceful_kill_process_group(
    pid,
    KillSignal::Int,  // Use SIGINT for interactive processes
    Duration::from_secs(1),  // Custom timeout
);

match result {
    GracefulTerminationResult::GracefulExit => println!("Exited gracefully"),
    GracefulTerminationResult::ForcefulKill => println!("Required SIGKILL"),
    GracefulTerminationResult::AlreadyExited => println!("Was already stopped"),
    GracefulTerminationResult::Error => println!("Failed to terminate"),
}
```

### Platform-Specific Behavior

**Unix (Linux/macOS):**

- Uses `SIGTERM` (or `SIGINT`) followed by `SIGKILL`
- Targets entire process group via `killpg()`
- Properly handles `ESRCH` (no such process)

**Windows:**

- Sends `CTRL_C_EVENT` (for `Int`) or `CTRL_BREAK_EVENT` (for `Term`)
- Falls back to `TerminateProcess` if process doesn't exit

## Security Features

### Environment Filtering

When using sandboxed execution (via `vtcode-core/src/sandboxing/child_spawn.rs`), sensitive environment variables are filtered:

- API keys (OPENAI_API_KEY, ANTHROPIC_API_KEY, etc.)
- Cloud credentials (AWS*\*, AZURE*\_, GOOGLE\_\_)
- Dynamic linker variables (LD_PRELOAD, DYLD_INSERT_LIBRARIES)

### Workspace Isolation

Process working directories are validated against workspace boundaries to prevent escapes.

## Comparison with codex-rs

| Feature               | codex-rs           | VT Code                      |
| --------------------- | ------------------ | ---------------------------- |
| PTY backend           | `portable-pty`     | `portable-pty`               |
| Pipe spawning         | Custom async       | Custom async (adapted)       |
| Process groups        | `process_group.rs` | `process_group.rs` (adapted) |
| Parent death signal   | Linux only         | Linux only                   |
| ConPTY (Windows)      | Custom wrapper     | `portable-pty` native        |
| Environment filtering | Basic              | Extended (Codex patterns)    |

## PTY Session Integration

The PTY system in `vtcode-core/src/tools/pty/` integrates with the process group utilities
for robust process cleanup:

### Graceful Termination

PTY sessions now use a unified graceful termination pattern via `graceful_kill_process_group()`:

1. Send `SIGTERM` to the process group (allows cleanup handlers to run)
2. Wait up to 500ms for graceful shutdown (configurable)
3. Send `SIGKILL` to the process group if still running
4. Returns `GracefulTerminationResult` indicating outcome

```rust
// PtySessionHandle::graceful_terminate() uses the unified helper:
use vtcode_bash_runner::{graceful_kill_process_group, GracefulTerminationResult, KillSignal};

let result = graceful_kill_process_group(
    pid,
    KillSignal::Term,
    Duration::from_millis(500),
);

match result {
    GracefulTerminationResult::GracefulExit => debug!("Exited gracefully"),
    GracefulTerminationResult::ForcefulKill => debug!("Required SIGKILL"),
    _ => {}
}
```

### Process Group Tracking

Each PTY session tracks its child process ID:

```rust
pub(super) struct PtySessionHandle {
    // ...
    pub(super) child_pid: Option<u32>,  // Used for process group operations
    // ...
}
```

### Drop Behavior

When a `PtySessionHandle` is dropped:

1. Writer is closed with an `exit\n` command
2. Process group receives graceful termination via `graceful_kill_process_group()`
    - First: `SIGTERM` to the process group
    - Wait: Up to 500ms for graceful exit
    - Then: `SIGKILL` if still running
3. Reader thread is joined with timeout

This ensures that:

- Child processes have a chance to clean up (handlers, temp files, etc.)
- Grandchild processes are also terminated (entire process group)
- No orphaned processes are left running
- Consistent behavior across Unix and Windows

## Architecture

```
┌─────────────────────────────────────────────────┐
│                   VT Code                        │
├─────────────────────────────────────────────────┤
│  vtcode-core/src/tools/pty/                     │
│  ├── manager.rs       (PtyManager)              │
│  ├── session.rs       (PtySessionHandle)        │
│  │   └── Uses process_group for cleanup         │
│  └── scrollback.rs    (Terminal state)          │
├─────────────────────────────────────────────────┤
│  vtcode-bash-runner/                            │
│  ├── pipe.rs          (spawn_pipe_process)      │
│  ├── process.rs       (ProcessHandle)           │
│  ├── process_group.rs (cleanup utilities)       │
│  │   ├── KillSignal (Int, Term, Kill)           │
│  │   ├── GracefulTerminationResult              │
│  │   └── graceful_kill_process_group()          │
│  └── executor.rs      (CommandExecutor trait)   │
├─────────────────────────────────────────────────┤
│  External: portable-pty                         │
│  (PTY abstraction for all platforms)            │
└─────────────────────────────────────────────────┘
```

## Testing

Run the test suite:

```bash
cargo test --package vtcode-bash-runner

# With output
cargo test --package vtcode-bash-runner -- --nocapture
```

Integration tests in `vtcode-bash-runner/tests/pipe_tests.rs` verify:

- Basic echo commands
- Stdin round-trip
- No-stdin mode
- Process termination
- Session detachment (Unix)
- Stderr capture

## References

- [codex-rs PTY utilities](https://github.com/openai/codex/tree/main/codex-rs/utils/pty)
- [portable-pty documentation](https://docs.rs/portable-pty)
- [Anthropic process sandboxing patterns](https://docs.anthropic.com/en/docs/build-with-claude/computer-use)
