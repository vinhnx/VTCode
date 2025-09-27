TODO: implement MCP allow list in we built with tools policy approval list. implement same logic for MCP tools.
idea: showing vtcode.toml config settings in ratatui modal


--

https://blog.nilenso.com/blog/2025/09/25/swe-benchmarks/

---

idea: showing vtcode.toml config settings in ratatui modal

---

<https://docs.exa.ai/reference/exa-mcp>

---

https://platform.openai.com/docs/guides/function-calling
improve function calling openai


--



https://docs.exa.ai/reference/exa-mcp

---

--

sync account with <https://vtchat.io.vn/>
idea: showing vtcode.toml config settings in an inline settings overlay

---

vscode extenson <https://code.visualstudio.com/api/get-started/your-first-extension>
<https://docs.exa.ai/reference/exa-mcp>

---

Fix homebrew issue
<https://github.com/vinhnx/vtcode/issues/61>

brew install vinhnx/tap/vtcode
==> Fetching downloads for: vtcode
==> Fetching vinhnx/tap/vtcode
==> Downloading <https://github.com/vinhnx/vtcode/releases/download/v0.8.2/vtcode-v0.8.2-aarch64-apple-darwin.tar.gz>
curl: (56) The requested URL returned error: 404

Error: vtcode: Failed to download resource "vtcode (0.8.2)"
Download failed: <https://github.com/vinhnx/vtcode/releases/download/v0.8.2/vtcode-v0.8.2-aarch64-apple-darwin.tar.gz>
==> No outdated dependents to upgrade!

--

sync account with <https://vtchat.io.vn/>

---

vscode extenson <https://code.visualstudio.com/api/get-started/your-first-extension>

--

enhance realtime and terminal size view port changes, for example using in small panes and responsive ui in tui.

--

<https://docs.claude.com/en/docs/claude-code/hooks-guide>

---

<https://docs.claude.com/en/docs/claude-code/output-styles>

---

<https://docs.claude.com/en/docs/claude-code/settings>

--

benchmark terminal bench
<https://www.tbench.ai/>

--

<https://agentclientprotocol.com/overview/introduction>

--

mcp integration
<https://modelcontextprotocol.io/>

---

<https://github.com/mgrachev/update-informer>


--

submit r/rust/

--

submit hackernew


-- 

update docs and readme about recent tui changes

-- 

MCP allow list

--


fix scrolling view port

## Key Solutions for Forcing Terminal Refresh

### 1. **Use `terminal.draw()` Properly**

Ratatui employs immediate mode rendering, which means it only updates when you tell it to. Make sure you're calling `terminal.draw()` after each state change:

```rust
use ratatui::Terminal;
use crossterm::terminal::{enable_raw_mode, disable_raw_mode};

// In your main loop
loop {
    // Handle events first
    if let Ok(event) = crossterm::event::read() {
        // Process your events
        handle_event(event, &mut app_state);
    }

    // Force a redraw after state changes
    terminal.draw(|frame| {
        ui(frame, &app_state);
    })?;
}
```

### 2. **Manual Backend Flushing**

For more control, you can access the backend directly and force a flush:

```rust
use std::io::Write;
use crossterm::terminal;

// After drawing, flush the backend
terminal.draw(|frame| {
    ui(frame, &app_state);
})?;

// Force flush the terminal output
std::io::stdout().flush()?;
```

### 3. **Clear and Redraw Pattern**

If you're having persistent rendering issues, use the clear-then-draw pattern:

```rust
use crossterm::terminal::{Clear, ClearType};
use crossterm::ExecutableCommand;

// Clear the terminal completely before drawing
std::io::stdout().execute(Clear(ClearType::All))?;
terminal.draw(|frame| {
    ui(frame, &app_state);
})?;
```

### 4. **Complete Rendering Setup**

Here's a robust pattern that addresses common rendering issues:

```rust
use ratatui::{backend::CrosstermBackend, Terminal};
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::io::{self, stdout, Stdout};

type Tui = Terminal<CrosstermBackend<Stdout>>;

fn init() -> io::Result<Tui> {
    execute!(stdout(), EnterAlternateScreen)?;
    enable_raw_mode()?;
    Terminal::new(CrosstermBackend::new(stdout()))
}

fn restore() -> io::Result<()> {
    execute!(stdout(), LeaveAlternateScreen)?;
    disable_raw_mode()?;
    Ok(())
}

// Main loop with proper cleanup
fn main() -> io::Result<()> {
    let mut terminal = init()?;
    let result = run_app(&mut terminal);
    restore()?;
    result
}

fn run_app(terminal: &mut Tui) -> io::Result<()> {
    loop {
        // Always redraw on each loop iteration
        terminal.draw(|frame| {
            ui(frame, &app_state);
        })?;

        // Handle events
        if event::poll(std::time::Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') {
                    break;
                }
                // Handle other events and update state
                handle_key_event(key, &mut app_state);
            }
        }

        // Optional: Add a small delay to prevent excessive CPU usage
        std::thread::sleep(std::time::Duration::from_millis(16)); // ~60 FPS
    }
    Ok(())
}
```

### 5. **Force Refresh with Backend Methods**

You can also use lower-level backend methods:

```rust
use ratatui::backend::Backend;

// Force a complete refresh
terminal.backend_mut().clear()?;
terminal.draw(|frame| {
    ui(frame, &app_state);
})?;
terminal.backend_mut().flush()?;
```

## Common Issues and Solutions

**Problem**: UI doesn't update after state changes

- **Solution**: Always call `terminal.draw()` after modifying application state

**Problem**: Artifacts or partial rendering

- **Solution**: Use `terminal.clear()` or `Clear(ClearType::All)` before drawing

**Problem**: Flickering or inconsistent rendering

- **Solution**: Implement proper frame rate limiting and ensure you're not calling draw too frequently

**Problem**: Terminal state corruption

- **Solution**: Always use proper setup/teardown with alternate screen and raw mode

The key insight from Ratatui's immediate rendering principle is that for each frame, your app must render all widgets that are supposed to be part of the UI, so make sure your render function completely redraws the entire interface each time.

---

fetch context7 ratatui and crossterm to improve refresh rate for optimal screen refresh and make sure optimal performant

---

fix not to execessive force screen refresh to improve performant

---

check scresnshot. whenever the human-in-the-loop prompt appear. the screen render is broken. text and visual are glitch. fix this is important

---

please double check version tag 0.14.1. the tui works better in that version. no visual glitch. can you review the whole tui change and revert back to previous version. make sure latest 0.15.0 keep core underlying logic as in
