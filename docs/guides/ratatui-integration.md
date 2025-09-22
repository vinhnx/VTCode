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
| Session bootstrap + renderer ownership | `run_single_agent_loop_unified` spawns a `RatatuiSession` and wires an `AnsiRenderer` sink.【F:src/agent/runloop/unified/turn.rs†L554-L582】 | The session exposes a `RatatuiHandle` for output commands and an event receiver for user input. |
| Streaming response rendering | `AnsiRenderer::stream_markdown_response` forwards segments to the ratatui sink while maintaining the persistent transcript.【F:vtcode-core/src/utils/ansi.rs†L218-L310】 | Streaming replacements are issued through `RatatuiCommand::ReplaceLast` with an explicit `RatatuiMessageKind` so each block retains its styling. |
| Prompt indicator, banner, tool summary | Rendered through existing helpers that now call `AnsiRenderer::with_ratatui` so the UI and transcript stay in sync.【F:src/agent/runloop/unified/display.rs†L1-L49】【F:src/agent/runloop/ui.rs†L1-L43】 |
| Chat input loop | Managed inside `vtcode-core/src/ui/ratatui.rs`. The loop listens for `crossterm` key events, maintains a local input buffer, and emits `RatatuiEvent` values back to the agent.【F:vtcode-core/src/ui/ratatui.rs†L169-L330】 |
| Transcript + context trimming | Managed entirely inside `vtcode-core/src/ui/ratatui.rs`; scroll commands now stay local to the renderer so no additional transcript logging is emitted.【F:vtcode-core/src/ui/ratatui.rs†L820-L918】 |
| Tool shell behavior | Unchanged: helpers render into the shared `AnsiRenderer` sink, which writes both to stdout and the ratatui session.【F:src/agent/runloop/unified/shell.rs†L1-L119】 |

## Key components introduced for `ratatui`

1. **`vtcode-core/src/ui/ratatui.rs`**
   - Defines `RatatuiCommand`, `RatatuiMessageKind`, `RatatuiEvent`, `RatatuiHandle`, and `RatatuiSession`.
   - Manages terminal lifecycle (raw mode, cursor visibility) without entering the alternate screen.
   - Maintains transcript state, prompt styling, placeholder hints, and input editing logic.
   - Draws the chat as stacked message blocks (user, assistant, tool, policy, PTY, info, error) with the
     prompt rendered as the final block so the entire conversation scrolls as one surface.

2. **`AnsiRenderer::with_ratatui`**
   - Wraps a `RatatuiHandle` so all structured output flows through ratatui while continuing to append
     to the persistent transcript log.【F:vtcode-core/src/utils/ansi.rs†L68-L184】
   - Converts `anstyle::Style` data into `ratatui` colors and modifiers to preserve syntax
     highlighting and theming.【F:vtcode-core/src/utils/ansi.rs†L320-L360】

3. **Run loop integration**
   - `run_single_agent_loop_unified` spawns the session, applies the active theme, and routes events
     from `RatatuiEvent` into the existing control flow (scrolling, cancel, interrupt, exit).【F:src/agent/runloop/unified/turn.rs†L554-L720】
   - `PlaceholderGuard` and `PlaceholderSpinner` continue to operate, now delegating to
     `RatatuiHandle::set_placeholder` so progress messages and hints show beneath the prompt.【F:src/agent/runloop/unified/turn.rs†L180-L360】

## Migration and maintenance checklist

1. Remove all references to `iocraft` crates, modules, and docs. Ensure `vtcode-core/Cargo.toml`
   only declares `ratatui` and supporting utilities (e.g., `unicode-width`).【F:vtcode-core/Cargo.toml†L84-L96】
2. Re-export the ratatui surface from `vtcode-core/src/ui/mod.rs` so downstream crates import
   `vtcode_core::ui::ratatui` exclusively.【F:vtcode-core/src/ui/mod.rs†L1-L5】
3. Update call sites to use `AnsiRenderer::with_ratatui` and the new event types, keeping the business
   logic unchanged.【F:src/agent/runloop/unified/turn.rs†L554-L720】
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
