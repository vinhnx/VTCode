# vtcode-theme

Theme definitions and color constants for VT Code TUI surfaces.

## Conventions

- Theme colors are defined as `anstyle::Style` constants. Convert to ratatui styles at the boundary via `vtcode-design` bridges.
- Each theme is a struct implementing the `Theme` trait. Do not use global state for theme selection.
- Keep theme definitions declarative -- no logic, just color/layout mappings.
