# vtcode-ui

[Root AGENTS.md](../AGENTS.md) | Unified UI: design system, theme registry, TUI framework. Consolidated from `vtcode-design` and `vtcode-theme`.

## Modules

| Area | Path |
|---|---|
| Design system | `design/` — color conversion, style bridging, layout, diff, panel primitives |
| Theme registry | `theme/` — ThemeStyles, runtime state, syntax theme resolution |
| TUI framework | `tui/` — session, widgets, runner, markdown rendering, config |

## Rules

- `design` and `theme` are re-exported at crate root (`pub use design::*; pub use theme::*`) for backward compatibility with the old standalone crates.
- `publish = false` — internal crate, not published to crates.io.
- `tui/core_tui/` owns the full terminal session lifecycle; `tui/ui/` has reusable widgets (markdown, interactive list).
- `tui/config/constants/` holds TUI-specific defaults — keep them here, not in `vtcode-config`.
- Snapshot tests live in `tui/core_tui/widgets/snapshots/`.

## Gotchas

- `vtcode-commons` provides `anstyle_utils` gated behind a `tui` feature — the style bridging in `design/` depends on it.
- The `crossterm` dependency enables `event-stream` and `osc52` features; do not duplicate these in downstream crates.
