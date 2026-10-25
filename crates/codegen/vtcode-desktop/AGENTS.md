# vtcode-desktop

[Root AGENTS.md](../../../AGENTS.md) | macOS GPUI client for the local desktop protocol.

- Render on the UI thread; supervise transport asynchronously. Use GPUI's asynchronous `PathPromptOptions` picker.
- `transcript.rs` owns rows/groups and queues measured-layout changes; `MessageScrollerState` owns virtual-list geometry.
- Use `vtcode-exec-events` runtime types; never duck-type JSON. File changes use bounded `DiffCard` rows.
- `inspector.rs` projects live/restored rows (live IDs win); rebuild changes on upsert, never parse in render.
  Newest valid patch wins; copy its exact source item. Retain one visible `DiffState`.
- Menus/`view/shortcuts.rs` reuse typed actions; desktop buttons share cursor policy. Send uses `submit_on_enter` + `Enter`.
- Gate discovery on validated initialization; serialize session/model changes and capture first-message Send content.
  Compact model picks use the optional CLI rank; full search remains the entire catalog.
- Palettes (including `view/typography.rs`) retain `CommandState`; focus next frame, defer confirms, consume Cancel.
- Kit TabBar needs scoped focus/arrow/Home/End; SidebarMenu lacks equivalent keyboard support. Keep navigation buttons.
- Theme: brand `primary`, hover `accent`, shared `apply_theme`; Markdown keeps image blocking and Kit `syntect` highlighting.
- Appearance scrolling locks axes; typography follows active profiles, then becomes shared when separation is disabled.
- Use `aa_muted` and `filled_button_foreground` over palette colors; never hardcode colors over a theme.
- Rich text needs readable accessibility nodes; `view/utilities.rs` reuses projections/actions, never infers subagent state.
- `view/composer_controls.rs` owns anchored controls; service validates options. Keep drafts/sources across option changes.
- Rust 2024 RPIT captures `&mut Context`: render helpers in loops must return `AnyElement`.
- `files.rs`: scan cap 400/depth 8; skip dep/VCS/hidden dirs; UTF-8 preview cap 64 KiB; bounded Git lookup.
  Accessible Kit Tree preserves selection/expansion on refresh; cancel stale I/O and reject escapes/truncation.
  Use the shared resolver; session switches reset projections; model/options retain drafts.
- Recheck workspace switching after an asynchronous picker completes; permit only one picker at a time.
- `view/navigation.rs` starts collapsed; Kit buttons own hover callbacks, so use wrappers for reveal handlers.
- Stable row IDs use session/path identity; restored history uses session/content plus duplicate occurrence.
- Bound queues; reject stale events, including primary-agent notifications. Keep mode selection next-session only.
  Reload bounded credential-filtered CLI settings after shutdown; retain drafts and never replay uncertain delivery.
- `view/workspace_review.rs`: retain Git navigation by path across refresh; history never replays lifecycle/approvals.
