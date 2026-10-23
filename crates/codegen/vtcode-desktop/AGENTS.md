# vtcode-desktop

[Root AGENTS.md](../../../AGENTS.md) | macOS GPUI client for the local desktop protocol.

## Conventions

- Keep GPUI rendering on the UI thread; transport and process supervision run asynchronously.
- Use GPUI's asynchronous `PathPromptOptions` picker; a blocking native panel can abort the GPUI app.
- `transcript.rs` owns row data and measured `ListState`; notify it of inserts, removals, text changes and expansion.
- Use Kit `TextView` for Markdown, preserve raw copy text, and keep transcript images and non-web URL schemes blocked.
- Recheck workspace switching after an asynchronous picker completes; permit only one picker at a time.
- Drain a bounded event batch per foreground tick so streaming cannot starve input.
- Keep transport, UI-event, and host-command queues bounded; preserve protocol-issued
  session, turn, interaction, and choice identities.
- Render only data received from the local service; agent execution remains in the unified runloop.
- Build and package with the commands in `docs/development/desktop-workspace.md`.
