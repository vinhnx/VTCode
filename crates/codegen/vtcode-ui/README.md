# vtcode-ui

Unified UI crate for VT Code: design system, theme registry, and TUI framework.

## Overview

This crate consolidates the UI layer for VT Code, providing:

- **Design system** -- shared color tokens, typography, and spacing primitives
- **Theme registry** -- runtime theme switching with built-in palettes (Catppuccin, custom)
- **TUI framework** -- terminal rendering, input handling, and widget primitives built on [Ratatui](https://ratatui.rs)

<!-- cargo-rdme start -->

Unified UI crate for VT Code: design system, theme registry, and TUI framework.

### Module layout

- `design` — Color conversion, style bridging, layout, diff, panel primitives
- `theme` — Theme registry, runtime state, syntax theme resolution
- `tui`   — Full TUI framework (session, widgets, runner, markdown, etc.)

Items from `design` and `theme` are also re-exported at the crate root for
backward-compatibility with callers that previously imported from the
standalone `vtcode-design` / `vtcode-theme` crates (now consolidated into `vtcode-ui`).

<!-- cargo-rdme end -->

## Crate Structure

```
src/
  design/     -- design tokens, color system, typography
  theme/      -- theme registry and built-in palettes
  tui/        -- terminal backend, input handling, rendering loop
  widgets/    -- reusable TUI widgets (markdown, fuzzy picker, status bar, etc.)
```

## Usage

Add to your `Cargo.toml`:

```toml
vtcode-ui = { path = "../vtcode-ui" }
```

## License

See the workspace `LICENSE` file for details.
