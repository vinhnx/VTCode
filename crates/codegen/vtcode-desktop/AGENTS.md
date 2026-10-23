# vtcode-desktop

[Root AGENTS.md](../../../AGENTS.md) | macOS GPUI client for the local desktop protocol.

## Conventions

- Keep GPUI rendering on the UI thread; transport and process supervision run asynchronously.
- Use GPUI's asynchronous `PathPromptOptions` picker; a blocking native panel can abort the GPUI app.
- `transcript.rs` owns row data and measured `ListState`; notify it of inserts, removals, text changes and expansion.
- Match `vtcode-exec-events` types for runtime items; never duck-type event JSON. File changes render as
  `DiffCard` rows; diff text is bounded before rendering.
- `inspector.rs` owns derived projections (changes, activity) over the transcript; rebuild the changes
  projection on diff upserts, never parse patches during render. Build `DiffState` entities on click only.
- The composer submits through `submit_on_enter` plus the propagated `Enter` action on its wrapper;
  `cx.subscribe`-style handlers lack a window, action handlers have one.
- The ⌘K palette creates its `CommandState` on open and focuses it on the next frame (the kit installs
  its input state during the first render); confirm callbacks are deferred, so tests need `run_until_parked`.
- `files.rs` bounds the workspace scan (400 entries, depth 3, skip dependency/VCS/hidden dirs),
  previews (64 KiB, no binary), and the git branch walk; the sidebar never blocks on unbounded I/O.
- Recheck workspace switching after an asynchronous picker completes; permit only one picker at a time.
- Drain a bounded event batch per foreground tick so streaming cannot starve input.
- Keep transport, UI-event, and host-command queues bounded; preserve protocol-issued
  session, turn, interaction, and choice identities.
- Render only data received from the local service; agent execution remains in the unified runloop.
- Build and package with the commands in `docs/development/desktop-workspace.md`.
