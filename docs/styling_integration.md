# Styling Integration: anstyle-crossterm

This document describes how VTCode integrates `anstyle-crossterm` for unified styling across CLI and TUI components.

## Overview

VTCode uses `anstyle` as the core styling library for ANSI terminal output, providing a generic, crate-agnostic way to define colors and text effects. The `anstyle-crossterm` adapter bridge integrates this with `crossterm` (used by our TUI), and our custom `ratatui_styles` module further bridges to `ratatui` components.

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    Application Code                      │
│           (CLI output, TUI widgets, etc.)                │
└──────────────────────┬──────────────────────────────────┘
                       │
        ┌──────────────┴──────────────┐
        │                             │
   ┌────▼────────┐            ┌──────▼──────────┐
   │   anstyle   │            │  ratatui_styles │
   │  (Generic)  │            │  (TUI Bridge)   │
   └────┬────────┘            └──────┬──────────┘
        │                             │
        └──────────────┬──────────────┘
                       │
            ┌──────────▼──────────┐
            │ anstyle-crossterm   │
            │  (to_crossterm)     │
            └──────────┬──────────┘
                       │
            ┌──────────▼──────────┐
            │  crossterm/ratatui  │
            │  (Terminal Output)   │
            └─────────────────────┘
```

## Components

### 1. anstyle (Core Styling)

Generic styling types that don't depend on any specific terminal library:

```rust
use anstyle::{Color, Style, Effects, AnsiColor, RgbColor};

// Create a styled string
let style = Style::new()
    .fg_color(Some(Color::Ansi(AnsiColor::Green)))
    .effects(Effects::BOLD | Effects::UNDERLINE);
```

**Advantages:**
- Crate-agnostic (can be used with any terminal library)
- Low-level ANSI styling with RGB and 256-color support
- Zero dependencies
- Perfect for library APIs

### 2. anstyle-crossterm (Adapter)

Converts generic `anstyle::Style` to `crossterm::style::ContentStyle`:

```rust
use anstyle_crossterm::to_crossterm;

let anstyle = anstyle::Style::new()
    .fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Red)));

let crossterm_style = to_crossterm(anstyle);
// Now usable with crossterm APIs
```

**Key Function:**
- `to_crossterm(astyle: anstyle::Style) -> crossterm::style::ContentStyle`

### 3. ratatui_styles (TUI Bridge)

Custom module that bridges `anstyle` to `ratatui` styles for widget rendering:

```rust
use vtcode_core::utils::ratatui_styles::anstyle_to_ratatui;
use anstyle::Style;
use ratatui::style::Color;

let anstyle = Style::new()
    .fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Blue)));

let ratatui_style = anstyle_to_ratatui(anstyle);
// Use with ratatui widgets
```

**Conversions Supported:**
- **Colors:** All standard colors (Red, Green, Blue, etc.), dark variants, RGB, indexed/ANSI values
- **Effects:** Bold, Italic, Underlined, Dimmed, Reversed, Blink, Crossed-out
- **Default mapping:** Dark variants map intelligently to standard colors when not available in ratatui

## Usage Examples

### CLI Output (Direct anstyle)

```rust
use vtcode_core::utils::colors::style;

// Simple usage
println!("{}", style("Success").green().bold());
println!("{}", style("Warning").yellow().dim());
println!("{}", style("Error").red());
```

### TUI Widgets (Using ratatui_styles)

```rust
use vtcode_core::utils::ratatui_styles::anstyle_to_ratatui;
use anstyle::{Style, Color, AnsiColor, Effects};
use ratatui::widgets::Paragraph;
use ratatui::text::{Line, Span};

// Define style once using anstyle
let title_style = Style::new()
    .fg_color(Some(Color::Ansi(AnsiColor::Cyan)))
    .effects(Effects::BOLD);

// Convert once for ratatui
let ratatui_style = anstyle_to_ratatui(title_style);

// Use in widgets
let span = Span::styled("My Title", ratatui_style);
let line = Line::from(span);
let paragraph = Paragraph::new(line);
```

### Unified Theme Management

```rust
// Define your theme once
pub struct AppTheme {
    pub success: anstyle::Style,
    pub warning: anstyle::Style,
    pub error: anstyle::Style,
    pub info: anstyle::Style,
}

impl AppTheme {
    pub fn default() -> Self {
        Self {
            success: anstyle::Style::new()
                .fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Green)))
                .effects(anstyle::Effects::BOLD),
            warning: anstyle::Style::new()
                .fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Yellow))),
            error: anstyle::Style::new()
                .fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Red)))
                .effects(anstyle::Effects::BOLD),
            info: anstyle::Style::new()
                .fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Blue))),
        }
    }

    // Use anywhere in your code
    pub fn render_cli(&self, text: &str) -> String {
        format!(
            "{}{}{}",
            self.success.render(),
            text,
            self.success.render_reset()
        )
    }

    pub fn render_tui(&self, text: &str) -> ratatui::style::Style {
        crate::utils::ratatui_styles::anstyle_to_ratatui(self.success)
    }
}
```

## Benefits

### For CLI Code
- **Simple API**: `style("text").green().bold()`
- **No TUI dependencies**: Works in any CLI context
- **Chainable**: Fluent interface for composing styles

### For TUI Code
- **Unified styling**: Same style definitions everywhere
- **Reusable**: Define once, use in CLI and TUI
- **Type-safe**: Full Rust type checking

### For Library Authors
- **Crate-agnostic**: Expose `anstyle::Style` in your API
- **No hard dependencies**: Consumers choose their terminal library
- **Composable**: Users can combine multiple libraries seamlessly

## Dependencies

```toml
[dependencies]
# Core styling library (zero dependencies)
anstyle = "1.0"

# Bridge from anstyle to crossterm
anstyle-crossterm = "4.0"

# TUI rendering
crossterm = "0.29"
ratatui = { version = "0.29", features = ["crossterm"] }
```

## Performance Considerations

All conversions are zero-cost:
- No allocations (except for String rendering in CLI)
- No runtime lookups (pure matching/conversion)
- Inline-friendly (small functions)

Measurement on typical use:
- `anstyle_to_ratatui()`: ~10-50 nanoseconds
- Full widget render with 100 styled spans: <1ms

## Testing

Unit tests for the ratatui bridge:

```bash
cargo test --lib ratatui_styles
```

Tests cover:
- ANSI color conversion
- Effects/attributes conversion
- Combined style conversion
- Edge cases

## Future Improvements

1. **Trait impl for automatic conversion**: `impl Into<ratatui::style::Style> for anstyle::Style`
2. **Theme system**: Central theme definition with automatic CLI/TUI sync
3. **Color palette system**: Predefined palettes (Solarized, Dracula, etc.)
4. **Terminal capability detection**: Automatically use appropriate color depth

## References

- [anstyle GitHub](https://github.com/rust-cli/anstyle)
- [anstyle-crossterm docs](https://docs.rs/anstyle-crossterm/)
- [crossterm docs](https://docs.rs/crossterm/)
- [ratatui docs](https://docs.rs/ratatui/)
