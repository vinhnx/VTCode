# VT Code Async Architecture Guide

This guide explains VT Code's use of async/await and tokio, based on Ratatui and terminal UI best practices.

## When Should VT Code Use Async?

Based on the **Ratatui FAQ: "When should I use tokio and async/await?"**, VT Code uses async for three key reasons:

### 1. Non-Blocking Event Handling

VT Code's main loop must handle three independent timers and one blocking I/O read without blocking:

```

 Tick Timer (4 Hz)                    Update app state
 Render Timer (60 FPS)                Redraw UI
 Crossterm Event Read (blocking)      Read terminal input
 Cancellation Token                   Graceful shutdown

```

Without async, polling each source would require sleeping, causing latency. With `tokio::select!`, VT Code reacts to whichever is ready first.

**File:** `src/tui.rs:164-179`

```rust
tokio::select! {
    _ = _cancellation_token.cancelled() => {
        break;  // Exit immediately on cancel
    }
    _ = tick_interval.tick() => {
        let _ = _event_tx.send(Event::Tick);
    }
    _ = render_interval.tick() => {
        let _ = _event_tx.send(Event::Render);
    }
    result = tokio::task::spawn_blocking(|| {
        crossterm::event::read()  // Blocking read
    }) => {
        // Process crossterm event
    }
}
```

### 2. Concurrent Tool Execution

VT Code spawns multiple async tasks for MCP tools, PTY commands, and LLM requests:

```rust
// Multiple tools run concurrently
let results = tokio::join!(
    tool1.execute(),
    tool2.execute(),
    llm.stream(),
);
```

**Without async:** VT Code would block on the first tool, then the second. Response time = Tool1 + Tool2 + LLM.

**With async:** Tools execute in parallel. Response time ≈ max(Tool1, Tool2, LLM).

### 3. Streaming API Responses

VT Code streams LLM responses token-by-token without blocking:

```rust
let mut stream = llm.stream(&prompt).await?;
while let Some(token) = stream.next().await {
    // Receive token, update UI immediately
    // UI remains responsive during streaming
}
```

## Architecture: Async vs. Synchronous Paths

VT Code's event loop uses two modes:

### Mode 1: Single-Threaded Async (Recommended)

```
Main tokio runtime
 Event handler task (spawned)
   Tick interval (async)
   Render interval (async)
   Crossterm read (spawned_blocking)
   Event dispatch (mpsc channel)
 Agent loop (async)
   Tool execution (concurrent tasks)
   LLM streaming (async)
   State updates (tokio::sync::Mutex)
 Lifecycle hooks (spawned async)
    Shell commands (tokio::process::Command)
```

**Used for:**
- Interactive chat mode
- ACP protocol integration
- Streaming responses

### Mode 2: Synchronous (Fallback)

```
Synchronous main
 Simple print/exec commands (no event loop)
```

**Used for:**
- One-shot CLI commands (`vtcode ask "prompt"`)
- Automation runs (`-a` flag)
- Tool policy testing

## Key Async Patterns in VT Code

### Pattern 1: Spawned Event Handler

**File:** `src/tui.rs:118-182`

```rust
fn start(&mut self) {
    self.task = tokio::spawn(async move {
        // This task runs on the tokio runtime
        loop {
            tokio::select! {
                // Three independent sources
            }
        }
    });
}
```

**Why spawn separately?**
- The event handler runs independently
- It can be stopped/restarted without blocking the main loop
- Main loop can continue processing events from the channel

### Pattern 2: Blocking I/O in Async Context

**File:** `src/tui.rs:162`

```rust
let event_fut = tokio::task::spawn_blocking(|| {
    crossterm::event::read()  // Blocks!
});
```

**Why `spawn_blocking`?**
- `crossterm::event::read()` is a blocking syscall
- Calling it directly in async code would block the entire runtime
- `spawn_blocking` runs it in a thread pool (non-blocking to tokio)

### Pattern 3: Multiple Concurrent Operations

**File:** `src/agent/runloop/unified/tool_pipeline.rs:6-9`

```rust
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio::time;
use tokio_util::sync::CancellationToken;

// Tools execute concurrently
let tool_tasks: Vec<JoinHandle<_>> = tools
    .iter()
    .map(|tool| {
        tokio::spawn(async move {
            tool.execute().await
        })
    })
    .collect();

// Wait for all to finish
let results = tokio::join_all(tool_tasks).await;
```

### Pattern 4: Graceful Shutdown with CancellationToken

**File:** `src/tui.rs:233-236`

```rust
pub fn cancel(&self) {
    self.cancellation_token.cancel();
}
```

**In event loop:**
```rust
tokio::select! {
    _ = _cancellation_token.cancelled() => {
        break;  // Exit immediately
    }
    // ... other branches
}
```

**Why CancellationToken?**
- Allows graceful shutdown of spawned tasks
- Tasks check for cancellation and clean up
- No need for forceful `abort()`

### Pattern 5: Sync primitives for Shared State

**File:** `src/agent/runloop/unified/progress.rs:6`

```rust
use tokio::sync::Mutex;
use tokio::sync::RwLock;

// Multiple tasks share state safely
let state = Arc::new(Mutex::new(AppState::new()));

let state1 = state.clone();
tokio::spawn(async move {
    let mut guard = state1.lock().await;
    guard.update();
});

let state2 = state.clone();
tokio::spawn(async move {
    let guard = state2.lock().await;
    println!("{}", guard);
});
```

**Note:** Use `tokio::sync::*`, not `std::sync::*` for async code.

## Anti-Patterns to Avoid

### Anti-Pattern 1: Mixing Blocking I/O with Async

  **Bad:**
```rust
async fn handle_event(key: KeyEvent) {
    let result = std::fs::read("file.txt");  // Blocks the runtime!
    process(result).await;
}
```

  **Good:**
```rust
async fn handle_event(key: KeyEvent) {
    let result = tokio::fs::read("file.txt").await;  // Async read
    process(result).await;
}
```

### Anti-Pattern 2: Spawning Tasks Without Tracking

  **Bad:**
```rust
tokio::spawn(async {
    expensive_operation().await;
    // Task runs in background, result lost
});
```

  **Good:**
```rust
let handle = tokio::spawn(async {
    expensive_operation().await
});

// Later: wait for result
let result = handle.await?;
```

### Anti-Pattern 3: std::sync Locks in Async Code

  **Bad:**
```rust
let state = Arc::new(Mutex::new(data));

tokio::spawn(async move {
    let guard = state.lock().unwrap();  // Can deadlock!
    // ... if another task holds the lock
});
```

  **Good:**
```rust
let state = Arc::new(tokio::sync::Mutex::new(data));

tokio::spawn(async move {
    let guard = state.lock().await;  // Async lock
    // ... safe, can yield
});
```

### Anti-Pattern 4: Not Handling Cancellation

  **Bad:**
```rust
tokio::spawn(async {
    loop {
        process().await;
        // Never checks for cancellation!
    }
});
```

  **Good:**
```rust
let cancel_token = CancellationToken::new();
let cancel_clone = cancel_token.clone();

tokio::spawn(async move {
    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => break,
            result = process() => handle(result),
        }
    }
});

// Later:
cancel_clone.cancel();  // Gracefully stop the task
```

## Integration with Event Loop

### The Main Event Loop (Async)

VT Code's main loop (in chat mode) typically looks like:

```rust
#[tokio::main]
async fn main() {
    let mut tui = Tui::new()?;
    tui.enter()?;  // Start event handler task
    
    loop {
        match tui.next().await {
            Some(Event::Key(key)) => {
                // Handle key (may spawn async tasks)
            }
            Some(Event::Render) => {
                // Redraw UI
            }
            Some(Event::Tick) => {
                // Update state (may await)
            }
            Some(Event::Quit) => break,
            _ => {}
        }
    }
    
    tui.exit()?;  // Stop event handler task
}
```

### Spawning Long-Running Operations

When a key is pressed that triggers a long operation:

```rust
Event::Key(key) if key.code == KeyCode::Enter => {
    // Start an async tool execution in the background
    let task = tokio::spawn(async move {
        let result = tool.execute().await;
        // Post result to UI via channel
    });
    
    // Main loop continues, responding to events
    // Task runs concurrently
}
```

## Testing Async Code

VT Code uses `#[tokio::test]` for async tests:

**File:** `src/hooks/lifecycle.rs:54`

```rust
#[tokio::test]
async fn test_lifecycle_hook_execution() {
    let config = load_test_config();
    let result = execute_hook(&config, event).await;
    assert!(result.is_ok());
}
```

**Run async tests:**
```bash
cargo test --lib  # Runs all #[tokio::test] tests
```

## Configuration

### Tokio Runtime

VT Code uses the default tokio runtime (work-stealing scheduler, multiple OS threads):

```rust
#[tokio::main]
async fn main() {
    // Default: multi-threaded runtime
    // All async code runs here
}
```

**For custom runtime config:**
```rust
#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    // Explicitly set 4 worker threads
}
```

### Timeouts

VT Code uses `tokio::time::timeout` for long operations:

**File:** `src/agent/runloop/unified/async_mcp_manager.rs:5`

```rust
use tokio::time::{Duration, timeout};

let result = timeout(
    Duration::from_secs(30),
    tool.execute()
).await;

match result {
    Ok(Ok(output)) => {},  // Completed in time
    Ok(Err(e)) => {},      // Tool error
    Err(_) => {},          // Timeout!
}
```

## Performance Considerations

### Memory: Task Overhead

Each spawned task allocates ~64 bytes. VT Code typically spawns:
- 1 event handler task
- N tool execution tasks (concurrent)
- Lifecycle hook tasks (per event)

For typical usage (5-10 concurrent operations), memory overhead is negligible.

### CPU: Context Switching

Tokio's work-stealing scheduler minimizes context switches. Most of VT Code's async operations are I/O-bound (waiting for network, terminal, file system), so context switching is cheap.

### Latency: select! Fairness

`tokio::select!` picks the first ready future. If multiple futures are ready, it picks in definition order. VT Code prioritizes shutdown > ticks > renders > events to ensure responsiveness.

## See Also

- [Ratatui FAQ: Async & Tokio](https://ratatui.rs/faq/#when-should-i-use-tokio-and-async--await-)
- [Tokio Tutorial](https://tokio.rs/tokio/tutorial)
- [Tokio Select Documentation](https://tokio.rs/tokio/tutorial/select)
- [Rust Async Book](https://rust-lang.github.io/async-book/)
- `src/tui.rs` - Event loop implementation
- `src/agent/runloop/unified/tool_pipeline.rs` - Concurrent tool execution
