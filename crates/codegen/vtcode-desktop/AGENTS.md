<!-- markdownlint-disable MD013 -->
# vtcode-desktop

[Root AGENTS.md](../../../AGENTS.md) | macOS-only (Apple silicon) GPUI Kit (`=0.8.1`) desktop app.

## Modules

- `src/desktop/view/` screens, overlays, and routing (`components.rs` + `glass_buttons.rs` shared builders, plus per-dialog modules); `appearance_themes.rs` owns custom-theme editing; Kit host: `settings_kit.rs`.
- `src/desktop/files/` workspace snapshot, fuzzy search, and launch/mutate (`launch.rs` fixed-argv external launches); `themes/` color tokens (`custom.rs` user + `glass.rs` glass themes); `preferences/`; `transport.rs`.
- `src/desktop/transcript/` transcript projections: turn grouping lives in `turn.rs`, user-echo attachments in `user_attachments.rs`, `view/` only renders it.
- `src/desktop/output_tail.rs` owns bounded raw, inspector, and compact transcript output projections.
- `src/desktop/localization/` plus per-surface `locales/app.<ns>.yml`; `app_icon.rs` artwork and Dock; `assets.rs` extra icons; `build_info.rs` and `startup.rs` About stamp and argv/URL handoff.

## Rules

- Every theme meets WCAG AA 4.5:1 for foreground and every accent against background, with focus indicators checked separately (`every_theme_*` tests plus `cargo nextest run -p vtcode-ui -E 'test(theme)'`).
- Use theme tokens by semantic role only, never hex/`rgb()`/`hsla()` outside `themes/`, never meaning by color alone, and verify both light and dark without assuming polarity.
- All user-facing strings go through `rust_i18n::t!` across all 20 locales, and new strings stay English literals unless full translations ship for every locale.
- Every action is keyboard-reachable with visible focus, and overlays dismiss through `dismiss_overlay` paths, bind Escape on the topmost surface, and restore focus.
- Destructive actions use a native `Delete "Name"?` confirm and are enforced server-side, while Full-access consent uses the checkbox-gated in-app modal with a one-shot attestation.
- Keep views free of service/runtime logic, with thin response routers delegating to `apply_*`/`fail_*` beside each feature's request builders.
- No I/O or unbounded loops in render functions; memoize projections per revision, page long lists, and run blocking work via `background_spawn`.
- External launches stay behind the fixed-argv boundary in `files/launch.rs`, and the app bundle must never declare `NSIsTouchNative`.
- Session changes during a running turn route through the running-confirm modal, and every `running = false` writer must drain or drop the armed wait.

## Gotchas

- Icons outside Kit's default bundle must join `ExtraIcons` and `DESKTOP_ICONS` in `assets.rs`, or they render blank with no headless failure.
- `app_owns_titlebar_drag` disables all native drag, so zen mode needs its own drag strip, and headless `start_window_move` is unimplemented (guard with `#[cfg(not(test))]`).
- `.test_support()` must precede `.track_focus()` or headless `focused()` panics, and `on_next_frame` never runs headless.
- `process_drained_event` runs `notify_*` hooks before `handle_backend_event`; the hooks read pending state the handler is about to clear, so never reorder them.
- Kit `Badge` is an overlay count, not a text pill — use `Tag::secondary().outline().small()` for pills, wrapped in an identified `div` since `RenderOnce` cannot carry `.test_support()`.
- The slash catalog is service-owned, so every new `CommandSkillSpec` auto-appears in the desktop popup and needs a native route or documented fallthrough.
- Plan rail/edit locking, custom themes, dismiss guards, and login items live in the [maintainer notes](../../../docs/development/maintainer-notes/vtcode-desktop.md#gotchas).
- `components.rs`, `composer_actions.rs`, `shortcuts.rs` are at the 1000-line ceiling (menus in `menu_bar.rs`, IDs in `view/ids.rs`): put new code in a focused module.
- Plain `Enter` while scrolled detours through `submit_or_jump_to_latest` (jump first, draft kept); the Send button always submits immediately.

Details: [maintainer notes](../../../docs/development/maintainer-notes/vtcode-desktop.md) — read the section for the subsystem you change before editing it.
