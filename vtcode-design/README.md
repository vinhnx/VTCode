# vtcode-design

Centralized design system for VT Code: unified color conversion, style bridging, design constants, layout, and base widget primitives.

`vtcode-design` is the single source of truth for all design system concerns in VT Code. It consolidates color mapping, style conversion, layout logic, and UI primitives that were previously scattered across `vtcode-commons` and `vtcode-tui`.

## Highlights

- **Correct Color Mapping** — unified `anstyle` to `ratatui` color conversion that fixes prior bugs (Magenta mapped to DarkGray, bright variants mapped to non-bright, Ansi256 mapped to Reset).
- **Style Bridging** — seamless conversion between `anstyle::Style` and `ratatui::style::Style`, including effects/modifier mapping.
- **Responsive Layout** — `LayoutMode` enum (Compact/Standard/Wide) driven by terminal dimensions, controlling borders, titles, sidebar, and footer visibility.
- **Design Constants** — shared UI tokens for ellipses, layout breakpoints, and spacing.
- **Panel Primitive** — base widget that renders standardized chrome (borders, titles) with decoupled styling via `PanelStyleProvider` trait.
- **Diff Formatting** — unified diff rendering with ANSI colors, re-exporting core diff types from `vtcode-commons`.

## Modules

| Module | Purpose |
|---|---|
| `color` | Unified `anstyle` to `ratatui` color mapping (all 16 ANSI colors, 256-color palette, true color) |
| `style` | Style bridging: `anstyle::Style` and `InlineTextStyle` to `ratatui::style::Style`, effects conversion |
| `constants` | Shared UI constants: ellipsis characters, layout breakpoints, spacing tokens |
| `layout` | `LayoutMode` enum with responsive logic based on terminal dimensions |
| `panel` | `Panel` widget primitive with `PanelStyleProvider` and `PanelStyles` traits |
| `diff` | Unified diff formatting with ANSI colors, re-exports core diff types from `vtcode-commons` |

## Public entrypoints

- `anstyle_to_ratatui_color` — canonical color conversion function
- `anstyle_to_ratatui_style` — full style conversion
- `fg_style` / `bg_style` / `fg_bg_style` / `with_effects` / `colored_with_effects` — convenience style builders
- `LayoutMode` — responsive layout mode enum
- `Panel` / `PanelStyleProvider` / `PanelStyles` — panel widget and styling traits
- `format_colored_diff` / `format_unified_diff` / `compute_diff_with_theme` — diff formatting
- `ELLIPSIS` / `ELLIPSIS_CHAR` / `ELLIPSIS_ASCII` — ellipsis constants
- `COMPACT_MAX_COLS` / `WIDE_MIN_COLS` / `SPACING_TIGHT` / `SPACING_NORMAL` / `SPACING_LOOSE` — layout and spacing constants

## Usage

```rust
use vtcode_design::color::anstyle_to_ratatui_color;
use vtcode_design::style::{fg_style, anstyle_to_ratatui_style};
use vtcode_design::layout::LayoutMode;
use vtcode_design::constants::{ELLIPSIS, SPACING_NORMAL};
use anstyle::{AnsiColor, Style as AnstyleStyle};
use ratatui::layout::Rect;

// Convert colors
let ratatui_color = anstyle_to_ratatui_color(anstyle::Color::Ansi(AnsiColor::Magenta));

// Build styles
let style = fg_style(anstyle::Color::Ansi(AnsiColor::Green));

// Responsive layout
let mode = LayoutMode::from_area(Rect::new(0, 0, 120, 30));
assert!(mode.allow_sidebar());
assert!(mode.show_borders());

// Design constants
let truncated = format!("text{}", ELLIPSIS);
```

## Bug fixes over prior implementations

This crate fixes several color mapping bugs from previous implementations in `vtcode-commons::anstyle_utils` and `vtcode-tui::core_tui::style`:

- `Magenta` incorrectly mapped to `DarkGray` (now correctly `Magenta`)
- `BrightMagenta` incorrectly mapped to `DarkGray` (now correctly `LightMagenta`)
- `BrightRed/Green/Yellow/Blue/Cyan` mapped to non-bright variants (now correctly `Light*`)
- `Ansi256` colors mapped to `Reset` instead of `Indexed` (now correctly `Indexed`)

## API reference

See [docs.rs/vtcode-design](https://docs.rs/vtcode-design).

## Related docs

- [Architecture overview](../docs/ARCHITECTURE.md)
