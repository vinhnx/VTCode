# vtcode-ui

Unified UI framework for VT Code: design system, theme registry, and TUI framework.

## Overview

`vtcode-ui` consolidates the design system, theme registry, and terminal UI framework into a single crate. It provides
the public UI-facing API surface for downstream consumers while keeping host-specific integrations inside `vtcode-core`.

## Architecture

| Area           | Path      | Description                                                      |
| -------------- | --------- | ---------------------------------------------------------------- |
| Design system  | `design/` | Color conversion, style bridging, layout, diff, panel primitives |
| Theme registry | `theme/`  | ThemeStyles, runtime state, syntax theme resolution              |
| TUI framework  | `tui/`    | Session, widgets, runner, markdown rendering, config             |

## Key Components

### Design System

- Color conversion utilities (RGB, HSL, hex)
- Style bridging between terminal and application styles
- Layout primitives for TUI composition
- Diff visualization components

### Theme Registry

- `ThemeStyles` — runtime theme state management
- Syntax highlighting theme resolution
- Theme preview functionality
- Default and custom theme support

### TUI Framework

- `tui/core_tui/` — full terminal session lifecycle
- `tui/ui/` — reusable widgets (markdown, interactive list)
- `tui/config/constants/` — TUI-specific defaults
- Snapshot tests in `tui/core_tui/widgets/snapshots/`

## Usage

The crate is re-exported at the root for backward compatibility:

```rust
pub use design::*;
pub use theme::*;
```

## Notes

- Internal crate (`publish = false`) — not published to crates.io
- Style bridging is centralized in `design/color.rs` + `design/style.rs` (crate-internal)
- `crossterm` dependency enables `event-stream` and `osc52` features

## See Also

- [Architecture Guide](../ARCHITECTURE.md) — TUI architecture section
- `vtcode-core::ui::tui` — canonical runtime type surface
