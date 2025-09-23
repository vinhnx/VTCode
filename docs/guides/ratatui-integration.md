# Integrating `ratatui` into VTCode

## Why `ratatui`

[`ratatui`](https://ratatui.rs) provides a mature immediate-mode terminal UI engine that works
seamlessly with `crossterm`, supports partial redraws outside of fullscreen alternate screen mode,
and offers first-class widgets for chat-style layouts. These characteristics line up with VTCode's
requirements:

- Render a streaming chat transcript and prompt without taking over the entire terminal.
- Preserve keyboard shortcuts such as double-Esc exit, Ctrl+C interrupt, and scrolling gestures.
- Reuse the existing theming pipeline based on `anstyle` while respecting syntax highlighting.
- Keep asynchronous event handling inside the Tokio runtime used by the agent loop.

## VTCode terminal surface after the migration

| Responsibility | Location | Notes |
| --- | --- | --- |
| Session bootstrap + renderer ownership | `run_single_agent_loop_unified` spawns a `RatatuiSession` and wires an `AnsiRenderer` sink.【F:src/agent/runloop/unified/turn.rs†L443-L477】 | The session exposes a `RatatuiHandle` for output commands and an event receiver for user input. |
| Streaming response rendering | `AnsiRenderer::stream_markdown_response` forwards segments to the ratatui sink while maintaining the persistent transcript.【F:vtcode-core/src/utils/ansi.rs†L254-L302】 | Streaming replacements are issued through `RatatuiCommand::ReplaceLast` with an explicit `RatatuiMessageKind` so each block retains its styling. |
| Prompt indicator, banner, tool summary | Rendered through existing helpers that now call `AnsiRenderer::with_ratatui` so the UI and transcript stay in sync.【F:src/agent/runloop/unified/display.rs†L1-L32】【F:src/agent/runloop/ui.rs†L13-L79】 |
| Chat input loop | Managed by the modular TUI package: `RatatuiLoop` owns the input buffer and command queue while `events::handle_event` translates keyboard and mouse input into `RatatuiEvent` notifications for the agent.【F:vtcode-core/src/ui/tui/state/mod.rs†L1018-L1180】【F:vtcode-core/src/ui/tui/state/mod.rs†L1740-L1763】【F:vtcode-core/src/ui/tui/events/mod.rs†L14-L197】 |
| Transcript + context trimming | Conversation tracking, scroll offsets, and PTY anchoring live inside the TUI state module, and the renderer consumes that metadata to build the transcript viewport without touching the persistent log.【F:vtcode-core/src/ui/tui/state/mod.rs†L479-L556】【F:vtcode-core/src/ui/tui/state/mod.rs†L972-L1180】【F:vtcode-core/src/ui/tui/render/mod.rs†L177-L245】【F:vtcode-core/src/ui/tui/render/mod.rs†L471-L546】 |
| Tool shell behavior | Unchanged: helpers render into the shared `AnsiRenderer` sink, which writes both to stdout and the ratatui session.【F:src/agent/runloop/unified/shell.rs†L1-L119】 |

## Key components introduced for `ratatui`

1. **`vtcode-core/src/ui/tui.rs`**
   - Owns session orchestration, including terminal setup, the async event loop, and the public `RatatuiSession`/`RatatuiHandle` API surface.【F:vtcode-core/src/ui/tui.rs†L1-L134】
   - Routes drawing to the internal state machine while supervising autoresize handling and the periodic redraw ticker.【F:vtcode-core/src/ui/tui.rs†L39-L127】

2. **`vtcode-core/src/ui/tui/state`**
   - Defines the shared data structures (`RatatuiCommand`, `RatatuiEvent`, scroll state, PTY tracking, status bar content) and encapsulates transcript mutation logic.【F:vtcode-core/src/ui/tui/state/mod.rs†L65-L156】【F:vtcode-core/src/ui/tui/state/mod.rs†L479-L1180】
   - Provides helpers for draining command queues, applying themes, managing placeholders, and coordinating PTY output with transcript autoscroll.【F:vtcode-core/src/ui/tui/state/mod.rs†L1018-L1180】【F:vtcode-core/src/ui/tui/state/mod.rs†L1700-L1763】

3. **`vtcode-core/src/ui/tui/events`**
   - Converts `crossterm` keyboard and mouse events into high-level actions, handles conversation navigation, and maintains slash-command selection state.【F:vtcode-core/src/ui/tui/events/mod.rs†L14-L197】

4. **`vtcode-core/src/ui/tui/render`**
   - Implements layout calculation, transcript rendering, PTY window placement, input widgets, and the status bar using Ratatui widgets and styling helpers.【F:vtcode-core/src/ui/tui/render/mod.rs†L1-L245】【F:vtcode-core/src/ui/tui/render/mod.rs†L471-L546】

5. **`AnsiRenderer::with_ratatui`**
   - Wraps a `RatatuiHandle` so all structured output flows through ratatui while continuing to append
     to the persistent transcript log.【F:vtcode-core/src/utils/ansi.rs†L61-L155】
   - Converts `anstyle::Style` data into `ratatui` colors and modifiers to preserve syntax
     highlighting and theming.【F:vtcode-core/src/utils/ansi.rs†L305-L379】

6. **Run loop integration**
   - `run_single_agent_loop_unified` spawns the session, applies the active theme, and routes events
     from `RatatuiEvent` into the existing control flow (scrolling, cancel, interrupt, exit).【F:src/agent/runloop/unified/turn.rs†L443-L720】
   - `PlaceholderGuard` and `PlaceholderSpinner` continue to operate, now delegating to
     `RatatuiHandle::set_placeholder` so progress messages and hints show beneath the prompt.【F:src/agent/runloop/unified/turn.rs†L180-L360】

## Migration and maintenance checklist

1. Remove all references to `iocraft` crates, modules, and docs. Ensure `vtcode-core/Cargo.toml`
   only declares `ratatui` and supporting utilities (e.g., `unicode-width`).【F:vtcode-core/Cargo.toml†L84-L96】
2. Re-export the ratatui surface from `vtcode-core/src/ui/mod.rs` so downstream crates import
   `vtcode_core::ui::tui` exclusively.【F:vtcode-core/src/ui/mod.rs†L6-L21】
3. Update call sites to use `AnsiRenderer::with_ratatui` and the new event types, keeping the business
   logic unchanged.【F:src/agent/runloop/unified/turn.rs†L443-L720】
4. Verify that Markdown streaming, placeholder hints, and theme updates work end-to-end by exercising
   interactive sessions and automated tests.
5. When modifying the UI, continue to respect the contract established by `RatatuiCommand` so the
   renderer and transcript stay consistent. New commands should be mirrored in `AnsiRenderer`'s sink.

## Testing the ratatui surface

- `cargo test` — validates unit and integration tests, including transcript utilities and renderer
  helpers.
- `cargo clippy --all-targets --all-features` — catches style regressions, especially around the new
  async loops and conversions.
- Manual smoke test: `cargo run -- ask "test"` to confirm the chat REPL starts, accepts input, streams
  output, and responds correctly to Esc/Ctrl+C without entering fullscreen mode.
- Resize the terminal and scroll through history to ensure layout adjustments and scrolling callbacks
  remain functional.

Following this structure keeps the ratatui integration minimal, preserves the existing run-loop and
business logic, and documents the touch points future contributors should use when extending the UI.
