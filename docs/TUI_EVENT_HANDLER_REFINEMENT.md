# Ratatui Terminal & Event Handler Refinement for VT Code

## Overview

This document describes the implementation of a refined **Terminal UI event handler** pattern based on the [Ratatui recipe](https://ratatui.rs/recipes/apps/terminal-and-event-handler/) and adapted for VT Code's architecture.

The new `tui::Tui` struct provides:

-   **Modular terminal management** with proper lifecycle (enter/exit raw mode, alternate screen)
-   **Async event-driven architecture** with configurable tick and frame rates
-   **Graceful shutdown** via cancellation tokens
-   **Extended event support** for keyboard, mouse, resize, focus, and paste events
-   **Drop-safe cleanup** to ensure terminal is restored even on panic

## Key Components

### 1. Event Enum

The `Event` enum in `src/tui.rs` represents all possible terminal events:

```rust
pub enum Event {
    Init,           // TUI started
    Quit,           // Application should exit
    Error,          // Event handler error
    Closed,         // Event channel closed
    Tick,           // Logical tick event (configurable rate)
    Render,         // Render frame event (configurable rate)
    FocusGained,    // Terminal gained focus
    FocusLost,      // Terminal lost focus
    Paste(String),  // Text pasted to terminal
    Key(KeyEvent),  // Keyboard input
    Mouse(MouseEvent), // Mouse input
    Resize(u16, u16), // Terminal resized
}
```

Events are emitted by the background event handler task and received asynchronously.

### 2. Tui Struct

The `Tui` struct manages the terminal lifecycle:

```rust
pub struct Tui {
    pub terminal: ratatui::Terminal<Backend<Stderr>>,
    pub task: JoinHandle<()>,
    pub cancellation_token: CancellationToken,
    pub event_rx: UnboundedReceiver<Event>,
    pub event_tx: UnboundedSender<Event>,
    pub frame_rate: f64,
    pub tick_rate: f64,
    pub mouse: bool,
    pub paste: bool,
}
```

**Key fields:**

-   `terminal`: Ratatui terminal for drawing UI
-   `task`: Background event handler tokio task
-   `cancellation_token`: Graceful shutdown signal
-   `event_rx/event_tx`: Async channel for events
-   Configuration: `frame_rate` (renders/sec), `tick_rate` (ticks/sec), `mouse`, `paste`

### 3. Builder Pattern

The `Tui` struct uses a builder pattern for configuration:

```rust
let mut tui = Tui::new()?
    .tick_rate(4.0)      // 4 ticks per second
    .frame_rate(60.0)    // 60 frames per second
    .mouse(true)         // Enable mouse
    .paste(true);        // Enable bracketed paste
```

### 4. Terminal Lifecycle Methods

#### `enter()`

-   Enables raw mode
-   Enters alternate screen
-   Shows/hides cursor as needed
-   Enables mouse capture (if configured)
-   Enables bracketed paste mode (if configured)
-   **Starts the event handler task**

#### `exit()`

-   **Stops the event handler task** (gracefully with timeout)
-   Disables bracketed paste (if enabled)
-   Disables mouse capture (if enabled)
-   Leaves alternate screen
-   Disables raw mode

#### `start()` / `stop()` / `cancel()`

-   `start()`: Spawns the background event handler task
-   `stop()`: Stops with graceful timeout (aborts after 100ms if needed)
-   `cancel()`: Signals cancellation to the task

### 5. Event Handler Task

The background event handler:

```rust
tokio::select! {
    // Cancellation signal
    _ = cancellation_token.cancelled() => break,

    // Crossterm events (via spawn_blocking)
    result = event_fut => {
        // Process keyboard, mouse, resize, focus, paste events
    }

    // Tick event (at configured tick_rate)
    _ = tick_interval.tick() => {
        // Emit Tick event
    }

    // Render event (at configured frame_rate)
    _ = render_interval.tick() => {
        // Emit Render event
    }
}
```

**Design notes:**

-   Uses `tokio::select!` for concurrent event handling
-   Crossterm event reading runs in `spawn_blocking` to avoid blocking the async runtime
-   Tick and render events are time-based, not IO-based
-   Events are sent over an unbounded MPSC channel

### 6. Deref & DerefMut Implementations

The `Tui` struct implements `Deref` and `DerefMut` to delegate to the inner terminal:

```rust
// This allows:
tui.draw(|f| { /* render */ })?;  // Instead of tui.terminal.draw(...)
tui.hide_cursor()?;                // Instead of tui.terminal.hide_cursor(...)
```

### 7. Drop Implementation

The `Drop` trait ensures cleanup even on panic:

```rust
impl Drop for Tui {
    fn drop(&mut self) {
        let _ = self.exit();  // Attempt cleanup
    }
}
```

## Comparison: Old vs. New

### Old Pattern (interactive_list.rs)

```rust
// Issues:
- Manual TerminalModeGuard for cleanup
- Blocking event::read() in main loop
- No background event task
- Limited event types (only Key/Resize)
- Hard to compose multiple UIs
- Must call event::read() synchronously
```

### New Pattern (src/tui.rs)

```rust
// Improvements:
 Background event handler task
 Configurable tick/frame rates
 Async event loop via .next().await
 All crossterm event types supported
 Composable and reusable
 Graceful shutdown with cancellation tokens
 Deref simplifies terminal access
 Drop-safe cleanup
```

## Usage Example

See `examples/tui_event_handler.rs` for a complete example:

```rust
#[tokio::main]
async fn main() -> Result<()> {
    let mut tui = Tui::new()?
        .tick_rate(4.0)
        .frame_rate(60.0);

    tui.enter()?;  // Start terminal, enter raw mode, spawn event task

    let mut should_quit = false;
    while !should_quit {
        // Draw the UI
        tui.draw(|frame| {
            // Your drawing logic
        })?;

        // Handle events
        if let Some(event) = tui.next().await {
            match event {
                Event::Key(key) => {
                    // Handle keyboard
                }
                Event::Render => {
                    // Render event triggered
                }
                Event::Tick => {
                    // Logical tick
                }
                Event::Resize(w, h) => {
                    // Terminal resized
                }
                _ => {}
            }
        }
    }

    tui.exit()?;  // Gracefully shutdown
    Ok(())
}
```

## Integration with VT Code

### 1. Replace interactive_list.rs

The `Tui` struct can eventually replace the manual `TerminalModeGuard` in `src/interactive_list.rs`:

```rust
// Old way
let mut terminal_guard = TerminalModeGuard::new(title);
terminal_guard.enable_raw_mode()?;
// ... manual cleanup ...

// New way
let mut tui = Tui::new()?;
tui.enter()?;
// ... no manual cleanup needed (Drop handles it) ...
```

### 2. Multi-UI Composition

The event-driven design allows multiple UIs to coexist:

```rust
let mut main_tui = Tui::new()?.frame_rate(30.0);
let mut secondary_tui = Tui::new()?.frame_rate(10.0);

main_tui.enter()?;

// Both can handle events concurrently
loop {
    tokio::select! {
        event = tui.next() => { /* handle */ }
        event = secondary.next() => { /* handle */ }
    }
}
```

### 3. Model-View-Update Pattern

The async event channel enables a classic MVU loop:

```rust
struct App {
    counter: u32,
}

loop {
    // View: Draw
    tui.draw(|f| {
        // Render app.counter
    })?;

    // Update: Handle event
    if let Some(event) = tui.next().await {
        match event {
            Event::Key(key) => app.handle_key(key),
            Event::Tick => app.update(),
            _ => {}
        }
    }
}
```

## Configuration

### Tick Rate vs Frame Rate

-   **Tick Rate** (default 4.0): Logical update events per second

    -   Use for game logic, state updates, animations
    -   Independent of rendering

-   **Frame Rate** (default 60.0): Rendering events per second
    -   Use to throttle draw calls
    -   Useful to avoid excessive redraws on fast terminals

### Example Configurations

```rust
// High-frequency game/animation
Tui::new()?.tick_rate(60.0).frame_rate(144.0)

// Low-frequency background task monitor
Tui::new()?.tick_rate(0.5).frame_rate(1.0)

// Default configuration
Tui::new()?.tick_rate(4.0).frame_rate(60.0)
```

## Error Handling

The event handler emits an `Event::Error` if:

-   Crossterm event reading fails
-   Event channel closes unexpectedly

Applications should handle this gracefully:

```rust
if let Some(event) = tui.next().await {
    match event {
        Event::Error => {
            eprintln!("Terminal error, shutting down");
            break;
        }
        Event::Closed => {
            eprintln!("Event channel closed");
            break;
        }
        _ => {}
    }
}
```

## Platform-Specific Features

### Unix Signal Handling (suspend/resume)

The `suspend()` and `resume()` methods handle SIGTSTP:

```rust
#[cfg(not(windows))]
pub fn suspend(&mut self) -> Result<()> {
    self.exit()?;
    signal_hook::low_level::raise(signal_hook::consts::signal::SIGTSTP)?;
    Ok(())
}
```

Enable this by handling Ctrl+Z in your event loop:

```rust
Event::Key(key) if key.code == KeyCode::Char('z') && key.modifiers.contains(KeyModifiers::CONTROL) => {
    tui.suspend()?;  // Suspend TUI, return to shell
}
```

## Testing

To test the event handler without a full terminal:

```bash
cargo build --example tui_event_handler
cargo run --example tui_event_handler
```

The example demonstrates:

-   Frame rate throttling (60fps)
-   Tick events (4Hz)
-   Keyboard input handling
-   Terminal resize handling
-   Graceful shutdown

## Dependencies

-   `ratatui = "0.29"` - TUI rendering
-   `crossterm = "0.29"` - Terminal control
-   `tokio = "1.48"` - Async runtime
-   `tokio-util = "0.7"` - Cancellation tokens
-   `futures = "0.3"` - Async utilities
-   `signal-hook = "0.3"` - Signal handling (optional, Unix only)
-   `anyhow = "1.0"` - Error handling

## Future Improvements

1. **Event batching**: Group multiple events before sending
2. **Custom event filters**: Allow filtering specific event types
3. **Event recording/playback**: For testing
4. **Mouse-aware rendering**: Auto-hide UI elements on mouse move
5. **Performance profiling**: Measure actual frame rate vs configured

## References

-   [Ratatui Recipe: Terminal & Event Handler](https://ratatui.rs/recipes/apps/terminal-and-event-handler/)
-   [Crossterm Docs](https://docs.rs/crossterm)
-   [Tokio Docs](https://docs.rs/tokio)
-   [VT Code Architecture](./ARCHITECTURE.md)
