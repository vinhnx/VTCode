# vtcode-desktop

[Root AGENTS.md](../../../AGENTS.md) | macOS GPUI client for the local desktop protocol.

- Render on the UI thread; supervise transport asynchronously. Use GPUI's asynchronous `PathPromptOptions` picker.
- `transcript.rs` owns rows/groups and queues measured-layout changes; `MessageScrollerState` owns virtual-list geometry.
- Use `vtcode-exec-events` runtime types; file changes use bounded `DiffCard` rows.
  Inspector projections reuse live/restored rows; rebuild changes on upsert.
- Menus and `view/shortcuts.rs` reuse typed actions; buttons share cursor policy. Send uses `submit_on_enter` + `Enter`.
- Gate discovery on validated initialization; serialize session/model changes and capture first-message Send content.
- Compact model picks use optional CLI ranking; full search uses the catalog.
  Fast mode follows the validated CLI route and persists through the service.
- Palettes retain `CommandState` and shared immutable entries; guard callbacks by originating entity.
  Focus next frame and consume Cancel. Kit TabBar needs scoped focus and arrow/Home/End navigation.
- Theme uses brand `primary`, hover `accent`, and shared `apply_theme`; Markdown blocks images and uses Kit `syntect` highlighting.
- Localization uses app-owned `app` keys in `locales/ui.yml`; notify the view after locale changes.
- Appearance scrolling locks axes; typography follows active profiles, then becomes shared when separation is disabled.
- Use `aa_muted` (metadata only), `label_foreground` (headings/labels/tabs), and `filled_button_foreground` over palette colors; never hardcode colors over a theme. App-owned labels use medium weight at minimum; Kit `GroupBox` titles need the shared `group_box_title` wrapper.
- Rich text needs readable accessibility nodes; `view/utilities.rs` reuses projections/actions
  and never infers subagent state.
- `view/composer_controls.rs` owns anchored controls; the service validates CLI-discovered modes
  and persists the CLI default. Save workspace permission choices only after acknowledgement;
  restore before Send. Keep drafts/sources across changes.
- Rust 2024 render helpers in loops must return `AnyElement` when they capture `&mut Context`.
