# vtcode-design

Centralized design system for VT Code: color conversion, style bridging, design constants, layout, and base widget primitives.

## Conventions

- All color types use `anstyle` as the canonical representation. Convert to `ratatui::style::Color` at the widget boundary only.
- Design constants (spacing, border widths, icon sets) live in `constants.rs`. Do not hardcode values in widget code.
- Style bridge helpers (`StyleBridge`) convert `anstyle::Style` to `ratatui::style::Style`. Use these instead of manual conversions.
- Base widgets in `widgets/` are pure layout primitives with no state or event handling.

## Dependencies

- `anstyle` (canonical color/style types)
- `ratatui` (TUI rendering types)
- `unicode-width` (text measurement)
- `vtcode-commons` (shared utilities)
