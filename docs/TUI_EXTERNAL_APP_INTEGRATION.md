# TUI & External App Integration Guide

## Overview

The refined `Tui` struct now includes the `ExternalAppLauncher` trait for safely launching external applications (editors, git clients, etc.) while maintaining proper terminal state.

This integrates the patterns from:
- [Ratatui spawn-vim recipe](https://ratatui.rs/recipes/apps/spawn-vim/)
- [vtcode-core TerminalAppLauncher](../vtcode-core/src/tools/terminal_app.rs)

## Design Pattern

The `with_suspended_tui()` method ensures correct terminal state management when spawning external applications:

```rust
tui.with_suspended_tui(|| {
    // Terminal is now in normal mode
    // External app can use terminal freely
    Command::new("vim").arg(file).status()
}).await?;
```

### State Transitions

```
TUI Active
    ↓
Stop event handler
    ↓
Leave alternate screen
    ↓
Drain pending events (CRITICAL!)
    ↓
Disable raw mode
    ↓
[External app runs freely here]
    ↓
Re-enable raw mode
    ↓
Re-enter alternate screen
    ↓
Clear terminal (remove artifacts)
    ↓
Restart event handler
    ↓
TUI Active
```

## Key Design Decisions

### 1. Drain Pending Events (CRITICAL)

Before disabling raw mode, we drain all pending crossterm events:

```rust
while crossterm::event::poll(Duration::from_millis(0)).unwrap_or(false) {
    let _ = crossterm::event::read();
}
```

**Why this matters:**
- Prevents garbage input (terminal capability responses, buffered keystrokes) from reaching the external app
- Without this, vim/nvim might behave unexpectedly or receive corrupt input
- This is the most critical step in the state transition

### 2. Clear Terminal After Resume

After re-entering alternate screen, we clear the entire terminal:

```rust
crossterm::execute!(
    std::io::stderr(),
    crossterm::terminal::Clear(crossterm::terminal::ClearType::All)
)?;
```

**Why this matters:**
- Removes ANSI escape codes from the external app (e.g., vim's background color requests)
- Prevents artifacts and color bleeding into the TUI
- Ensures clean terminal state for TUI to render

### 3. Stop vs. Exit

The method calls `stop()` instead of `exit()`:

```rust
self.stop()?;  // Stops event handler, but keeps raw mode active
```

**Why this matters:**
- We need to manually control when we disable raw mode (after event handler stops)
- `exit()` would disable raw mode for us, but we need finer control
- This allows us to restart the event handler after the app completes

## Usage Patterns

### Pattern 1: Launch Editor

```rust
use std::process::Command;
use vtcode::tui::ExternalAppLauncher;

// Launch user's editor on a file
tui.with_suspended_tui(|| {
    Command::new("vim")
        .arg("src/main.rs")
        .current_dir(workspace_root)
        .status()
        .map(|s| s.success())
        .map_err(|e| anyhow::anyhow!("{}", e))
}).await?;
```

### Pattern 2: Launch Git Client

```rust
// Launch Lazygit or interactive git
tui.with_suspended_tui(|| {
    let cmd = if which::which("lazygit").is_ok() {
        "lazygit"
    } else {
        "git"
    };
    
    Command::new(cmd)
        .current_dir(workspace_root)
        .status()
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("{}", e))
}).await?;
```

### Pattern 3: Reuse TerminalAppLauncher

The existing `TerminalAppLauncher` in vtcode-core can be adapted:

```rust
impl TerminalAppLauncher {
    /// Launch editor, suspending TUI if available
    pub async fn launch_editor_with_tui(
        &self,
        file: Option<PathBuf>,
        tui: Option<&mut Tui>,
    ) -> Result<Option<String>> {
        if let Some(tui) = tui {
            // Use TUI-aware suspension
            tui.with_suspended_tui(|| {
                self.launch_editor_blocking(file)
            }).await
        } else {
            // Fallback to non-TUI version
            self.launch_editor_blocking(file)
        }
    }
    
    fn launch_editor_blocking(&self, file: Option<PathBuf>) -> Result<Option<String>> {
        // ... existing implementation ...
    }
}
```

### Pattern 4: Multi-Step Operations

```rust
// Handle user interaction in editor, then continue
tui.with_suspended_tui(|| {
    // Step 1: Let user edit a file
    Command::new("vim").arg("notes.md").status()?;
    
    // Step 2: Let user review in pager
    Command::new("less").arg("notes.md").status()?;
    
    Ok(())
}).await?;
```

## Error Handling

### Graceful Recovery

If the external app fails, the TUI state is still properly restored:

```rust
match tui.with_suspended_tui(|| {
    Command::new("vim")
        .arg(file)
        .status()
        .map_err(|e| anyhow::anyhow!("Failed to launch vim: {}", e))
}).await {
    Ok(_) => {
        // App succeeded, continue
    }
    Err(e) => {
        // App failed or state restoration failed
        // TUI is still properly restored
        eprintln!("Error: {}", e);
    }
}
```

### Timeout Handling

The event handler restart has a timeout (100ms):

```rust
pub fn stop(&self) -> Result<()> {
    self.cancel();
    let mut counter = 0;
    while !self.task.is_finished() {
        std::thread::sleep(Duration::from_millis(1));
        counter += 1;
        if counter > 50 {
            self.task.abort();  // Force abort after 50ms
        }
        if counter > 100 {
            tracing::error!("Failed to abort task in 100 milliseconds");
            break;  // Give up after 100ms
        }
    }
    Ok(())
}
```

## Integration with Existing Code

### TerminalAppLauncher Refactoring (COMPLETED)

The vtcode-core `TerminalAppLauncher` has been refactored to use a unified `suspend_terminal_for_command()` pattern:

**Key changes in vtcode-core/src/tools/terminal_app.rs:**

1. **New internal method:** `suspend_terminal_for_command()`
   - Centralizes all terminal state management
   - Ensures consistent behavior for all external apps
   - Includes critical event draining step

2. **Refactored `launch_editor()`**
   - Now uses `suspend_terminal_for_command()` internally
   - Cleaner separation of concerns
   - Reduced code duplication

3. **New `launch_git_interface()` method**
   - Attempts lazygit first, falls back to git
   - Uses same terminal suspension pattern
   - Ready to be called from CLI handlers

**Before (duplicated logic):**
```rust
// Manual state management repeated for each app type
io::stdout().execute(LeaveAlternateScreen)?;
while crossterm::event::poll(...).unwrap_or(false) {
    let _ = crossterm::event::read();
}
disable_raw_mode()?;
Command::new(&editor).arg(&file_path).status()?;
io::stdout().execute(EnterAlternateScreen)?;
enable_raw_mode()?;
io::stdout().execute(Clear(ClearType::All))?;
```

**After (unified logic):**
```rust
// Single method handles all terminal state transitions
self.suspend_terminal_for_command(|| {
    Command::new(&editor).arg(&file_path).status()
})?;
```

### Location in Architecture

```
vtcode (binary) - Main TUI app
 src/tui.rs - Tui struct + ExternalAppLauncher trait
    For async/event-driven contexts
 src/main.rs - Main event loop
 src/interactive_list.rs - Interactive selection UI

vtcode-core (library)
 src/tools/terminal_app.rs - TerminalAppLauncher
    launch_editor() - Uses suspend_terminal_for_command()
    launch_git_interface() - Uses suspend_terminal_for_command()
    suspend_terminal_for_command() - Unified logic
 Works in blocking contexts
```

### Two Integration Patterns

**Pattern A: Blocking context (vtcode-core TerminalAppLauncher)**
```rust
pub fn launch_editor(&self, file: Option<PathBuf>) -> Result<Option<String>> {
    self.suspend_terminal_for_command(|| {
        Command::new(&editor).arg(&file_path).status()
    })?;
    // ... read results ...
}
```

**Pattern B: Async context (vtcode Tui struct)**
```rust
tui.with_suspended_tui(|| {
    Command::new(&editor).arg(&file_path).status()
}).await?;
```

Both ensure identical terminal state transitions and event handling.

## Performance Considerations

### Event Handler Restart Latency

When resuming after external app:
1. Stop event handler: ~1-50ms (sync)
2. Terminal state changes: ~0.1-1ms (sync)
3. External app execution: variable
4. Terminal state recovery: ~0.1-1ms (sync)
5. Restart event handler: <1ms (spawn task)

Total overhead: ~2-52ms (mostly from graceful shutdown)

### Optimization: Reuse Event Channels

For repeated suspensions (e.g., multiple editor launches), the event channels are reused:
- Event sender/receiver are not dropped
- Only the background task is stopped/restarted
- Minimal allocation overhead

## Testing

### Unit Test Pattern

```rust
#[tokio::test]
async fn test_suspended_tui_restores_state() {
    let mut tui = Tui::new().unwrap();
    tui.enter().unwrap();
    
    let result = tui.with_suspended_tui(|| {
        // Verify terminal is in normal mode
        assert!(!crossterm::terminal::is_raw_mode_enabled().unwrap());
        Ok(())
    }).await;
    
    assert!(result.is_ok());
    
    // Verify TUI is still active
    tui.draw(|f| { /* render */ }).unwrap();
}
```

### Integration Test Pattern

```rust
#[tokio::test]
async fn test_launch_editor() {
    let mut tui = Tui::new().unwrap();
    tui.enter().unwrap();
    
    tui.with_suspended_tui(|| {
        Command::new("true")  // Mock editor that succeeds
            .status()
            .map_err(|e| anyhow::anyhow!("{}", e))
    }).await.unwrap();
    
    // Verify TUI is still responsive
    let event = tokio::time::timeout(
        Duration::from_millis(100),
        tui.next()
    ).await;
    
    tui.exit().unwrap();
}
```

## Troubleshooting

### Issue: External app receives garbage input
**Solution:** Ensure `crossterm::event::poll()` drains all events before disabling raw mode.

### Issue: Terminal shows artifacts after app exits
**Solution:** The terminal clear command is required. If still seeing artifacts, check that `ClearType::All` is being used (not just `Purge`).

### Issue: Event handler doesn't restart
**Solution:** Check that `self.start()` is called after `with_suspended_tui()`. This happens automatically in the method.

### Issue: External app hangs or doesn't receive input
**Solution:** Verify that raw mode is properly disabled before spawning the app. Check `crossterm::terminal::is_raw_mode_enabled()`.

## Future Enhancements

1. **Event queue preservation**: Option to save/restore event queue
2. **Nested suspension**: Handle multiple levels of app launching
3. **Custom event loop**: Allow apps to provide their own event handler
4. **Performance metrics**: Track suspension latency
5. **Signal handling**: Support SIGTSTP/SIGCONT for app suspension

## See Also

- [TUI_EVENT_HANDLER_REFINEMENT.md](./TUI_EVENT_HANDLER_REFINEMENT.md) - Core TUI design
- [TUI_QUICK_START.md](./TUI_QUICK_START.md) - Quick reference
- [Ratatui spawn-vim recipe](https://ratatui.rs/recipes/apps/spawn-vim/)
- [vtcode-core TerminalAppLauncher](../vtcode-core/src/tools/terminal_app.rs)
