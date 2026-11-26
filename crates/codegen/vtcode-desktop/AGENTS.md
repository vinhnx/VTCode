<!-- markdownlint-disable MD013 -->
# vtcode-desktop

[Root AGENTS.md](../../../AGENTS.md) | macOS-only (Apple silicon) GPUI Kit desktop app.

## Modules

- `src/desktop/view/` screens, overlays, routing; `components/` shared builders; `transcript_command.rs` commands; appearance modules edit/select themes; `settings_kit.rs` settings.
- `src/desktop/files/` workspace snapshot, fuzzy search, launch/mutate (`launch.rs` fixed-argv); `themes/` tokens (`custom.rs`, `fills.rs` ramps); `preferences/`; `transport.rs`.
- `src/desktop/transcript/` projections: rows in `item.rs`, turns in `turn.rs`, user-echo attachments in `user_attachments.rs`; `view/` only renders.
- `src/desktop/output_tail.rs` owns bounded raw, inspector, and compact output projections.
- `src/desktop/terminal_{process,program_status,screen,tab}.rs`, `view/terminal_panel.rs`, `preferences/terminal.rs`, `themes/terminal.rs`: embedded user-shell PTY (scrollback, ANSI, drag-select).
- `src/desktop/localization/` + per-surface `locales/app.<ns>.yml`; `app_icon.rs` artwork/Dock; `assets.rs` extra icons; `build_info.rs` About stamp; `startup.rs` argv/URL handoff.

## Rules

- Every theme meets WCAG AA 4.5:1 for foreground and every accent against background (filled labels: softened `#171C26`/`#CCCCCC`; ring token transparent; `every_theme_*` tests + `cargo nextest run -p vtcode-ui -E 'test(theme)'`).
- Use theme tokens by semantic role only — never hex/`rgb()`/`hsla()` outside `themes/`, never meaning by color alone; verify light and dark without assuming polarity.
- All user-facing strings go through `rust_i18n::t!` across all 20 locales; new strings stay English literals unless every locale gets full translations.
- Every action is keyboard-reachable (no focus-ring borders by policy; the caret carries focus visibility); overlays dismiss through `dismiss_overlay`, bind Escape on the topmost surface, and restore focus.
- Destructive actions use a native `Delete "Name"?` confirm enforced server-side; Full-access consent uses the checkbox-gated in-app modal with a one-shot attestation.
- Views stay free of service/runtime logic; thin response routers delegate to `apply_*`/`fail_*` beside each feature's request builders.
- Transcript titles/details/labels delegate to `vtcode-commons::tool_headlines`; categories, collapse, renderers stay local — never mirror.
- No I/O or unbounded loops in render; memoize projections per revision, page long lists, run blocking work via `background_spawn`.
- External launches stay behind the fixed-argv boundary in `files/launch.rs`, and the app bundle must never declare `NSIsTouchNative`.
- Session changes during a running turn route through running-confirm; every `running = false` writer must drain or drop the armed wait.

## Gotchas

- Non-default icons must join `ExtraIcons` in `assets.rs` (blank, no headless failure); a source-scan test rejects unregistered `IconName::`.
- Composer popups mount at the composer panel, never inside a trigger pill subtree (focus traps detach); they anchor to `composer_trigger_pill` bounds via `anchored().attach` (Kit `open` is paint-free).
- Keep the window interactive during startup. The `INITIALIZE` request gates backend actions and drives inline connection status.
- `app_owns_titlebar_drag` disables all native drag (zen needs its own drag strip); headless `start_window_move` is unimplemented (guard `#[cfg(not(test))]`).
- `.test_support()` must precede `.track_focus()` or headless `focused()` panics; `on_next_frame` never runs headless.
- `process_drained_event` runs `notify_*` hooks before `handle_backend_event`; hooks read state the handler clears, so never reorder them.
- Never rebuild rio-vt palettes per frame (uncached regex); tables are statics, rows virtualize, custom buttons fix height.
- Transcript rows share one title spine (`themes/layout.rs` tokens); never retier one row alone. Plan rail, themes, dismiss guards, login, handoff, ceilings: [maintainer notes](../../../docs/development/maintainer-notes/vtcode-desktop.md#gotchas).

Details: [maintainer notes](../../../docs/development/maintainer-notes/vtcode-desktop.md) — read the relevant section (button roles for UI changes)
