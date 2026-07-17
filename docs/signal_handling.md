# Signal Handling Architecture

This document describes the signal handling architecture in vtcode, with emphasis on the priority guarantees for Ctrl+C (SIGINT) and /exit commands.

## Priority Guarantees

**Ctrl+C and /exit are always the highest priority and cannot be blocked by any other process.** This is a critical user safety guarantee - users must always be able to exit the program without being trapped in an unresponsive state.

### Ctrl+C Priority Guarantees

1. **First Ctrl+C**: Immediately cancels the current operation (Cancel signal)
2. **Second Ctrl+C (within 1 second)**: Immediately exits the program (Exit signal)
3. **Emergency exit path**: On double Ctrl+C, the program calls `std::process::exit(130)` which bypasses all other operations and cleanup routines
4. **No signal masking**: SIGINT is never blocked or masked anywhere in the codebase
5. **Atomic operations**: All state transitions use lock-free atomic operations, ensuring no mutex contention can block signal handling

### /exit Command Priority Guarantees

1. **Immediate processing**: `/exit`, `/quit`, `exit`, and `quit` are processed immediately and cannot be blocked by any other operation
2. **No waiting**: These commands return `InteractionOutcome::Exit { reason: SessionEndReason::Exit }` which is checked at the top of every interaction loop iteration
3. **No cleanup delays**: The exit path skips non-essential cleanup operations to ensure immediate termination

## Signal Handling Components

### 1. CtrlCState State Machine

**Location:** `src/agent/runloop/unified/state.rs`

The `CtrlCState` struct implements a lock-free state machine using atomic operations. It tracks the current phase of Ctrl+C handling and ensures that signals are always processed immediately.

#### State Machine Phases

- **Idle**: Initial state, no signals received
- **CancelRequested**: First Ctrl+C received, current operation should be cancelled
- **ExitArmed**: Cancel has been handled, ready for exit escalation
- **ExitRequested**: Second Ctrl+C received, program should exit

#### State Transitions

```
Idle → CancelRequested (first Ctrl+C)
CancelRequested → ExitArmed (after cancel is handled)
CancelRequested → ExitRequested (second Ctrl+C within 1s)
ExitArmed → ExitRequested (second Ctrl+C within 1s)
ExitRequested → ExitRequested (any subsequent Ctrl+C)
```

#### Key Properties

- **Lock-free**: Uses `AtomicU8` and `AtomicU64` for state and timestamps
- **Debounce**: 200ms debounce window prevents accidental rapid escalation
- **Window**: 1-second window for double Ctrl+C escalation
- **Priority**: Exit signals are always processed, never blocked

### 2. Signal Handler Task

**Location:** `src/agent/runloop/unified/session_setup/signal.rs`

The signal handler is a Tokio task that listens for SIGINT and SIGTERM signals. It runs on its own task and cannot be blocked by other operations.

#### Signal Flow

1. OS delivers SIGINT → Tokio runtime wakes up the signal handler task
2. Signal handler calls `request_local_stop()` → `CtrlCState::register_signal()`
3. If `CtrlCSignal::Exit` is returned → fire-and-forget MCP shutdown (500ms timeout)
4. Call `emergency_terminal_cleanup()` → `restore_tui()` → `std::process::exit(130)`

#### Emergency Exit Path

When a double Ctrl+C is detected, the signal handler:

1. Fire-and-forget MCP shutdown with a 500ms timeout
2. Call `emergency_terminal_cleanup()` which:
   - Disables terminal focus tracking
   - Restores the TUI to its original state
   - Flushes trace logs
   - Calls `std::process::exit(130)` to immediately terminate the process

The `std::process::exit(130)` call bypasses:
- Rust's drop logic
- Async runtime shutdown
- Any pending background tasks
- Any blocking operations

This ensures the program exits immediately, even if other tasks are blocked.

### 3. SIGTERM Emergency Handler

**Location:** `crates/codegen/vtcode-ui/src/tui/core_tui/runner/signal.rs`

A dedicated thread handles SIGTERM as an emergency fallback. This is necessary because the process may not have a running Tokio reactor to observe SIGTERM through the async path.

#### Design Note

SIGINT is deliberately NOT handled in this thread because:
- The TUI runs in raw mode where Ctrl+C is delivered as a key event, not a Unix signal
- The async signal handler in `session_setup/signal.rs` owns the Ctrl+C state machine
- Handling SIGINT in both places caused a "split-brain race" where the thread would call `restore_tui()` + `process::exit(130)` while the async handler hadn't finished shutting down

### 4. Exit Command Handling

**Location:** `src/agent/runloop/unified/turn/session/slash_command_handler.rs`

Exit commands are recognized and processed immediately:

- `exit` / `quit` (bare input)
- `/exit` / `/quit` (slash commands)

These commands return `InteractionOutcome::Exit { reason: SessionEndReason::Exit }` which is checked at the top of every interaction loop iteration.

#### Priority Guarantee

Exit commands cannot be blocked by:
- LLM streaming operations
- Tool execution
- MCP operations
- Any other background task

The exit path is checked before any other operation in the interaction loop.

## Interaction Loop Exit Check

**Location:** `src/agent/runloop/unified/turn/session/interaction_loop_runner.rs`

The interaction loop checks for exit requests at the top of every iteration:

```rust
if ctx.ctrl_c_state.is_exit_requested() {
    return Ok(InteractionOutcome::Exit {
        reason: SessionEndReason::Exit,
    });
}
```

This ensures that:
1. Exit requests are always processed
2. No long-running operation can prevent exit
3. The program can always be terminated by the user

## Process Group Management

**Location:** `crates/codegen/vtcode-bash-runner/src/process_group.rs`

Child processes are managed using process groups to ensure clean termination:

- Child processes are spawned in their own process groups (`process_group(0)` / `setpgid`)
- On Linux, `PR_SET_PDEATHSIG` ensures children receive SIGTERM when the parent exits
- `graceful_kill_process_group()` implements staged termination (SIGTERM → SIGKILL)

## Emergency Terminal Cleanup

**Location:** `crates/codegen/vtcode-ui/src/tui/panic_hook.rs`

The `restore_tui()` function ensures the terminal is left in a usable state even on emergency exit:

1. Drains pending crossterm events
2. Clears the current line
3. Leaves alternate screen
4. Disables bracketed paste, focus change, and mouse capture
5. Pops keyboard enhancement flags

This is called from both:
- The SIGTERM emergency handler
- The Ctrl+C exit path

## Testing Signal Handling

**Location:** `src/agent/runloop/unified/state.rs` (tests module)

The `CtrlCState` state machine is thoroughly tested to verify:

1. **Escalation within window**: Double Ctrl+C within 1 second triggers exit
2. **Reset clears state**: `reset()` clears all state back to idle
3. **Mark cancel handled**: Transitions to ExitArmed phase
4. **Immediate exit after cancel handled**: Second Ctrl+C exits immediately
5. **Priority guarantee**: Double Ctrl+C always exits
6. **Debounce prevents accidental escalation**: Rapid Ctrl+C doesn't immediately exit
7. **Exit is always processed**: Once in exit state, all subsequent signals return exit
8. **Check cancellation returns error**: Returns error on cancel or exit
9. **Window expires after one second**: After 1 second, second Ctrl+C cancels again
10. **Reset clears all state**: Complete reset to idle state
11. **Atomic operations are thread-safe**: Concurrent access works correctly

## Design Principles

1. **User Safety First**: Users must always be able to exit the program
2. **No Blocking**: Signal handling never blocks on mutexes or other synchronization
3. **Atomic Operations**: State transitions use lock-free atomic operations
4. **Emergency Exit**: Double Ctrl+C bypasses all other operations
5. **Clean Terminal State**: Terminal is always restored to a usable state
6. **Process Group Management**: Child processes are properly terminated

## Common Pitfalls

1. **Never use `blocking_write()` in signal-critical paths**: This can block a tokio worker thread
2. **Never mask SIGINT**: Always allow the OS to deliver signals to the handler
3. **Never hold locks across signal checks**: This can cause deadlocks
4. **Always check exit at the top of loops**: Ensure exit requests are processed promptly
5. **Use `std::process::exit()` for emergency exit**: This bypasses all other operations

## Future Improvements

1. **Add signal handling metrics**: Track signal handling latency
2. **Add signal handling stress tests**: Test under high load
3. **Add signal handling documentation**: More detailed documentation for developers
4. **Add signal handling monitoring**: Real-time monitoring of signal handling
