# vtcode-desktop

[Root AGENTS.md](../../../AGENTS.md) | macOS GPUI client for the local desktop protocol.

## Conventions

- Keep GPUI rendering on the UI thread; transport and process supervision run asynchronously.
- Use GPUI's asynchronous `PathPromptOptions` picker; a blocking native panel can abort the GPUI app.
- `transcript.rs` owns row data and measured `ListState`; notify it of inserts, removals, text changes and expansion.
- Match `vtcode-exec-events` types for runtime items; never duck-type event JSON. File changes render as
  `DiffCard` rows; diff text is bounded before rendering.
- The composer submits through `submit_on_enter` plus the propagated `Enter` action on its wrapper;
  `cx.subscribe`-style handlers lack a window, action handlers have one.
- `files.rs` bounds the workspace scan (400 entries, depth 3, skip dependency/VCS/hidden dirs) and
  previews (64 KiB, no binary); the sidebar never blocks on unbounded I/O.
- Recheck workspace switching after an asynchronous picker completes; permit only one picker at a time.
- Drain a bounded event batch per foreground tick so streaming cannot starve input.
- Keep transport, UI-event, and host-command queues bounded; preserve protocol-issued
  session, turn, interaction, and choice identities.
- Render only data received from the local service; agent execution remains in the unified runloop.
- Build and package with the commands in `docs/development/desktop-workspace.md`.
