# Inline UI session architecture

The interactive terminal experience now relies on a lightweight inline renderer
built on top of `crossterm`, `anstyle`, and the shared `AnsiRenderer` sink. The
current API mirrors the legacy surface, but the internals are fully implemented
with the crossterm inline session so the agent runtime remains decoupled from the
presentation layer.

## Core components

| Responsibility | Location | Notes |
| --- | --- | --- |
| Session bootstrap + renderer ownership | `spawn_session` spawns a new inline session and returns the `InlineHandle`/event pair. | The handle now drives the crossterm-based renderer through the inline session entrypoint.【F:vtcode-core/src/ui/tui.rs†L20-L49】 |
| Streaming response rendering | `AnsiRenderer::with_inline_ui` forwards structured output to the inline sink while keeping the transcript file in sync.【F:vtcode-core/src/utils/ansi.rs†L72-L235】 |
| Input loop | `Session::handle_event` translates crossterm key events into prompt edits, submissions, and scroll actions that surface as `InlineEvent` messages.【F:vtcode-core/src/ui/tui/session.rs†L183-L303】 |

## Rendering pipeline

1. `spawn_session` wires configuration into `run_tui`, which establishes the
   crossterm raw-mode surface and spins up the input listener.
2. `Session::handle_command` mutates transcript and prompt state in response to
   commands from the agent loop, marking the session dirty whenever a redraw is
   needed.【F:vtcode-core/src/ui/tui/session.rs†L58-L117】
3. `Session::render` clears the configured viewport, replays the visible
   transcript, and redraws the prompt with placeholder styling before
   positioning the cursor based on the prompt buffer.【F:vtcode-core/src/ui/tui/session.rs†L216-L318】
4. `AnsiRenderer::with_inline_ui` allows all high-level output helpers to write
   both to stdout and the inline session without duplicating rendering logic.
5. The agent runtime continues to listen for `InlineEvent` values, enabling
   submission, cancellation, and scrolling with minimal changes to the control
   flow.【F:src/agent/runloop/unified/turn.rs†L821-L900】

This architecture keeps the inline UI simple while preserving the ergonomic API
surface that other parts of the codebase depend on.

## Status line customization

Use the `[ui.status_line]` table to control the prompt status bar. The default
`auto` mode shows git status and the active model, while `hidden` removes the
bar entirely. Setting `mode = "command"` runs a user script and renders the
first line of stdout, allowing full customization of the inline footer. See
[`status-line.md`](./status-line.md) for payload details and examples.
