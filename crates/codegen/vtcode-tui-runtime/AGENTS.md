# vtcode-tui-runtime

[Root AGENTS.md](../../../AGENTS.md) | TUI/ANSI rendering stack extracted from `vtcode-core`: theming, markdown-to-ANSI, transcript presentation, inline-session protocol, and terminal setup.

## Conventions

- This crate is the TUI boundary: it depends on `vtcode-core`, never the reverse. Do not add a dependency from `vtcode-core` to `vtcode-ui` or a `tui` feature back to core.
- `vtcode-core` has no `ui`/`terminal_setup` module and no `tui` feature. Rendering types belong here; the slash catalog lives in `vtcode-skills::command_slash` and dependency-free helpers (fuzzy search, terminal detection) in `vtcode-commons`.
- Always compile with `cargo check --workspace` and `cargo check -p vtcode-core --no-default-features` (headless consumers must stay UI-free).
- Preserve the public paths `vtcode_tui_runtime::ui::*`, `::utils::ansi::*`, and `::terminal_setup::*`.
- Terminal icon artwork ships in `assets/` and is included directly by `include_bytes!`; no build script or `OUT_DIR` indirection.

## Dependencies

- `vtcode-ui` — full TUI protocol and rendering implementations.
- `vtcode-core` — config, skills, and terminal detection shared with headless builds.
- `vtcode-commons` — shared UI protocol types and diagnostics.
- `ratatui`/`ansi-to-tui`/`crossterm` — ANSI parsing and terminal control.
