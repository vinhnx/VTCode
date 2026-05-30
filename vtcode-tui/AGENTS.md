# vtcode-tui

[Root AGENTS.md](../AGENTS.md) | Public TUI API surface. Inline terminal UI with crossterm.

## Modules

`app/` app lifecycle | `core/` core TUI state | `core_tui/` implementation (log, panic_hook) | `ui/` theme + widgets | `host/` host integration | `config/` syntax highlighting, keyboard protocol | `options/` session surface settings | `utils/` helpers | `cache/` TUI cache

## Rules

- `core_tui/` is the migrated implementation — `lib.rs` re-exports public API.
- `prelude` module bundles commonly used types.
- `ThemeSuite` + `available_theme_suites()` drive theming — add new themes there.
- Feature gate `tui` on downstream crates that depend on TUI primitives.

## Gotchas

- `cache` and `config` modules are `#[expect(dead_code)]` — internal, not yet public.
- Clippy suppressions in `lib.rs` (`unreachable`, `cast_sign_loss`, `map_err_ignore`) — do not remove.
