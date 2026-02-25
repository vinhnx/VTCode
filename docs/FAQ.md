# VT Code FAQ

Based on best practices from Ratatui and terminal UI development, this FAQ addresses common questions about VT Code architecture and usage.

## Terminal & TUI

### Why don't I see duplicate key events on Windows?

VT Code correctly filters key events to only process `KeyEventKind::Press`, avoiding duplicate events from both press and release.

See `src/tui.rs:153`:
```rust
if key.kind == KeyEventKind::Press {
    let _ = _event_tx.send(Event::Key(key));
}
```

This pattern is **cross-platform compatible** (Windows, macOS, Linux).

### Why should VT Code use tokio/async?

VT Code uses Tokio for:
1. **Event multiplexing** - Handling terminal events, ticks, and renders concurrently without blocking
2. **Multi-tool execution** - Running MCP tools, PTY sessions, and API calls in parallel
3. **Lifecycle hooks** - Running shell commands asynchronously during agent events

The architecture uses `tokio::select!` to multiplex:
- Terminal input (blocking read in spawned task)
- Frame rate ticks (60 FPS)
- Event processing ticks (4 Hz)

**When NOT to use async:**
- Simple one-off CLI tasks (prefer synchronous main loop)
- Tools that don't need concurrent execution

VT Code's multi-agent coordination and tool execution justify async.

### Why use stderr instead of stdout for terminal rendering?

VT Code renders to `stderr` (via `CrosstermBackend::new(std::io::stderr())` in `src/tui.rs:73`).

**Rationale:**
- Allows piping output: `vtcode ask "task" | jq` doesn't break the TUI
- Makes the TUI work out-of-the-box in pipes (no special TTY detection needed)
- Compatible with shell pipelines and CI/CD environments

### Can I run multiple terminal.draw() calls in the same loop?

No. VT Code uses a single `terminal.draw()` call per frame that renders all widgets together. See `vtcode-core/src/ui/tui/session.rs:render()` method, which orchestrates all UI components in one closure.

### How do I pipe output to other tools?

Because VT Code strictly separates data (`stdout`) from metadata/logging (`stderr`), you can pipe the output of commands like `ask` and `exec` directly.

```bash
# Code goes to file, logs stay on screen
vtcode ask "code for a fibonacci function" > fib.py
```

Use `vtcode exec --json` to get a pipeable stream of structured events.

### How does VT Code handle terminal resizing?

VT Code listens for `Event::Resize(x, y)` events and updates the layout automatically. The TUI widgets reflow based on the new terminal dimensions—no special handling needed.

### Can I change font size in a VT Code terminal?

No, VT Code can't control terminal font size. That's a terminal emulator setting. VT Code adapts to the terminal's actual size via `Event::Resize`.

**Tip:** Use `tui-big-text` or `figlet` for large ASCII art titles within the TUI.

### What characters look weird or display as ?

VT Code assumes a **Nerd Font** for box-drawing and icon support. Install one of:
- [Nerd Fonts](https://www.nerdfonts.com/) (recommended)
- [Kreative Square](http://www.kreativekorp.com/software/fonts/ksquare/)

## Architecture & Design

### Is VT Code a library or framework?

**VT Code is a tool/agent**, not a library or framework.

However, `vtcode-core` (the Rust crate) is a **library** you can use in your own Rust projects. The binary (`vtcode` CLI) uses this library to implement a coding agent.

**Philosophy:**
- **Library-like:** You control the event loop and can extend VT Code's functionality
- **Agent-first:** The CLI provides opinionated defaults for common coding tasks

### How does VT Code differ from similar tools?

VT Code is an **AI coding agent** with:
- **Security-first:** Execution policies, workspace isolation, tool policies, and tree-sitter-bash command validation
- **Multi-LLM:** OpenAI, Anthropic, Gemini, Ollama, LM Studio, etc.
- **Semantic code understanding:** LLM-native analysis and navigation across all modern languages
- **Context engineering:** Token budget tracking, dynamic context curation
- **Editor integration:** Agent Client Protocol (ACP) for Zed, Cursor, etc.

### Why does VT Code use async/await extensively?

**Reasons:**
1. **Tool execution:** MCP tools, PTY sessions, API calls run concurrently
2. **Event handling:** Terminal input, ticks, renders multiplexed with `tokio::select!`
3. **Streaming:** Real-time AI responses streamed without blocking
4. **Lifecycle hooks:** Shell commands execute without blocking the agent loop

**Result:** VT Code remains responsive even during long-running operations.

### How does VT Code manage terminal state?

VT Code uses a robust state machine in `src/tui.rs`:
1. **Enter:** Enable raw mode → alternate screen → start event handler
2. **Running:** Process events → update state → render UI
3. **Exit:** Stop event handler → leave alternate screen → disable raw mode

The `ExternalAppLauncher` trait allows suspending the TUI to launch editors, git clients, etc.

### Why suspend the TUI to launch external apps?

See `src/tui.rs:303` (`with_suspended_tui`):
1. Stops event handler
2. Leaves alternate screen
3. **Drains pending events** (critical!)
4. Disables raw mode
5. Runs external app
6. Re-enables raw mode and returns

This prevents terminal artifacts and ensures external apps get clean input/output.

## Debugging & Troubleshooting

### How do I enable debug logging?

Set `RUST_LOG` environment variable:
```bash
RUST_LOG=vtcode_core=debug,vtcode=debug vtcode
```

Or configure in `vtcode.toml`:
```toml
[debug]
enable_tracing = true
trace_level = "debug"
trace_targets = ["vtcode_core", "vtcode"]
```

See `src/main.rs:244` and `src/main.rs:260` for initialization.

### How does VT Code handle buffer overruns?

The Ratatui FAQ recommends using `area.intersection(buf.area)` to prevent out-of-bounds rendering. This is applied in VT Code's widget implementations to clamp rendering to valid regions.

**Best practice:** Use `Rect::intersection()` and `Rect::clamp()` when calculating layouts manually.

## Performance & Optimization

### What's VT Code's frame and tick rate?

- **Frame rate:** 60 FPS (default in `src/tui.rs:72`)
- **Tick rate:** 4 Hz (default in `src/tui.rs:71`)

Both are configurable via builder methods:
```rust
tui.frame_rate(60.0).tick_rate(4.0)
```

### How does VT Code reduce latency?

1. **Async event handling:** Non-blocking `tokio::select!` in `src/tui.rs:138`
2. **Double buffering:** Ratatui only renders diffs, not full redraws
3. **Lazy rendering:** Only recompute UI when state changes (not every frame)

### Can I use VT Code in a pipe?

Yes. VT Code detects when stdout is a pipe (not a TTY) and adapts:
- **Interactive mode:** Full TUI if terminal is available
- **Pipe mode:** Text output (if piped)

Check `src/main.rs:228` for TTY detection.

## See Also

- [Ratatui FAQ](https://ratatui.rs/faq/) - Terminal UI best practices
- [src/tui.rs](../../src/tui.rs) - Terminal event handler implementation
- [docs/ARCHITECTURE.md](./ARCHITECTURE.md) - VT Code system architecture
