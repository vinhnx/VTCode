# VT Code Async Architecture Guide

This guide explains VT Code's use of async/await and tokio, based on Ratatui and terminal UI best practices.

## When Should VT Code Use Async?

Based on the **Ratatui FAQ: "When should I use tokio and async/await?"**, VT Code uses async for three key reasons:

### 1. Non-Blocking Event Handling

VT Code's main loop must handle three independent timers and one blocking I/O read without blocking:

```text

 Tick Timer (4 Hz)                    Update app state
 Render Timer (60 FPS)                Redraw UI
 Crossterm Event Read (blocking)      Read terminal input
 Cancellation Token                   Graceful shutdown

```

Without async, polling each source would require sleeping, causing latency. With `tokio::select!`, VT Code reacts to
whichever is ready first.

**File:** `crates/codegen/vtcode-ui/src/tui/core_tui/runner/`

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

```text
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

```text
Synchronous main
 Simple print/exec commands (no event loop)
```

**Used for:**

- One-shot CLI commands (`vtcode ask "prompt"`)
- Automation runs (`-a` flag)
- Tool policy testing

## Key Async Patterns in VT Code

### Pattern 1: Spawned Event Handler

**File:** `crates/codegen/vtcode-ui/src/tui/core_tui/runner/`

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

**File:** `crates/codegen/vtcode-ui/src/tui/core_tui/runner/`

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

**File:** `crates/codegen/vtcode-ui/src/tui/`

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

### Pattern 6a: Bounded coalescing writers for synchronous side channels

Some domain APIs intentionally remain synchronous (`ProgressLedgerSink` is one example), while their callers run on
async paths. Do not perform filesystem writes in those methods. Enqueue the latest snapshot into a bounded queue with
`try_send`; a dedicated writer thread drains the queue and coalesces stale updates. The queue should retain one pending
signal and the newest snapshot, with checkpoint intent merged into that snapshot. This keeps progress updates
best-effort and non-blocking without allowing unbounded memory growth.

If the API can be async, prefer `tokio::fs` for ordinary file operations and `spawn_blocking` for recursive scans,
tree-sitter aggregation, or other synchronous libraries. `code_search` follows this split: independent backend processes
overlap with `tokio::join!`, then filesystem and parser aggregation is isolated in one blocking task.

`tokio::fs` runs blocking syscalls on `spawn_blocking` behind the scenes, so tune for few pool hops (see `tokio::fs`
"Tuning your file IO"): batch into as few calls as possible, prefer whole-file `read`/`write` over chunked `File` loops,
coalesce streaming writes with `BufWriter` + `flush` only when the file is not concurrently read (downloads), use
`std::fs` inside one `spawn_blocking` for multi-step sequences (open + write + flush, checksum + extract), remember
`flush()` before `sync_all()` on async files, and note `File::set_max_buf_size` (default 2 MiB) caps bytes per blocking
call. Never use `tokio::fs` for special files (pipes); use `AsyncFd` or `tokio::net::unix::pipe` instead. Live-read
spool files must stay unbuffered so every chunk reaches disk immediately; buffering would hide output from concurrent
readers until `flush`.

### Pattern 7: Actor Pattern (Handle + Background Task)

The actor pattern separates the handle (what callers interact with) from the background task (which owns state and
performs I/O). This is the recommended pattern when a component needs to own exclusive access to a resource while
accepting messages from multiple callers.

**Core recipe:**

```rust
// --- Actor message enum ---
enum ActorMessage {
    DoWork { data: String, respond_to: oneshot::Sender<Result<()>> },
    Shutdown,
}

// --- Handle: what callers see ---
#[derive(Clone)]
struct MyActorHandle {
    tx: mpsc::Sender<ActorMessage>,
}

impl MyActorHandle {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel(64); // bounded for backpressure
        tokio::spawn(actor_loop(rx));
        Self { tx }
    }

    async fn do_work(&self, data: String) -> Result<()> {
        let (respond_to, response) = oneshot::channel();
        let _ = self.tx.send(ActorMessage::DoWork { data, respond_to }).await;
        response.await.expect("actor task panicked")
    }
}

// --- Background task: owns state ---
async fn actor_loop(mut rx: mpsc::Receiver<ActorMessage>) {
    while let Some(msg) = rx.recv().await {
        match msg {
            ActorMessage::DoWork { data, respond_to } => {
                let result = process(data).await;
                let _ = respond_to.send(result);
            }
            ActorMessage::Shutdown => break,
        }
    }
}
```

**Key principles (from [Actors with Tokio](https://ryhl.io/blog/actors-with-tokio/)):**

1. **Handle is separate from task.** The handle struct holds only a channel sender. The background task owns all mutable
   state. This enforces single-ownership of state at compile time.

2. **Handle is Clone.** Because `mpsc::Sender` is `Clone`, multiple callers can talk to the same actor concurrently
   without locks.

3. **Bounded channels for backpressure.** Use `mpsc::channel(N)` with a reasonable capacity. If the channel is full, the
   caller blocks (or you can use `try_send` for fire-and-forget paths). Never use unbounded channels for data that could
   grow without bound.

4. **`oneshot` for request-response.** When a caller needs a result, include a `oneshot::Sender` in the message. The
   actor sends the result back on that channel. The caller awaits the receiver.

5. **Graceful shutdown via dropped sender.** When all handles are dropped, the channel closes, `rx.recv()` returns
   `None`, and the loop exits. No explicit shutdown signal needed for the common case.

6. **Never `tokio::spawn` inside `Drop`.** Spawning from `Drop` creates fire-and-forget tasks that cannot be awaited and
   are lost during shutdown. Instead, send a message on a channel (unbounded `send` is sync and non-blocking).

7. **Avoid cycles of bounded channels.** If Actor A sends to Actor B and B sends to A, both using bounded channels, a
   deadlock can occur if both channels fill up. Break cycles with `tokio::select!` on a "primary" channel, or use
   `try_send` for the cycle-closing path.

### Pattern 8: Pinning futures (stack first, boxes only at type-erasure boundaries)

`Pin` exists for address-sensitive types — futures whose compiled state holds pointers into themselves across `.await`
points. Pinning is an ordinary type system feature, not compiler magic: `Pin<Ptr>` pins the _pointee_, so a
`Pin<Box<dyn Future>>` handle is itself freely movable (`Unpin`) even though the future it targets is not. VT Code keeps
all pinning inside standard combinators (`Box::pin`, `tokio::pin!`, `async_stream`) and never projects through pins
manually — no `get_unchecked_mut`, `Pin::new_unchecked`, or `PhantomPinned` anywhere in the workspace.

Conventions, in order of preference:

1. **Plain `.await` needs no pinning at all.** Never write `Box::pin(fut).await` — allocation for nothing. Reach for
   pinning only when a future must be _held_ across other awaits (e.g. polled in a `tokio::select!` loop) or stored in a
   struct.
2. **Stack-pin held locals with `tokio::pin!`.** This is allocation-free and keeps the borrow local. The tool-execution
   and LLM-request keepalive loops both follow this shape:

   ```rust
   let generate_future = ctx.provider_client.generate(request);
   tokio::pin!(generate_future);

   loop {
       let cancel_notifier = ctx.ctrl_c_notify.notified();
       tokio::pin!(cancel_notifier);
       tokio::select! {
           res = &mut generate_future => { /* ... */ }
           _ = &mut cancel_notifier => { /* ... */ }
       }
   }
   ```

3. **`Box::pin` only where a `Pin<Box<dyn Future/Stream>>` boundary exists:** type-erased returns from traits,
   `tokio::spawn` payloads, recursive async fns (breaking infinite future size), and match arms that must unify to one
   type. All other uses are wasted allocations.
4. **Do not re-box already-pinned handles.** `LLMStream` is `Pin<Box<dyn Stream + Send>>`, which implements `Stream`
   _and_ `Unpin` — call `stream.next().await` on it directly.
5. **If you must poll by hand** (e.g. a `futures::Sink` impl on an `Unpin` wrapper like `LegacyMessageSink` in
   `vtcode-webmcp`), take `self: Pin<&mut Self>` and get `&mut Self` via `self.get_mut()` — the conditional `Unpin` impl
   makes that safe without `unsafe`. If the inner type is genuinely `!Unpin`, use `self.project()` from `pin-project`
   rather than hand-written unsafe projections.

### Local stdio transport decision

The [stdio MCP/LSP analysis](https://developerlife.com/2026/08/22/to-async-or-not-to-async-rust-mcp-server/) is right
that a standalone 1:1 local pipe does not become better merely by adding an async runtime. VT Code still keeps Tokio at
this boundary because the transport is embedded in a mixed async application that also multiplexes the TUI, provider
streams, concurrent tools, and network MCP connections. We apply the article's reliability rules inside the async
transport instead:

- ACP and Copilot each have one bounded writer channel and one task that owns serialized stdin writes.
- The stdout reader owns response routing; EOF or a reader error wakes every pending call immediately instead of leaving
  callers to wait for a timeout.
- Newline-delimited JSON-RPC frames are capped at 64 MiB; oversized frames are drained and rejected so a malformed child
  cannot desynchronise later frames.
- Request guards remove pending entries on send failure, timeout, cancellation, or response completion, so abandoned
  calls cannot accumulate.
- Stderr is continuously drained, retained per record only up to the provider diagnostic limit, and passed through the
  secret-redacting sanitizer.

Use synchronous threads and standard channels for a genuinely standalone local bridge when it reduces complexity. Do not
introduce a synchronous island into these transports without preserving the same bounded buffering, serialized writes,
response demultiplexing, and deterministic teardown guarantees.

**Real examples in vtcode:**

| Component         | File                                                                                                  | Pattern                                                                                                                                                                 |
| ----------------- | ----------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `StdioTransport`  | `crates/codegen/vtcode-acp/src/transport.rs` and `crates/codegen/vtcode-llm/src/copilot/transport.rs` | Handle sends JSON-RPC via bounded `mpsc`; background tasks handle stdin write, stdout read, and bounded/sanitized stderr diagnostics. Uses `oneshot` for RPC responses. |
| `AsyncLineWriter` | `crates/codegen/vtcode-core/src/utils/async_line_writer.rs`                                           | Cloneable handle sends `LogMessage` via bounded `mpsc`; the actor bounds queued bytes/lines and periodically flushes through `spawn_blocking`.                          |
| `TimeoutDetector` | `crates/codegen/vtcode-core/src/core/timeout_detector.rs`                                             | Global detector with `mpsc::UnboundedSender<String>` cleanup channel; background task processes end-operation requests from dropped `TimeoutHandle`s.                   |
| `ProcessHandle`   | `crates/codegen/vtcode-bash-runner/src/pipe.rs`                                                       | Handle wraps channels for stdin, output broadcast, and exit status; separate writer, reader, and wait tasks.                                                            |

ACP and Copilot share the bounded byte reader in `vtcode-commons::line_framing`,
with explicit LF accounting per adapter. See [stdio line framing](../development/stdio-line-framing.md)
for delimiter, EOF, error, and cancellation contracts.

**When to use the actor pattern vs. simpler alternatives:**

| Scenario                                         | Recommended approach          |
| ------------------------------------------------ | ----------------------------- |
| One-shot background work (e.g. prefetch)         | `tokio::spawn` + `JoinHandle` |
| Shared read-only state                           | `Arc<RwLock<T>>`              |
| Exclusive ownership of a resource                | Actor pattern                 |
| Multiple producers, single consumer event stream | `mpsc` channel (bounded)      |
| Broadcasting to multiple subscribers             | `broadcast` channel           |

### Eager pipeline decision criteria

Use eager stages when they isolate blocking work, make ownership explicit, or allow independent work to overlap. Do not
introduce a generic pipeline merely to replace an already-bounded `buffer_unordered`, `join_all`, or Rayon stage;
benchmark the real workload first.

The channel policy follows the data's correctness contract:

| Data path                                       | Backpressure policy                                                                                                                                                                                                                                                                                                                                     | Shutdown policy                                                                                                                                                                                                                                                        |
| ----------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Authoritative session `ThreadEvent` persistence | One bounded non-blocking handoff (`try_send`) keeps Tokio workers responsive. The queue has event-count and estimated-byte limits; saturation fails closed and reports the persistence failure, while accepted events remain ordered and are not silently dropped. Canonical files live under `<workspace>/.vtcode/sessions/<session_id>/events.jsonl`. | The runner closes and awaits the sink after terminal events; accepted queued events drain, and queue/append/flush failures make the task fail rather than being reported as success. Optional legacy exporters are separate and do not create global files by default. |
| Diagnostic trajectory JSONL                     | Bounded by channel/line count and bytes. Saturation may drop records and increments diagnostics.                                                                                                                                                                                                                                                        | `flush()` remains best-effort for compatibility; the internal result path reports actor or file failures, and actor shutdown performs a final flush.                                                                                                                   |
| Tool/read results                               | Explicit non-zero concurrency, bounded in-flight work, deterministic input-order assembly.                                                                                                                                                                                                                                                              | Caller cancellation drops the in-flight futures; no extra worker pool is introduced without measured benefit.                                                                                                                                                          |

Every spawned stage must have an observable failure path and a clear owner for shutdown. Blocking filesystem work
belongs in `spawn_blocking`; periodic flushes prevent a healthy producer from allowing an actor's internal buffer to
grow indefinitely. For lossless paths, a full queue is a signal to slow the producer. For best-effort paths, drops are
acceptable only when they are bounded and visible through diagnostics or tracing.

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

### Anti-Pattern 4: tokio::spawn in Drop

**Bad:**

```rust
impl Drop for MyHandle {
    fn drop(&mut self) {
        let resource = self.resource.clone();
        tokio::spawn(async move {
            resource.cleanup().await;  // Fire-and-forget, lost on shutdown
        });
    }
}
```

**Good:** Use a queue the owner drains instead (a channel-fed actor works when a runtime is guaranteed; a synchronous
queue drained by async methods works everywhere, see Anti-Pattern 6):

```rust
impl Drop for MyHandle {
    fn drop(&mut self) {
        // Channel send is synchronous and non-blocking
        let _ = self.cleanup_tx.send(self.operation_id.clone());
    }
}
// A background actor task receives these messages and does the async work.
```

**Why?** `tokio::spawn` in `Drop` creates a fire-and-forget task. If the runtime is shutting down, the task is silently
lost. You cannot await its completion, and there is no way to handle errors.

### Anti-Pattern 5: Not Handling Cancellation

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

### Anti-Pattern 6: Ambient `tokio::spawn` in sync constructors and globals

`tokio::spawn` and `spawn_blocking` read a hidden thread-local runtime context and panic off-runtime (std threads, rayon
workers, `Lazy` initializers). A global that spawns an actor in its constructor also binds to whichever runtime touched
it first and goes dead when that runtime drops.

**Good:**

- Sync constructors that must spawn call `Handle::try_current()` first and return an error (or degrade with a warning)
  before any side effect; async constructors take `Handle::current()` and spawn through the explicit handle.
- Globals and `Drop`-driven cleanup avoid tasks: push to a synchronous queue (`Mutex<Vec<_>>`, stamped with the event
  time) that async methods drain (`TimeoutDetector::drain_pending_ends`).

Greppable rule: a `tokio::spawn` inside a non-`async` `fn` needs a reason or a `Handle` parameter.

## Task Extent, Error Propagation, and Cancel-Safety

Rust's async model gives every spawned task three properties that differ from most other async/await languages (see "A
Design Space Exploration of Async/Await", Gray/Krishnamurthi/Crichton, OOPSLA 2026 — dimensions of _extent_,
_propagation_, and _awareness_): tasks have **indefinite extent** (a detached task outlives the scope that spawned it),
**unaware cancellation** (a task is cancelled only by being dropped or aborted — it runs no cleanup except `Drop` of its
own locals), and **never-propagated errors** (an unawaited `JoinHandle` discards panics and aborts silently). VT Code
rules that follow from this:

### Rule 1: Every spawned task has an owner

A `tokio::spawn`/`spawn_blocking` call site must satisfy exactly one of:

1. **Awaited** — the handle is joined before the spawning scope exits.
2. **Guarded** — the handle is stored in a Drop-abort guard or an owned field with a shutdown path (see the shared
   `vtcode_commons::TaskGuard` and its current adopters `BackgroundTaskGuard`, `SignalHandlerGuard`, and
   `ProgressUpdateGuard`, plus the cooperative-cancel `TimeoutWarningGuard` and `ProcessHandle::Drop`).
3. **Documented detached** — the handle is dropped _only_ with a comment stating why detachment is safe: the work is
   bounded, terminated by a token or channel drop, and its outcome is observable (logged or sent over a channel).
   Example: the legacy WebMCP session expiry loop (`crates/codegen/vtcode-webmcp/src/remote_mcp.rs`), the cancel-path
   MCP shutdown in `src/agent/runloop/unified/session_setup/signal.rs`, and the best-effort A2A webhook deliveries
   (`spawn_webhook_delivery` in `crates/codegen/vtcode-a2a/src/server.rs`, bounded by the webhook client timeout/retry
   budget with failures logged).

"Fire-and-forget" without all three properties is a bug: on process exit the task is killed mid-flight (nothing joins
it), and its error is invisible.

### Rule 2: Detached tasks must not die silently

An unawaited `JoinHandle` throws away `JoinError` (panic or abort) and the task's own `Err` results. If a detached task
can fail in a way that matters, spawn a small observer that joins the handle and logs, or have the task send its outcome
over a channel. Example: `orchestration.rs` wraps the timeout-detached persistent-memory finalization task in an
observer that logs the eventual outcome.

### Rule 3: `select!` and `timeout` cancel at every `.await`

`tokio::select!` polls arms and drops the losing futures; `tokio::time::timeout` drops the wrapped future on elapse.
Cancellation happens at each `.await` point inside those futures, so:

- Only put **cancel-safe** futures in `select!` arms (e.g. `recv()` on channels, `cancelled()` on tokens). A future that
  writes state or produces partial output before completing loses that progress when dropped.
- Work inside a `timeout` must be resumable or its cleanup must live **outside** the future: a task-local
  `CancellationToken`, an RAII guard, or explicit teardown after the timeout (see the tool pipeline's
  `terminate_active_exec_sessions` after `timeout` in `src/agent/runloop/unified/tool_pipeline/execution_attempts.rs`).
- Prefer awaiting a bounded cleanup inline over spawning it when the next step is `std::process::exit` — a spawned
  shutdown task never runs (see `signal.rs`, where the double-Ctrl+C path awaits MCP shutdown inline within a 500ms
  bound before exiting).

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

When a key is pressed that triggers a long operation, keep ownership of the spawned task (see Anti-Pattern 2 and the
task-extent rules below):

```rust
Event::Key(key) if key.code == KeyCode::Enter => {
    // Start an async tool execution in the background
    let task = tokio::spawn(async move {
        let result = tool.execute().await;
        // Post result to UI via channel
    });

    // Main loop continues, responding to events.
    // Store `task` (e.g. in a JoinHandle slot or Drop-abort guard) so the
    // task has an owner — do not drop it and forget the outcome.
}
```

## Testing Async Code

VT Code uses `#[tokio::test]` for async tests:

**File:** `crates/codegen/vtcode-core/src/hooks/lifecycle/tests.rs`

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

VT Code builds one multi-threaded runtime in `src/main.rs` and reuses it for both startup-context resolution and the
long-lived agent loop:

```rust
let mut runtime_builder = tokio::runtime::Builder::new_multi_thread();
runtime_builder.enable_all().thread_name("vtcode-rt-worker");
if let Some(workers) = vtcode_commons::runtime_diagnostics::configured_worker_threads() {
    runtime_builder.worker_threads(workers);
}
let runtime = runtime_builder.build().context("failed to build Tokio runtime")?;
```

Worker threads are named `vtcode-rt-worker` so profiles and `spawn_blocking` traces are attributable to VT Code.

**For custom runtime config:**

```rust
#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    // Explicitly set 4 worker threads
}
```

#### Runtime diagnostics and tuning

`vtcode_commons::runtime_diagnostics` exposes stable `tokio::runtime::RuntimeMetrics` counters without requiring the
`tokio_unstable` cfg:

| Variable                 | Default | Effect                                                                                                                                                                                                                                                                  |
| ------------------------ | ------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `VTCODE_RUNTIME_METRICS` | unset   | `1`/`true`/`yes`/`on`/`debug` enables the boot snapshot plus a 60s periodic snapshot at `DEBUG` on target `vtcode.runtime`.                                                                                                                                             |
| `VTCODE_RUNTIME_WORKERS` | unset   | Positive integer worker-thread count for the main runtime. Unset keeps one worker per core. Lower it to reserve cores for non-Tokio background work (see the [fast-Tokio isolation guidance](https://dial9-rs.github.io/blog/principles-for-fast-tokio-applications/)). |
| `VTCODE_STARTUP_TRACE`   | unset   | `1` enables the existing startup trace and implies runtime diagnostics.                                                                                                                                                                                                 |

The snapshot reports `workers`, `alive_tasks`, `global_queue_depth`, and `worker_busy_ms`. In a healthy application the
global (injection) queue stays close to empty; a consistently deep queue means work is being scheduled from outside
runtime workers or local queues are overflowing.

Per-worker local-queue depth, steal/overflow counts, blocking-pool depth, and poll-time/schedule-latency histograms are
gated behind `RUSTFLAGS="--cfg tokio_unstable"` in current Tokio and stay opt-in follow-up work.

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

Tokio's work-stealing scheduler minimizes context switches. Most of VT Code's async operations are I/O-bound (waiting
for network, terminal, file system), so context switching is cheap.

### Latency: select! Fairness

`tokio::select!` picks the first ready future. If multiple futures are ready, it picks in definition order. VT Code
prioritizes shutdown > ticks > renders > events to ensure responsiveness.

## See Also

- [Ratatui FAQ: Async & Tokio](https://ratatui.rs/faq/#when-should-i-use-tokio-and-async--await-)
- [Tokio Tutorial](https://tokio.rs/tokio/tutorial)
- [Tokio Select Documentation](https://tokio.rs/tokio/tutorial/select)
- [Rust Async Book](https://rust-lang.github.io/async-book/)
- `crates/codegen/vtcode-ui/src/tui/` - Event loop implementation
- `src/agent/runloop/unified/tool_pipeline.rs` - Concurrent tool execution
