# Styling Refactor Implementation Guide

## Quick Reference: All Styling Issues

### 1. Hardcoded ANSI Codes (Replace with anstyle)

#### Location: `src/agent/runloop/unified/tool_summary.rs:23-97`
```rust
// CURRENT (BAD)
let status_color = if let Some(code) = exit_code {
    if code == 0 { "\x1b[32m" } else { "\x1b[31m" }
} else {
    "\x1b[36m"
};
line.push_str(status_color);
line.push_str("\x1b[0m ");

// GOAL (using style_helpers)
use vtcode_core::utils::style_helpers::{ColorPalette, render_styled};
let palette = ColorPalette::default();
let color = match exit_code {
    Some(0) => palette.success,
    Some(_) => palette.error,
    None => palette.info,
};
let styled = render_styled(&status_icon, color, None);
line.push_str(&styled);
```

**Instances in this file:** 6+ hardcoded codes

#### Location: `src/agent/runloop/unified/turn/session.rs:1666`
```rust
// CURRENT
&format!("\x1b[31m✗\x1b[0m Tool '{}' failed", name),

// GOAL
&format!("{} Tool '{}' failed", 
    render_styled("✗", ColorPalette::default().error, None), 
    name)
```

---

### 2. Repeated Pattern: `.into()` Conversions

#### Location: `src/agent/runloop/tool_output/styles.rs:40-85`
```rust
// CURRENT (repeats 7 times)
AnsiStyle::new()
    .bold()
    .fg_color(Some(AnsiColor::Blue.into())),

AnsiStyle::new()
    .bold()
    .fg_color(Some(AnsiColor::Cyan.into())),

AnsiStyle::new()
    .bold()
    .fg_color(Some(AnsiColor::Green.into())),
```

**Fix Strategy:** Create factory in `style_helpers`:
```rust
pub fn bold_color(color: AnsiColor) -> Style {
    Style::new()
        .bold()
        .fg_color(Some(color.into()))
}

// Usage:
classes.insert("di".to_string(), bold_color(AnsiColor::Blue));
classes.insert("ln".to_string(), bold_color(AnsiColor::Cyan));
classes.insert("ex".to_string(), bold_color(AnsiColor::Green));
```

#### Location: `vtcode-core/src/ui/diff_renderer.rs:22-26`
```rust
// CURRENT
let parse = |spec: &str| -> Style {
    if use_colors {
        match spec {
            "yellow" => Style::new().fg_color(Some(AnsiColor::Yellow.into())),
            "white" => Style::new().fg_color(Some(AnsiColor::White.into())),
            "green" => Style::new().fg_color(Some(AnsiColor::Green.into())),
            "red" => Style::new().fg_color(Some(AnsiColor::Red.into())),
            "cyan" => Style::new().fg_color(Some(AnsiColor::Cyan.into())),
            _ => Style::new(),
        }
    } else {
        Style::new()
    }
};

// GOAL: Reuse centralized function
let parse = |spec: &str| -> Style {
    if use_colors {
        style_helpers::style_from_color_name(spec)
    } else {
        Style::new()
    }
};
```

---

### 3. Magic RGB Values

#### Location: `src/agent/runloop/tool_output/styles.rs:16-29`
```rust
// CURRENT (magic numbers)
AnsiStyle::new()
    .fg_color(Some(Color::Rgb(RgbColor(200, 255, 200))))
    .bg_color(Some(Color::Rgb(RgbColor(0, 64, 0)))),

AnsiStyle::new()
    .fg_color(Some(Color::Rgb(RgbColor(255, 200, 200))))
    .bg_color(Some(Color::Rgb(RgbColor(64, 0, 0)))),
```

**Goal:** Use `DiffColorPalette` (already partially exists in `diff_renderer.rs`)

```rust
// CURRENT in diff_renderer.rs
struct GitDiffPalette {
    stat_added: Style,
    stat_removed: Style,
}

// GOAL: Extract to shared module
pub struct DiffColorPalette {
    pub added_fg: RgbColor,
    pub added_bg: RgbColor,
    pub removed_fg: RgbColor,
    pub removed_bg: RgbColor,
}

impl DiffColorPalette {
    pub fn added_style(&self) -> Style {
        Style::new()
            .fg_color(Some(Color::Rgb(self.added_fg)))
            .bg_color(Some(Color::Rgb(self.added_bg)))
    }

    pub fn removed_style(&self) -> Style {
        Style::new()
            .fg_color(Some(Color::Rgb(self.removed_fg)))
            .bg_color(Some(Color::Rgb(self.removed_bg)))
    }
}
```

**Usage in both files:**
```rust
let palette = DiffColorPalette::default();
let add_style = palette.added_style();
let remove_style = palette.removed_style();
```

---

### 4. Manual Style Construction Chains

#### Location: `src/workspace_trust.rs:70-131`
```rust
// CURRENT (repeated pattern 6 times)
println!(
    "{}",
    Style::new()
        .fg_color(Some(Color::Ansi(anstyle::AnsiColor::Green)))
        .render()
        .to_string()
    + msg
    + &Style::new().render_reset().to_string()
);

// GOAL: Use helper
use vtcode_core::utils::style_helpers::render_styled;

println!("{}", render_styled(msg, ColorPalette::default().success, None));
```

This pattern appears **5 times** in `workspace_trust.rs` with different colors.

---

### 5. Ratatui Hardcoded Colors

#### Location: `src/interactive_list.rs:119-175`
```rust
// CURRENT
Style::default()
    .fg(Color::LightBlue)
    .add_modifier(Modifier::BOLD),

Style::default().fg(Color::Gray),

Style::default().fg(Color::White)

Style::default()
    .fg(Color::Cyan)
    .add_modifier(Modifier::BOLD | Modifier::REVERSED),

Style::default().fg(Color::Gray),
```

**Goal:** Define as constants near top of file or in theme module:

```rust
mod styles {
    use ratatui::style::{Color, Modifier, Style};
    
    pub const ITEM_NUMBER: Style = Style::new()
        .fg(Color::LightBlue)
        .add_modifier(Modifier::BOLD);
    
    pub const DESCRIPTION: Style = Style::new()
        .fg(Color::Gray);
    
    pub const DEFAULT_TEXT: Style = Style::new()
        .fg(Color::White);
    
    pub const HIGHLIGHT: Style = Style::new()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD | Modifier::REVERSED);
}

// Usage:
Span::styled(format!("{:>2}. ", idx + 1), styles::ITEM_NUMBER),
Span::styled(description.clone(), styles::DESCRIPTION),
```

---

### 6. Incomplete Color Mappings

#### Location: `vtcode-core/src/utils/ratatui_styles.rs:122-130`
```rust
// CURRENT (incomplete mapping)
pub fn crossterm_to_ratatui_color(color: CrosstermColor) -> RatatuiColor {
    match color {
        CrosstermColor::Black => RatatuiColor::Black,
        CrosstermColor::Red => RatatuiColor::Red,
        // ... only 10 cases, some missing bright variants
    }
}
```

**Goal:** Complete comprehensive bidirectional mapping

```rust
pub fn ansicolor_to_ratatui(ac: AnsiColor) -> RatatuiColor {
    match ac {
        AnsiColor::Black => RatatuiColor::Black,
        AnsiColor::Red => RatatuiColor::Red,
        AnsiColor::Green => RatatuiColor::Green,
        AnsiColor::Yellow => RatatuiColor::Yellow,
        AnsiColor::Blue => RatatuiColor::Blue,
        AnsiColor::Magenta => RatatuiColor::Magenta,
        AnsiColor::Cyan => RatatuiColor::Cyan,
        AnsiColor::White => RatatuiColor::White,
        AnsiColor::BrightBlack => RatatuiColor::DarkGray,
        AnsiColor::BrightRed => RatatuiColor::LightRed,
        AnsiColor::BrightGreen => RatatuiColor::LightGreen,
        AnsiColor::BrightYellow => RatatuiColor::LightYellow,
        AnsiColor::BrightBlue => RatatuiColor::LightBlue,
        AnsiColor::BrightMagenta => RatatuiColor::LightMagenta,
        AnsiColor::BrightCyan => RatatuiColor::LightCyan,
        AnsiColor::BrightWhite => RatatuiColor::White,
    }
}

pub fn ratatui_to_ansicolor(rc: RatatuiColor) -> Option<AnsiColor> {
    match rc {
        RatatuiColor::Black => Some(AnsiColor::Black),
        RatatuiColor::Red => Some(AnsiColor::Red),
        RatatuiColor::DarkGray => Some(AnsiColor::BrightBlack),
        RatatuiColor::LightRed => Some(AnsiColor::BrightRed),
        // ... etc
        _ => None,
    }
}
```

---

## Implementation Checklist

### New Modules to Create
- [ ] `vtcode-core/src/utils/style_helpers.rs` - Central style factory
- [ ] `vtcode-core/src/utils/diff_styles.rs` - Diff color palette

### Files to Refactor
- [ ] `src/agent/runloop/unified/tool_summary.rs` - Replace ANSI codes
- [ ] `src/agent/runloop/tool_output/styles.rs` - Use `style_from_color_name`
- [ ] `src/workspace_trust.rs` - Use `render_styled` helper
- [ ] `src/interactive_list.rs` - Extract style constants
- [ ] `vtcode-core/src/ui/diff_renderer.rs` - Use `DiffColorPalette`
- [ ] `vtcode-core/src/utils/ratatui_styles.rs` - Complete mappings
- [ ] `vtcode-core/src/utils/colors.rs` - Reuse `style_from_color_name`

### Testing
- [ ] Add tests for `style_helpers` module
- [ ] Add tests for `diff_styles` module
- [ ] Verify color conversions are lossless where possible
- [ ] Test ANSI code generation doesn't regress

---

## Code Template: New style_helpers Module

```rust
// vtcode-core/src/utils/style_helpers.rs

use anstyle::{AnsiColor, Color, Style, Effects, RgbColor};

/// Standard color palette with semantic names
#[derive(Debug, Clone, Copy)]
pub struct ColorPalette {
    pub success: Color,      // Green
    pub error: Color,        // Red
    pub warning: Color,      // Yellow
    pub info: Color,         // Cyan
    pub accent: Color,       // Blue
    pub muted: Color,        // Gray/Dim
}

impl ColorPalette {
    pub fn default() -> Self {
        Self {
            success: Color::Ansi(AnsiColor::Green),
            error: Color::Ansi(AnsiColor::Red),
            warning: Color::Ansi(AnsiColor::Yellow),
            info: Color::Ansi(AnsiColor::Cyan),
            accent: Color::Ansi(AnsiColor::Blue),
            muted: Color::Ansi(AnsiColor::White), // Will be dimmed
        }
    }
}

/// Render text with a single color
pub fn render_styled(text: &str, color: Color, effects: Option<Effects>) -> String {
    let mut style = Style::new().fg_color(Some(color));
    if let Some(e) = effects {
        style = style.effects(e);
    }
    format!("{style}{text}{}", style.render_reset())
}

/// Build style from CSS/terminal color name
pub fn style_from_color_name(name: &str) -> Style {
    let color = match name.to_lowercase().as_str() {
        "red" => Color::Ansi(AnsiColor::Red),
        "green" => Color::Ansi(AnsiColor::Green),
        "blue" => Color::Ansi(AnsiColor::Blue),
        "yellow" => Color::Ansi(AnsiColor::Yellow),
        "cyan" => Color::Ansi(AnsiColor::Cyan),
        "magenta" | "purple" => Color::Ansi(AnsiColor::Magenta),
        "white" => Color::Ansi(AnsiColor::White),
        "black" => Color::Ansi(AnsiColor::Black),
        _ => return Style::new(),
    };
    
    Style::new().fg_color(Some(color))
}

/// Create a bold colored style
pub fn bold_color(color: AnsiColor) -> Style {
    Style::new()
        .bold()
        .fg_color(Some(color.into()))
}

/// Create a dimmed style
pub fn dimmed_color(color: AnsiColor) -> Style {
    Style::new()
        .dimmed()
        .fg_color(Some(color.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_palette_defaults() {
        let palette = ColorPalette::default();
        assert!(matches!(palette.success, Color::Ansi(AnsiColor::Green)));
        assert!(matches!(palette.error, Color::Ansi(AnsiColor::Red)));
    }

    #[test]
    fn test_style_from_color_name() {
        let style = style_from_color_name("red");
        assert!(!style.to_string().is_empty());
    }

    #[test]
    fn test_render_styled_contains_reset() {
        let result = render_styled("test", Color::Ansi(AnsiColor::Green), None);
        assert!(result.contains("\x1b"));
        assert!(result.contains("test"));
    }
}
```

---

## Git Diff Palette Template

```rust
// vtcode-core/src/utils/diff_styles.rs

use anstyle::{AnsiColor, Color, RgbColor, Style};

#[derive(Debug, Clone, Copy)]
pub struct DiffColorPalette {
    pub added_fg: RgbColor,
    pub added_bg: RgbColor,
    pub removed_fg: RgbColor,
    pub removed_bg: RgbColor,
    pub header_color: AnsiColor,
}

impl DiffColorPalette {
    /// Green on dark green for additions, red on dark red for deletions
    pub fn default() -> Self {
        Self {
            added_fg: RgbColor(200, 255, 200),
            added_bg: RgbColor(0, 64, 0),
            removed_fg: RgbColor(255, 200, 200),
            removed_bg: RgbColor(64, 0, 0),
            header_color: AnsiColor::Cyan,
        }
    }

    pub fn added_style(&self) -> Style {
        Style::new()
            .fg_color(Some(Color::Rgb(self.added_fg)))
            .bg_color(Some(Color::Rgb(self.added_bg)))
    }

    pub fn removed_style(&self) -> Style {
        Style::new()
            .fg_color(Some(Color::Rgb(self.removed_fg)))
            .bg_color(Some(Color::Rgb(self.removed_bg)))
    }

    pub fn header_style(&self) -> Style {
        Style::new().fg_color(Some(Color::Ansi(self.header_color)))
    }
}
```

---

## Validation Commands

After refactoring, verify no hardcoded ANSI codes remain:

```bash
# Check for remaining hardcoded escape codes
grep -r "\\x1b\[" --include="*.rs" src/ vtcode-core/src/

# Check for raw Color:: usage outside of constants/configs
grep -r "Color::" --include="*.rs" src/ vtcode-core/src/ | \
  grep -v "ColorPalette\|style_helpers\|diff_styles\|constants"

# Verify all colors go through helpers
cargo build
cargo clippy
cargo test
```

---

## Performance Considerations

- `ColorPalette::default()` should be cheap (it's all Copy types)
- `render_styled()` allocates one String; acceptable for logging/output
- Consider caching `DiffColorPalette` in `DiffRenderer` struct (already done)
- No regex or expensive conversions; all hardcoded match arms

