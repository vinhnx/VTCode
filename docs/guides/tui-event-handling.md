# VT Code TUI Event Handling Guide

This guide documents best practices for terminal UI event handling in VT Code, derived from Ratatui and crossterm standards.

## Overview

VT Code uses a modular event-driven architecture with async/await and `tokio::select!`. The pattern is:

```

  crossterm        Raw terminal input
  event::read()  

         
         

  src/tui.rs                               Event filtering & multiplexing
  Tui::start() spawns task               
  tokio::select! on 3 channels:          
  - tick_interval                        
  - render_interval                      
  - crossterm::event::read()             

         
         

  Event enum                               Application event abstraction
  Key | Mouse | Resize | Paste | etc.    

         
         

  event_rx: UnboundedReceiver<Event>       Async channel to main loop

```

## Key Implementation Details

### 1. Platform-Specific Event Filtering

VT Code filters key events to avoid duplicates on Windows:

**File:** `src/tui.rs:152-155`
```rust
CrosstermEvent::Key(key) => {
    if key.kind == KeyEventKind::Press {
        let _ = _event_tx.send(Event::Key(key));
    }
}
```

**Why:** Windows emits `KeyEventKind::Press` and `KeyEventKind::Release` for every keypress, while macOS/Linux emit only `Press`.

### 2. Async Event Loop with tokio::select!

VT Code multiplexes three independent timers and one blocking read:

**File:** `src/tui.rs:118-182`

```rust
fn start(&mut self) {
    self.task = tokio::spawn(async move {
        let mut tick_interval = tokio::time::interval(tick_delay);
        let mut render_interval = tokio::time::interval(render_delay);

        loop {
            tokio::select! {
                _ = _cancellation_token.cancelled() => break,
                _ = tick_interval.tick() => {
                    let _ = _event_tx.send(Event::Tick);
                }
                _ = render_interval.tick() => {
                    let _ = _event_tx.send(Event::Render);
                }
                result = tokio::task::spawn_blocking(|| {
                    crossterm::event::read()
                }) => {
                    // Handle result...
                }
            }
        }
    });
}
```

**Rationale:**
- `tokio::time::interval()` is non-blocking (no busy-wait)
- `tokio::task::spawn_blocking()` prevents the blocking `crossterm::event::read()` from blocking the async runtime
- `tokio::select!` ensures the first ready future wins (fair scheduling)

### 3. Graceful Shutdown with CancellationToken

**File:** `src/tui.rs:233-236`

```rust
pub fn cancel(&self) {
    self.cancellation_token.cancel();
}
```

**Usage in `exit()`:**
```rust
pub fn exit(&mut self) -> Result<()> {
    self.stop()?;  // Cancels the token, waits for task to finish
    // Clean up terminal state...
}
```

### 4. External App Suspension Pattern

When launching external editors/git clients, VT Code suspends the TUI:

**File:** `src/tui.rs:303-357`

```rust
pub async fn with_suspended_tui<F, T>(&mut self, f: F) -> Result<T>
where
    F: FnOnce() -> Result<T>,
{
    // 1. Stop event handler
    self.stop()?;
    
    // 2. Leave alternate screen
    crossterm::execute!(std::io::stderr(), LeaveAlternateScreen)?;
    
    // 3. CRITICAL: Drain pending events
    while crossterm::event::poll(Duration::from_millis(0)).unwrap_or(false) {
        let _ = crossterm::event::read();
    }
    
    // 4. Disable raw mode
    crossterm::terminal::disable_raw_mode()?;
    
    // 5. Run external app
    let result = f();
    
    // 6-9. Restore terminal state
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(std::io::stderr(), EnterAlternateScreen)?;
    crossterm::execute!(std::io::stderr(), crossterm::terminal::Clear(All))?;
    self.start();
    
    result
}
```

**Critical step:** Draining pending events prevents garbage input (terminal capability responses, buffered keystrokes) from reaching the external app.

## Event Types

VT Code defines these application events:

**File:** `src/tui.rs:16-44`

| Event | Source | Purpose |
|-------|--------|---------|
| `Init` | start() | Application initialization signal |
| `Quit` | User/code | Request graceful shutdown |
| `Error` | Event handler | Unexpected error occurred |
| `Closed` | Event channel | Channel closed (shouldn't happen) |
| `Tick` | Timer (4 Hz default) | Update non-rendering state |
| `Render` | Timer (60 FPS default) | Redraw UI |
| `FocusGained` | crossterm | Terminal gained focus |
| `FocusLost` | crossterm | Terminal lost focus |
| `Paste(String)` | crossterm | User pasted text (bracketed paste mode) |
| `Key(KeyEvent)` | crossterm | Key pressed (Press kind only) |
| `Mouse(MouseEvent)` | crossterm | Mouse event |
| `Resize(u16, u16)` | crossterm | Terminal resized |

## Configuration

### Frame and Tick Rates

**File:** `src/tui.rs:70-72`

```rust
let tick_rate = 4.0;     // 4 ticks per second (250ms intervals)
let frame_rate = 60.0;   // 60 frames per second (16.7ms intervals)
```

**Adjust via builder:**
```rust
let tui = Tui::new()?
    .tick_rate(10.0)   // Faster state updates
    .frame_rate(120.0);  // Higher FPS for smooth animations
```

### Mouse and Paste Support

**File:** `src/tui.rs:63-65`

```rust
pub mouse: bool,   // Capture mouse events
pub paste: bool,   // Enable bracketed paste mode
```

**Enable via builder:**
```rust
let tui = Tui::new()?
    .mouse(true)
    .paste(true);
```

When enabled:
- Mouse events are sent via `Event::Mouse(MouseEvent)`
- Pasted text is sent via `Event::Paste(String)` (not individual keypresses)

## Integration with VT Code

VT Code's main event loop typically looks like:

```rust
loop {
    match tui.next().await {
        Some(Event::Key(key)) => {
            // Handle keypress
        }
        Some(Event::Mouse(mouse)) => {
            // Handle mouse click/scroll
        }
        Some(Event::Resize(w, h)) => {
            // Recalculate layout
        }
        Some(Event::Render) => {
            // Redraw UI
            tui.draw(|f| {
                // Render all widgets here
            })?;
        }
        Some(Event::Tick) => {
            // Update internal state (no rendering)
        }
        _ => {}
    }
}
```

## Best Practices

### 1. Filter Input on Windows

Always check `KeyEventKind::Press` on platforms that emit both press and release:
```rust
if key.kind == KeyEventKind::Press {
    // Process key
}
```

### 2. Don't Call terminal.draw() Multiple Times

  **Bad:**
```rust
tui.draw(|f| f.render_widget(widget1, ...))?;
tui.draw(|f| f.render_widget(widget2, ...))?;
tui.draw(|f| f.render_widget(widget3, ...))?;
```

  **Good:**
```rust
tui.draw(|f| {
    f.render_widget(widget1, ...);
    f.render_widget(widget2, ...);
    f.render_widget(widget3, ...);
})?;
```

Ratatui uses double buffering—multiple calls within one iteration only render the last one.

### 3. Use stderr for Rendering

VT Code uses `CrosstermBackend::new(std::io::stderr())` to allow stdout for piped output:
```bash
vtcode ask "task" | jq '.result'
```

### 4. Handle Terminal Resize Gracefully

Detect `Event::Resize(w, h)` and recalculate layouts. Modern widgets in Ratatui handle this automatically.

### 5. Suspend TUI for External Apps

Use `with_suspended_tui()` when launching editors:
```rust
tui.with_suspended_tui(|| {
    std::process::Command::new("vim")
        .arg(file_path)
        .status()
}).await?;
```

### 6. Use Bracketed Paste Mode

Enable to distinguish pasted text from individual keypresses:
```rust
let tui = Tui::new()?.paste(true);

match tui.next().await {
    Some(Event::Paste(text)) => {
        // Entire clipboard in one event
    }
    _ => {}
}
```

## Common Pitfalls

### Pitfall 1: Blocking the Async Runtime

  **Bad:**
```rust
let task = tokio::spawn(async {
    let _ = crossterm::event::read();  // Blocks the tokio runtime!
});
```

  **Good:**
```rust
let task = tokio::spawn(async {
    let _ = tokio::task::spawn_blocking(|| {
        crossterm::event::read()
    });
});
```

### Pitfall 2: Rendering in Tick Handler

  **Bad:**
```rust
Event::Tick => {
    tui.draw(|f| { /* ... */ })?;  // Blocks state updates
}
```

  **Good:**
```rust
Event::Tick => {
    // Update internal state only
    state.update();
}
Event::Render => {
    tui.draw(|f| { /* ... */ })?;
}
```

### Pitfall 3: Not Draining Events on Suspend

  **Bad:**
```rust
crossterm::terminal::disable_raw_mode()?;
external_app_result = f();  // Garbage input in external app!
crossterm::terminal::enable_raw_mode()?;
```

  **Good:** (as in `with_suspended_tui`)
```rust
while crossterm::event::poll(Duration::from_millis(0)).unwrap_or(false) {
    let _ = crossterm::event::read();  // Drain buffered events
}
crossterm::terminal::disable_raw_mode()?;
external_app_result = f();
crossterm::terminal::enable_raw_mode()?;
```

## Testing

VT Code includes tests in `src/tui.rs` (marked with `#[allow(dead_code)]`). To test event handling:

1. Create a mock `Event` stream
2. Assert state changes per event
3. Verify rendering output

Example (in vtcode-core):
```rust
#[tokio::test]
async fn test_key_event_handling() {
    let tui = Tui::new().unwrap();
    tui.enter().unwrap();
    
    // Simulate key press (in real tests, use crossterm::event::write_raw)
    // assert_eq!(state, expected);
    
    tui.exit().unwrap();
}
```

## See Also

- [Ratatui FAQ: Async & Tokio](https://ratatui.rs/faq/#when-should-i-use-tokio-and-async--await-)
- [Ratatui Recipe: External Apps](https://ratatui.rs/recipes/apps/spawn-vim/)
- [crossterm Event Docs](https://docs.rs/crossterm/0.27.0/crossterm/event/)
- [Tokio Select Documentation](https://tokio.rs/tokio/tutorial/select)
