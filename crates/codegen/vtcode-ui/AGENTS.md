<!-- markdownlint-disable MD013 -->
# vtcode-ui

[Root AGENTS.md](../../../AGENTS.md) | Unified UI crate (design system, theme registry, TUI framework), consolidated from `vtcode-design` and `vtcode-theme` and not published.

## Modules

- `design/` color conversion, style bridging, layout, diff, and panel primitives; `theme/` ThemeStyles, runtime state, and syntax theme resolution; `vim/` crate-internal modal prompt editing.
- `tui/core_tui/` owns the full terminal session lifecycle: `session/` (actions, layout/render state, modals, diagnostics, `text_utils.rs`), `app/session/` (key events, transcript review, file palette, task panel), `runner/`, `program_status/`, and snapshot tests in `widgets/snapshots/`.
- `tui/ui/` reusable widgets (Markdown, interactive list, `search.rs` UI-only filters); `tui/config/constants/` TUI-only defaults; `tui/session_options.rs` headless host bridge with a bounded, backpressured command queue.

## Rules

- Built-in themes must meet WCAG AA 4.5:1 for foreground and every accent against background; validate with `cargo nextest run -p vtcode-ui -E 'test(theme)'`.
- Style bridging lives only in `design/color.rs` and `design/style.rs`, and downstream code uses the `tui/core_tui/style.rs` wrappers, so never fork new converters.
- Fuzzy matching primitives belong to `vtcode-commons::search` and are only re-exported here, so never fork the matcher.
- Values `vtcode-config` reads live there and are re-exported, while TUI-only knobs stay in `tui/config/constants/`; never redeclare a config-owned name locally or leave a shadowed config copy.
- `UpdateProgress` is presentation-only: runtime guards own accepted operations, the progress row stays out of history/exports/selection, and keyboard/paste handlers must not invent unowned progress.
- `ActivityState` is the authoritative busy/idle signal, and background activity may drive the shimmer but must never set `is_running_activity`.
- Every transcript append path captures `current_max_scroll_offset` then calls `adjust_scroll_after_change`, and batching stays in `append_inline` because per-chunk reflow dominates runtime.
- `transcript_area` is the source of truth for scroll metrics and hit-testing, and modal list hit-testing must use `modal::visible_index_at_row` with the same inputs render used.
- Terminal restore is tracked only after a successful mutation, and render/finalize writes share the terminal-operation lock so no frame reaches main scrollback after restoration.

## Gotchas

- `crossterm` is a maintained fork at `patches/crossterm` that adds `Event::ColorSchemeReport`; re-apply the patch when upgrading and treat it as workspace code under `-D warnings`.
- Modal painting must check `has_active_overlay()` before clearing the frame, otherwise only stale modal hit areas are cleared.
- `Blocked` is quiescent despite its status text, so it must not start spinners or block slash commands.
- Floating overlays own mouse input only inside `modal_list_area`; wheel events outside must pass through so long transcripts stay scrollable.
- Enter has dual funnels (core `session/events.rs` + app `app/session/events/key_actions.rs`): land behavior in both via a shared core helper.
- `take_submitted_input` re-follows the tail even when it returns `None`, so never rely on scroll surviving a take.

Details: [maintainer notes](../../../docs/development/maintainer-notes/vtcode-ui.md) — read the section for the subsystem you change before editing it.
