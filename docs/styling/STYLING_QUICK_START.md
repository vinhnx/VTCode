# Styling Quick Start Guide

## For CLI Output

Use the simple chainable API:

```rust
use vtcode_core::utils::colors::style;

// Basic colors
println!("{}", style("text").green());
println!("{}", style("text").red().bold());
println!("{}", style("text").yellow().dim());

// RGB colors
println!("{}", style("text").rgb(r, g, b));
```

## For TUI Widgets

Convert anstyle to ratatui styles:

```rust
use anstyle::{Style, Color, AnsiColor, Effects};
use vtcode_core::utils::ratatui_styles::anstyle_to_ratatui;
use ratatui::text::Span;

let anstyle = Style::new()
    .fg_color(Some(Color::Ansi(AnsiColor::Blue)))
    .effects(Effects::BOLD);

let ratatui_style = anstyle_to_ratatui(anstyle);
let span = Span::styled("Text", ratatui_style);
```

## Unified Theme

Define once, use everywhere:

```rust
use anstyle::{Style, Color, AnsiColor, Effects};

pub struct Theme {
    pub title: Style,
    pub success: Style,
    pub error: Style,
}

impl Theme {
    pub fn default() -> Self {
        Self {
            title: Style::new()
                .fg_color(Some(Color::Ansi(AnsiColor::Cyan)))
                .effects(Effects::BOLD),
            success: Style::new()
                .fg_color(Some(Color::Ansi(AnsiColor::Green))),
            error: Style::new()
                .fg_color(Some(Color::Ansi(AnsiColor::Red)))
                .effects(Effects::BOLD),
        }
    }
}

// Use in CLI
let theme = Theme::default();
println!("{}", format!("{}{}{}", 
    theme.success.render(), 
    "Done", 
    theme.success.render_reset()
));

// Use in TUI
use vtcode_core::utils::ratatui_styles::anstyle_to_ratatui;
let span = Span::styled("Done", anstyle_to_ratatui(theme.success));
```

## Color Reference

### Standard Colors
- `AnsiColor::Black`
- `AnsiColor::Red`
- `AnsiColor::Green`
- `AnsiColor::Yellow`
- `AnsiColor::Blue`
- `AnsiColor::Magenta`
- `AnsiColor::Cyan`
- `AnsiColor::White`

### Effects
- `Effects::BOLD`
- `Effects::ITALIC`
- `Effects::UNDERLINE`
- `Effects::DIMMED`
- `Effects::INVERT`
- `Effects::BLINK`
- `Effects::STRIKETHROUGH`

### Color Types
```rust
use anstyle::Color;

Color::Ansi(AnsiColor::Red)           // Standard ANSI color
Color::Ansi(AnsiColor::BrightRed)     // Bright ANSI color
Color::Rgb(255, 0, 0)                 // RGB color
Color::Ansi256(196)                   // 256-color indexed
```

## Common Patterns

### Error Messages
```rust
use vtcode_core::utils::colors::style;
eprintln!("{}", style("Error: ").red().bold());
eprintln!("{}", style(message).red());
```

### Status Display
```rust
use anstyle::{Style, Color, AnsiColor};

let success = Style::new()
    .fg_color(Some(Color::Ansi(AnsiColor::Green)));
let failed = Style::new()
    .fg_color(Some(Color::Ansi(AnsiColor::Red)));

println!("Status: {}{}{}", success.render(), "OK", success.render_reset());
```

### TUI List Item
```rust
use vtcode_core::utils::ratatui_styles::anstyle_to_ratatui;
use ratatui::text::{Line, Span};

let label_style = anstyle_to_ratatui(theme.title);
let value_style = anstyle_to_ratatui(theme.success);

let line = Line::from(vec![
    Span::styled("Label: ", label_style),
    Span::styled("value", value_style),
]);
```

## Tips

1. **Define styles once** - Create a central `Theme` struct
2. **Reuse styles** - Pass around the same `Style` objects
3. **Convert once** - Call `anstyle_to_ratatui()` once per render
4. **Layer styles** - Combine color, effects, and backgrounds
5. **Test with real terminal** - Terminal capabilities vary

## See Also

- `docs/styling_integration.md` - Detailed guide
- `docs/ANSTYLE_INTEGRATION_SUMMARY.md` - Technical summary
- `vtcode-core/examples/anstyle_ratatui_example.rs` - Full working example
- `vtcode-core/src/utils/ratatui_styles.rs` - Module documentation

## Running the Example

```bash
cd vtcode-core
cargo run --example anstyle_ratatui_example
# Press 'q' to quit
```

Demonstrates:
- CLI colored output
- TUI widget styling
- Unified theme management
- Interactive terminal UI
