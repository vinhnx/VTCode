# vtcode-desktop

[Root AGENTS.md](../AGENTS.md) | macOS-only (Apple silicon) GPUI Kit (`=0.8.0`) app; modules: `view/` (screens), `themes/` (tokens), `preferences/`, `transport.rs`, `localization/`.

## Hard rules

- WCAG AA 4.5:1 for foreground + every accent vs background in all themes (`every_theme_*` tests + `cargo nextest run -p vtcode-ui -E 'test(theme)'`); focus indicators get their own contrast check apart from text.
- All user-facing strings localized (`rust_i18n::t!`, 20 locales, args match `%{...}` placeholders); locale data lives per-surface in `locales/app.<ns>.yml` (one line per key, `_version: 2` first line, no trailing whitespace in values); GUI tests use `#[gpui_kit::test]` with stable `ElementId`s, `lock_scroll_axis()` before `.test_support()`, and `set_reduce_motion(true)` for sheet-click flows (opacity fades stay clickable).
- Every action keyboard-reachable with visible focus; overlays dismiss via `dismiss_overlay` paths and restore focus (`*_return_focus`), new overlays must also bind Escape on the topmost surface; icon-only controls carry an accessible name (+ tooltip when scope needs explaining); never encode meaning by color alone; Buttons for in-app commands (`primary` only for the default commitment, `.danger()` for destructive) with result labels (`Delete`, `Cancel`) and `Delete "Name"?` native confirms (`[Destructive, Cancel]`, `matches!(answer.await, Ok(0))`, untestable headless); `Switch` for immediately-applied settings.

## Best practice

- Compose from `view/components.rs` (`desktop_button`, `settings_row`, `settings_group_box`, `accessible_group_box`, `centered_empty_state`, `track_focus`, skeleton lines, shimmer labels, turn pending row); rem scale (`text_sm`, `gap_2`, `p_4`) and component `Size` APIs over `px()` (pixels only for hairlines, platform insets, hit-test tolerances, and fixed pane/region geometry); `truncate()` long/dynamic text; mono font only for code/ids/shortcuts/aligned numbers; load the `gpui-kit` (coding guides) and `gpui-kit-design-guides` (design guides) skills via `read_skill` before UI work, follow their review checklists, and check `docs/development/desktop-component-review.md` before introducing a component (Applied/Retained/Not-needed decisions stand).
- States visible and honest: distinct hover/focus/selected/disabled/loading; spinners in `muted_foreground`; skeletons only while genuinely loading; disable submission in flight; errors scoped near the action (`colors.danger`, `text_sm`); keep views free of service/runtime logic.
- Thin response routers: method match arms delegate to `apply_*`/`fail_*` beside the feature's request builders; `fail_pending_navigation()` on async errors; confirm destructive service calls natively and enforce server-side (service validates, client guards).

## Typography & accessibility

- Hierarchy via `row_title`/`row_description`, `text_sm` body, `text_xs` metadata; `muted_text` for secondary info, `aa_muted` captions-only; semibold reserved for unread/active/emphasis; verify roles/labels in the test tree (`role()`, `label()`); container aria labels need `role(Group)`.
- Type scales with the user's text-size choice: verify zoom at several base fonts (wrapping, truncation, pane minima, virtual-list measurement, focus-ring clearance); longer translations must not clip or shift sibling spines.

## Color

- Theme tokens only: no hex/`rgb()`/`hsla()` outside `themes/`; read `cx.theme()` by semantic role (`background`/`foreground`, `muted`, `primary`, `danger`/`warning`/`success` only for their meanings; `chart_*` for categorical segments); floors: secondary copy targets 6:1 where headroom allows, AA 4.5 enforced, chrome 3:1 (`faded_chrome`, `label_foreground`, `tab_foreground`).
- Derive via `readable_semantic_color`, `readable_indicator_color`, `accessible_focus_color`, `filled_button_foreground`; verify light + dark, never assume fg/bg polarity; sRGB holds on Display P3 via macOS color management (wide-gamut authoring blocked on kit support; `every_theme_stays_within_srgb_gamut` guards it).

## Performance

- Memoize projections per revision/epoch (transcript revision, config catalog epoch, inspector); page long lists (20 rows + `Show more (N remaining)`); virtualize measured rows; remeasure only on typography/theme changes; blocking work via `background_spawn`; no I/O or unbounded loops in render fns.

## Gotchas

- Menu-bar close/Cmd-W share `menu_bar::should_hide` (objc2 AppKit bindings); cross-window refresh goes through `reload_views`; settings sheets swallow cmd-w.
- Storage search keywords are plain substrings; storage mutations refetch usage and review protects the current session; shortcuts persist only on capture; transcript follows THREAD_ID ordering with role labels from `t!`.
