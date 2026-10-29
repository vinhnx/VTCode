# vtcode-desktop

[Root AGENTS.md](../../../AGENTS.md) | macOS GPUI client for the local desktop protocol.

- Render on the UI thread; supervise transport asynchronously. Use GPUI's asynchronous `PathPromptOptions` picker.
- `transcript.rs` owns rows/groups and queues measured-layout changes; `MessageScrollerState` owns virtual-list geometry. Hidden-only archive pages advance the cursor and keep earlier history reachable. Row roles/statuses are typed (`TranscriptRole`, `ActivityState::from_status`); cross-file strings live in `strings.rs`, test fixtures keep literals as oracles.
- Use `vtcode-exec-events` runtime types; file changes use bounded `DiffCard` rows.
  Inspector projections reuse live/restored rows; rebuild changes on upsert.
- Menus and `view/shortcuts.rs` reuse typed actions; shortcut ids are a private `CommandId` table. Buttons share cursor policy. Send uses `submit_on_enter` + `Enter`.
- Gate discovery on validated initialization; serialize session/model changes and capture first-message Send content.
- Compact model picks use optional CLI ranking; full search uses the catalog.
  Fast mode follows the validated CLI route and persists through the service.
- Palettes retain `CommandState` and shared immutable entries; guard callbacks by originating entity.
  Focus next frame and consume Cancel. Kit TabBar needs scoped focus and arrow/Home/End navigation; segmented selection is a canvas-colored pill on the track, hover shifts label color only.
- Theme uses brand `primary`, hover `accent`, and shared `apply_theme`; Markdown blocks images and uses Kit `syntect` highlighting. Syntax *token* colors stay on the Kit mode default (`ThemeStyle` is unconstructible in Kit 0.8.0; pinned by `code_token_colors_follow_the_kit_mode_default`); only code-block surfaces follow the palette.
- Localization uses app-owned `app` keys in `locales/ui.yml`; notify the view after locale changes.
- Appearance scrolling locks axes; typography follows active profiles, then becomes shared when separation is disabled.
- Use `aa_muted` (captions/metadata only), `muted_text` (row descriptions/hints, 6:1 target), `label_foreground` (headings/labels), `faded_chrome` (quiet chrome icons, 3:1 floor), and `filled_button_foreground` over palette colors; tab labels use the dedicated `tab_foreground`/`tab_bar_segmented` tokens; never hardcode colors over a theme. Titles and body stay regular; semibold is reserved for small headings via `group_box_title`/`section_label` (Kit `GroupBox` titles render muted by default). Settings rows use `row_title`/`row_description` with `row_divider` separators. Compose new screens from `view/components.rs` builders (`settings_group_box`, `settings_row`, `settings_switch`, `centered_empty_state`, `scroll_column`/`scroll_column_flex`, `sheet_panel_height`, `desktop_selected`); check there before adding render helpers.
- GPUI interaction styles are single-assignment: an instance `.hover()` on a kit `Button` panics (`hover style already set`) because the component registers its own. Pick a variant with the wanted states (e.g., `.text()` for fill-free) instead of overriding.
- Rich text needs readable accessibility nodes; `view/utilities.rs` reuses projections/actions
  and never infers subagent state.
- Transcript `notice` rows carry `NoticeSeverity` and render as plain severity rows (no bubble/Markdown/copy); message
  actions reveal on hover in a fixed slot, with the palette copy command as the keyboard path.
- `view/composer_controls.rs` owns anchored controls; the service validates CLI-discovered modes
  and persists the CLI default. Save permission choices only after acknowledgement; restore before Send.
- New `view/` splits use `use super::*` + `impl DesktopView`; cross-module items are `pub(super)`, test-only re-exports `#[cfg(test)]`. GUI tests live in `view/tests/<topic>.rs` (`use super::*;` only); shared helpers and `pub(super) use` imports stay in `tests/mod.rs`.
- Rust 2024 render helpers in loops must return `AnyElement` when they capture `&mut Context`.
- `/config` opens the dedicated page (search, groups rail, value-first rows, trailing notes); deep-links resolve
  path → group → search, runtime sections stay CLI. Sheet slide is wall-clock (sheet-click tests need reduce motion); opacity fades stay clickable.
- Notification decisions needing the window run as pre-handlers in `process_drained_event`; `handle_backend_event` only stores state.
- Unauthenticated services stay connected: submit is blocked with credential guidance while local commands still work. Static-key providers offer an in-app Settings secret form (`config/secret/store`, masked input wiped on send, form-scoped errors); login-flow providers keep Terminal/Reload guidance. Zen empty threads render `zen-welcome` (centered mark + one greeting line), never the full welcome.
