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
| Chat input loop | Managed by the component-based TUI: `App` converts termion keyboard events into prompt edits, submissions, and scroll actions that surface as `RatatuiEvent` notifications for the agent runtime.【F:vtcode-core/src/ui/tui/app.rs†L94-L163】 |
| Transcript + context trimming | The `Transcript` component owns scroll offsets, indicator rendering, and wrapped line generation so inline updates never disturb the persisted log.【F:vtcode-core/src/ui/tui/components/transcript.rs†L25-L198】 |
| Prompt indicator and placeholder | The `Prompt` component renders the inline prefix, placeholder hint, and cursor offsets while applying theme colors from `RatatuiCommand::SetPrompt` and `SetPlaceholder`.【F:vtcode-core/src/ui/tui/components/prompt.rs†L102-L199】 |
| Tool shell behavior | Unchanged: helpers render into the shared `AnsiRenderer` sink, which writes both to stdout and the ratatui session.【F:src/agent/runloop/unified/shell.rs†L1-L119】 |

## Key components introduced for `ratatui`

1. **`vtcode-core/src/ui/tui.rs`**
   - Exposes the session orchestration API, wiring configuration into `run_tui` and returning the `RatatuiSession`/`RatatuiHandle` pair used by the agent runtime.【F:vtcode-core/src/ui/tui.rs†L1-L49】

2. **`vtcode-core/src/ui/tui/tui.rs`**
   - Builds the termion-backed terminal, spawns the blocking input listener, drains command queues, and schedules redraws within the inline viewport loop.【F:vtcode-core/src/ui/tui/tui.rs†L1-L162】

3. **`vtcode-core/src/ui/tui/app.rs`**
   - Maintains prompt and transcript state, applies `RatatuiCommand`s, and translates user actions into outbound `RatatuiEvent`s for the agent loop.【F:vtcode-core/src/ui/tui/app.rs†L14-L163】

4. **`vtcode-core/src/ui/tui/components/transcript.rs` & `components/prompt.rs`**
   - Provide focused components for rendering the chat history and prompt line, encapsulating cursor math, wrapping, and indicator styling in a reusable layer.【F:vtcode-core/src/ui/tui/components/transcript.rs†L25-L198】【F:vtcode-core/src/ui/tui/components/prompt.rs†L102-L199】

5. **`vtcode-core/src/ui/tui/types.rs`**
   - Declares the shared command/event types, text styling primitives, and session handle API used by both the renderer and higher-level run loop.【F:vtcode-core/src/ui/tui/types.rs†L1-L195】

6. **`AnsiRenderer::with_ratatui`**
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

## Configuration Hooks

- Set `ui_surface = "inline"` under the `[agent]` section in `vtcode.toml` to keep the chat interface
  inline with the terminal scrollback instead of switching to the alternate screen. Use `"alternate"`
  to force fullscreen behavior or leave the default `"auto"` to let VTCode decide based on the
  current stdout surface.

Following this structure keeps the ratatui integration minimal, preserves the existing run-loop and
business logic, and documents the touch points future contributors should use when extending the UI.
