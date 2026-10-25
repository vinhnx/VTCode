# vtcode-desktop

[Root AGENTS.md](../../../AGENTS.md) | macOS GPUI client for the local desktop protocol.

## Conventions

- Render on the UI thread; supervise transport asynchronously. Use GPUI's asynchronous `PathPromptOptions` picker.
- `transcript.rs` owns rows, contiguous activity groups and measured `ListState`; remeasure grouping/expansion changes.
- Use `vtcode-exec-events` runtime types; never duck-type JSON. File changes use bounded `DiffCard` rows.
- `inspector.rs` projects live/restored rows (live IDs win); rebuild changes on upsert, never parse in render.
  Newest valid patch wins; copy its exact source item. Retain one visible `DiffState`.
- Menus/`view/shortcuts.rs` reuse typed actions; desktop buttons share cursor policy. Send uses `submit_on_enter` + `Enter`.
- Gate discovery on validated initialization; serialize session/model changes and capture first-message Send content.
- Palettes (including `view/typography.rs`) retain `CommandState`; focus next frame, defer confirms, consume Cancel.
- Kit TabBar needs scoped focus/arrow/Home/End; SidebarMenu lacks equivalent keyboard support. Keep navigation buttons.
- Theme: brand `primary`, hover `accent`, shared `apply_theme`; Markdown keeps image blocking and Kit `syntect` highlighting.
- Use `aa_muted` and `filled_button_foreground` over palette colors; never hardcode colors over a theme.
- Rich text needs readable accessibility nodes; `view/utilities.rs` reuses projections/actions, never infers subagent state.
- `view/composer_controls.rs` owns anchored controls; service validates options. Keep drafts/sources across option changes.
- Rust 2024 RPIT captures `&mut Context`: render helpers in loops must return `AnyElement`.
- `files.rs`: scan cap 400/depth 8; skip dep/VCS/hidden dirs; UTF-8 preview cap 64 KiB; bounded git lookup.
  Accessible header focuses Kit Tree; selection/expansion survive refresh. Keep I/O
  background/cancellable; reject stale paths, escapes, and truncation; use the shared resolver.
  Session switches reset projections; model/options retain drafts.
- Recheck workspace switching after an asynchronous picker completes; permit only one picker at a time.
- `view/navigation.rs` starts collapsed; Kit buttons own hover callbacks, so use wrappers for reveal handlers.
- Dynamic navigation rows key element IDs by session identity or opaque workspace identity, never list position.
- Bound queues/event drains; reject stale events. CLI reload waits for shutdown and refreshes
  `view/configuration.rs` through the bounded credential-filtered catalog, keeping same-workspace draft sources.
- Retain submissions until typed acknowledgement or runtime proof; never automatically replay uncertain delivery.
- `view/workspace_review.rs`: retain Git navigation by path across refresh; history never replays lifecycle/approvals.
