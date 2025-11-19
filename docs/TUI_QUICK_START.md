# Ratatui TUI Quick Start Guide

## Quick Reference

### Basic Setup

```rust
use anyhow::Result;
use vtcode::tui::{Event, Tui};

#[tokio::main]
async fn main() -> Result<()> {
    // Create TUI instance
    let mut tui = Tui::new()?
        .tick_rate(4.0)      // 4 logical ticks per second
        .frame_rate(60.0)    // 60 renders per second
        .mouse(false)        // Disable mouse (default)
        .paste(false);       // Disable paste (default)

    // Enter terminal mode (raw mode + alternate screen + spawn event task)
    tui.enter()?;

    // Main loop
    let mut should_quit = false;
    while !should_quit {
        // Draw UI
        tui.draw(|frame| {
            // Rendering code here
        })?;

        // Handle events (non-blocking async)
        if let Some(event) = tui.next().await {
            match event {
                Event::Key(key) => {
                    // Handle keyboard input
                }
                Event::Render => {
                    // Frame render event (frame_rate frequency)
                }
                Event::Tick => {
                    // Logical tick (tick_rate frequency)
                }
                Event::Resize(w, h) => {
                    // Terminal was resized
                }
                Event::Quit => {
                    should_quit = true;
                }
                _ => {}
            }
        }
    }

    // Gracefully exit (cleanup automatic via Drop if forgotten)
    tui.exit()?;
    Ok(())
}
```

## Available Events

| Event | When Emitted | Use Case |
|-------|---|---|
| `Init` | TUI starts | Initialize state |
| `Key(KeyEvent)` | User presses key | Handle input |
| `Mouse(MouseEvent)` | User moves/clicks mouse | Track cursor (requires `.mouse(true)`) |
| `Resize(u16, u16)` | Terminal resized | Recalculate layout |
| `Render` | Frame rate interval | Throttle draws (at `.frame_rate()`) |
| `Tick` | Tick rate interval | Game logic, animations (at `.tick_rate()`) |
| `FocusGained` | Terminal gained focus | Resume updates |
| `FocusLost` | Terminal lost focus | Pause updates |
| `Paste(String)` | Text pasted | Handle pasted content (requires `.paste(true)`) |
| `Error` | Event handler fails | Shutdown gracefully |
| `Closed` | Event channel closes | Shutdown gracefully |

## Common Patterns

### Handle Keyboard Shortcuts

```rust
use crossterm::event::{KeyCode, KeyModifiers};

if let Some(Event::Key(key)) = tui.next().await {
    match (key.code, key.modifiers) {
        // Simple keys
        (KeyCode::Char('q'), KeyModifiers::empty()) => should_quit = true,
        
        // Modifiers
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => should_quit = true,
        
        // Special keys
        (KeyCode::Esc, _) => should_quit = true,
        (KeyCode::Enter, _) => process_input(),
        
        // Arrow keys
        (KeyCode::Up, _) => move_up(),
        (KeyCode::Down, _) => move_down(),
        (KeyCode::Left, _) => move_left(),
        (KeyCode::Right, _) => move_right(),
        
        _ => {}
    }
}
```

### Implement MVU (Model-View-Update) Loop

```rust
struct Model {
    counter: u32,
    input: String,
}

impl Model {
    fn render(&self, frame: &mut ratatui::Frame) {
        // Draw UI based on state
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Key(key) => {
                match key.code {
                    KeyCode::Char(c) => self.input.push(c),
                    KeyCode::Backspace => { self.input.pop(); }
                    _ => {}
                }
            }
            Event::Tick => {
                self.counter = self.counter.saturating_add(1);
            }
            _ => {}
        }
    }
}

// In main loop:
let mut model = Model::default();
loop {
    tui.draw(|f| model.render(f))?;
    if let Some(event) = tui.next().await {
        model.handle_event(event);
    }
}
```

### Enable Mouse Support

```rust
let mut tui = Tui::new()?
    .mouse(true)    // Enable mouse
    .paste(true);   // Enable bracketed paste

// Later:
if let Some(Event::Mouse(mouse_event)) = tui.next().await {
    use crossterm::event::MouseEventKind;
    match mouse_event.kind {
        MouseEventKind::Down(_) => println!("Mouse down at ({}, {})", mouse_event.column, mouse_event.row),
        MouseEventKind::Up(_) => println!("Mouse up"),
        MouseEventKind::Drag(_) => println!("Mouse drag"),
        MouseEventKind::Moved => println!("Mouse moved"),
        MouseEventKind::ScrollDown => println!("Scroll down"),
        MouseEventKind::ScrollUp => println!("Scroll up"),
        _ => {}
    }
}
```

### Throttle Updates with Render Event

```rust
loop {
    // Only draw when render event fires (at frame_rate frequency)
    // This prevents busy-waiting in the draw loop
    if let Some(Event::Render) = tui.next().await {
        tui.draw(|frame| {
            // Expensive rendering
        })?;
    }
}
```

### Separate Game Logic from Rendering

```rust
loop {
    // Draw every frame
    tui.draw(|f| { /* rendering */ })?;

    if let Some(event) = tui.next().await {
        match event {
            // Logic runs at tick rate
            Event::Tick => {
                app.update();  // Game logic, animations
            }
            // Rendering happens at frame rate (automatically)
            Event::Render => {
                // Frame render event (you can use for expensive ops)
            }
            // Input handled immediately
            Event::Key(key) => {
                app.handle_input(key);
            }
            _ => {}
        }
    }
}
```

### Suspend/Resume (Unix Only)

```rust
#[cfg(not(windows))]
{
    if let Some(Event::Key(key)) = tui.next().await {
        use crossterm::event::{KeyCode, KeyModifiers};
        if key.code == KeyCode::Char('z') && key.modifiers.contains(KeyModifiers::CONTROL) {
            tui.suspend()?;  // Suspend to shell (Ctrl+Z)
            // Returns when user resumes (fg command in shell)
        }
    }
}
```

## Configuration Guide

### Frame Rate vs Tick Rate

- **Frame Rate**: Rendering frequency
  - Higher = more frequent redraws (smoother but more CPU)
  - Default: 60.0 (60 redraws/sec)
  - Range: 1.0 - 1000.0+ (recommended 30-144)

- **Tick Rate**: Logic/update frequency
  - Higher = more frequent updates (better responsiveness but more CPU)
  - Default: 4.0 (4 updates/sec)
  - Range: 0.1 - 60.0+

### Recommended Configs

```rust
// Low-power mode (background monitoring)
Tui::new()?.tick_rate(1.0).frame_rate(2.0)

// Default mode (general purpose)
Tui::new()?.tick_rate(4.0).frame_rate(60.0)

// Game/animation mode (high responsiveness)
Tui::new()?.tick_rate(30.0).frame_rate(144.0)

// Text editor (high tick, high frame)
Tui::new()?.tick_rate(60.0).frame_rate(120.0)
```

## Error Handling

### Graceful Shutdown on Errors

```rust
loop {
    tui.draw(|f| { /* */ })?;

    match tui.next().await {
        Some(Event::Error) => {
            eprintln!("Terminal error!");
            break;
        }
        Some(Event::Closed) => {
            eprintln!("Event channel closed!");
            break;
        }
        // ... other events ...
        None => break,  // Channel dropped
        _ => {}
    }
}

tui.exit()?;  // Cleanup
```

### Automatic Cleanup via Drop

You don't have to call `tui.exit()` - it's automatic:

```rust
{
    let mut tui = Tui::new()?;
    tui.enter()?;
    // ... use tui ...
} // Drop is called here, tui.exit() runs automatically
```

## Performance Tips

1. **Use Render events for expensive operations**: Only update complex UI when `Event::Render` fires
2. **Batch updates**: Process multiple Tick events before redrawing
3. **Use Tick for animation**: Keep frame-independent logic in Tick handlers
4. **Profile frame rates**: Monitor actual vs configured rates with system tools
5. **Avoid blocking in event loop**: Use tokio tasks for long operations

## Testing

Run the example:

```bash
cargo run --example tui_event_handler
```

The example demonstrates:
- Basic event loop
- Multiple event types
- Keyboard input handling
- Counter state updates
- Graceful shutdown

Press:
- **Space**: Increment counter
- **r**: Reset counter
- **q**: Quit

## Troubleshooting

### Terminal doesn't exit properly
→ Make sure you call `tui.exit()` or let Drop handle it automatically

### Events feel slow/unresponsive
→ Increase `tick_rate` for faster logical updates

### CPU usage too high
→ Decrease `frame_rate` for fewer redraws

### Mouse events not received
→ Call `.mouse(true)` when building Tui

### Paste events not received
→ Call `.paste(true)` when building Tui

### Terminal state corrupted on panic
→ Drop handler attempts cleanup, but consider using a panic hook:
```rust
use color_eyre::config::HookBuilder;

let hook = HookBuilder::default()
    .display_location(false)
    .install();
```

## See Also

- [TUI_EVENT_HANDLER_REFINEMENT.md](./TUI_EVENT_HANDLER_REFINEMENT.md) - Detailed design doc
- [examples/tui_event_handler.rs](../examples/tui_event_handler.rs) - Full example
- [Ratatui Docs](https://docs.rs/ratatui)
- [Crossterm Docs](https://docs.rs/crossterm)
