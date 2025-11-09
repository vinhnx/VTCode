# Phase 1 Implementation Guide: Anstyle Integration

Concrete implementation steps for Phase 1 (Foundation) improvements.

## Step 1: Update Cargo.toml Dependencies

**File**: `vtcode-core/Cargo.toml`

Add these three lines to the `[dependencies]` section (around line 88, after existing anstyle crates):

```toml
anstyle-git = "1.1"
anstyle-ls = "1.0"
```

Note: `anstyle`, `anstyle-parse`, `anstyle-crossterm`, and `anstyle-query` are already present.

**Why**: These crates provide battle-tested parsers for Git and LS_COLORS syntax, reducing our custom parsing burden.

---

## Step 2: Expand InlineTextStyle Struct

**File**: `vtcode-core/src/ui/tui/types.rs`

**Current code** (lines 82-97):
```rust
pub struct InlineTextStyle {
    pub color: Option<AnsiColorEnum>,
}

impl InlineTextStyle {
    pub fn merge_color(mut self, fallback: Option<AnsiColorEnum>) -> Self {
        if self.color.is_none() {
            self.color = fallback;
        }
        self
    }
    // ...
}
```

**Replace with**:
```rust
use anstyle::Effects;

pub struct InlineTextStyle {
    pub color: Option<AnsiColorEnum>,
    pub bg_color: Option<AnsiColorEnum>,  // NEW
    pub effects: Effects,                  // NEW: replaces bold, italic
}

impl InlineTextStyle {
    pub fn merge_color(mut self, fallback: Option<AnsiColorEnum>) -> Self {
        if self.color.is_none() {
            self.color = fallback;
        }
        self
    }
    
    pub fn merge_bg_color(mut self, fallback: Option<AnsiColorEnum>) -> Self {
        if self.bg_color.is_none() {
            self.bg_color = fallback;
        }
        self
    }
    
    pub fn bold(mut self) -> Self {
        self.effects = self.effects | Effects::BOLD;
        self
    }
    
    pub fn italic(mut self) -> Self {
        self.effects = self.effects | Effects::ITALIC;
        self
    }
    
    pub fn underline(mut self) -> Self {
        self.effects = self.effects | Effects::UNDERLINE;
        self
    }
    
    pub fn dim(mut self) -> Self {
        self.effects = self.effects | Effects::DIMMED;
        self
    }
    
    pub fn reverse(mut self) -> Self {
        self.effects = self.effects | Effects::REVERSE;
        self
    }
    
    pub fn to_ansi_style(&self, fallback: Option<AnsiColorEnum>) -> AnsiStyle {
        let mut style = AnsiStyle::new();
        
        let color = self.color.or(fallback);
        if let Some(col) = color {
            style = style.fg_color(Some(anstyle::Color::from(col)));
        }
        
        if let Some(col) = self.bg_color {
            style = style.bg_color(Some(anstyle::Color::from(col)));
        }
        
        // Apply effects
        if self.effects.contains(Effects::BOLD) {
            style = style.bold();
        }
        if self.effects.contains(Effects::ITALIC) {
            style = style.italic();
        }
        if self.effects.contains(Effects::UNDERLINE) {
            style = style.underline();
        }
        if self.effects.contains(Effects::DIMMED) {
            style = style.dimmed();
        }
        if self.effects.contains(Effects::REVERSE) {
            style = style.reversed();
        }
        
        style
    }
}

impl Default for InlineTextStyle {
    fn default() -> Self {
        Self {
            color: None,
            bg_color: None,
            effects: Effects::empty(),
        }
    }
}
```

**Why**: 
- `bg_color` enables background color support
- `effects` uses `anstyle::Effects` bitmask for all text decorations (bold, dim, italic, underline, reverse)
- Replaces individual `bool` fields (`bold`, `italic`) with a single composable bitmask
- Adds helper methods for fluent style building

---

## Step 3: Create Theme Parser Module

**File**: `vtcode-core/src/ui/tui/theme_parser.rs` (NEW)

```rust
//! Parse theme configuration from multiple syntaxes (Git, LS_COLORS, custom).

use anyhow::{Context, Result, anyhow};
use anstyle::Style as AnsiStyle;

/// Parses color configuration strings in different syntaxes
pub struct ThemeConfigParser;

impl ThemeConfigParser {
    /// Parse a string in Git's color configuration syntax.
    ///
    /// Examples:
    /// - "bold red" → bold red foreground
    /// - "red blue" → red foreground on blue background
    /// - "#0000ee ul" → RGB blue with underline
    ///
    /// # Errors
    /// Returns error if the input doesn't match Git color syntax.
    pub fn parse_git_style(input: &str) -> Result<AnsiStyle> {
        anstyle_git::parse(input)
            .map_err(|e| anyhow!("Failed to parse Git style '{}': {:?}", input, e))
    }

    /// Parse a string in LS_COLORS syntax (ANSI escape codes).
    ///
    /// Examples:
    /// - "34" → blue foreground
    /// - "01;34" → bold blue
    /// - "34;03" → blue with italic
    ///
    /// # Errors
    /// Returns error if the input doesn't match LS_COLORS syntax.
    pub fn parse_ls_colors(input: &str) -> Result<AnsiStyle> {
        anstyle_ls::parse(input)
            .map_err(|e| anyhow!("Failed to parse LS_COLORS '{}': {:?}", input, e))
    }

    /// Parse a style string, attempting Git syntax first, then LS_COLORS as fallback.
    ///
    /// This is a convenience function for flexible input parsing.
    pub fn parse_flexible(input: &str) -> Result<AnsiStyle> {
        // Try Git syntax first (more human-readable)
        Self::parse_git_style(input)
            .or_else(|_| Self::parse_ls_colors(input))
            .with_context(|| format!("Could not parse style string: '{}'", input))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_git_bold_red() {
        let style = ThemeConfigParser::parse_git_style("bold red").unwrap();
        assert!(style.get_effects().contains(anstyle::Effects::BOLD));
    }

    #[test]
    fn test_parse_git_hex_color() {
        let style = ThemeConfigParser::parse_git_style("#0000ee").unwrap();
        assert!(style.get_fg_color().is_some());
    }

    #[test]
    fn test_parse_ls_colors_blue() {
        let style = ThemeConfigParser::parse_ls_colors("34").unwrap();
        assert!(style.get_fg_color().is_some());
    }

    #[test]
    fn test_parse_ls_colors_bold_blue() {
        let style = ThemeConfigParser::parse_ls_colors("01;34").unwrap();
        assert!(style.get_effects().contains(anstyle::Effects::BOLD));
    }
}
```

**Why**:
- Centralizes all style string parsing logic
- Provides clear API surface for consuming code
- Wraps parser errors with context
- Enables flexible input (try multiple syntaxes)
- Includes tests for validation

---

## Step 4: Update Module Exports

**File**: `vtcode-core/src/ui/tui/mod.rs`

Add module export (if not already present):

```rust
pub mod theme_parser;
```

And optionally re-export for convenience:

```rust
pub use theme_parser::ThemeConfigParser;
```

---

## Step 5: Update convert_style Function

**File**: `vtcode-core/src/ui/tui/style.rs`

**Current code** (lines 17-30):
```rust
fn convert_style_color(style: &AnsiStyle) -> Option<AnsiColorEnum> {
    style.get_fg_color().and_then(convert_ansi_color)
}

pub fn convert_style(style: AnsiStyle) -> InlineTextStyle {
    let mut converted = InlineTextStyle {
        color: convert_style_color(&style),
        ..InlineTextStyle::default()
    };
    let effects = style.get_effects();
    converted.bold = effects.contains(Effects::BOLD);
    converted.italic = effects.contains(Effects::ITALIC);
    converted
}
```

**Replace with**:
```rust
fn convert_style_color(style: &AnsiStyle) -> Option<AnsiColorEnum> {
    style.get_fg_color().and_then(convert_ansi_color)
}

fn convert_style_bg_color(style: &AnsiStyle) -> Option<AnsiColorEnum> {
    style.get_bg_color().and_then(convert_ansi_color)
}

pub fn convert_style(style: AnsiStyle) -> InlineTextStyle {
    InlineTextStyle {
        color: convert_style_color(&style),
        bg_color: convert_style_bg_color(&style),
        effects: style.get_effects(),  // Use full Effects bitmask
    }
}
```

**Why**:
- Maps ANSI background colors to `bg_color` field
- Preserves all effects (not just bold/italic)
- Simpler code (no manual field assignment)

---

## Step 6: Update ratatui_style_from_inline

**File**: `vtcode-core/src/ui/tui/style.rs`

**Current code** (lines 71-86):
```rust
pub fn ratatui_style_from_inline(
    style: &InlineTextStyle,
    fallback: Option<AnsiColorEnum>,
) -> Style {
    let mut resolved = Style::default();
    if let Some(color) = style.color.or(fallback) {
        resolved = resolved.fg(ratatui_color_from_ansi(color));
    }
    if style.bold {
        resolved = resolved.add_modifier(Modifier::BOLD);
    }
    if style.italic {
        resolved = resolved.add_modifier(Modifier::ITALIC);
    }
    resolved
}
```

**Replace with**:
```rust
pub fn ratatui_style_from_inline(
    style: &InlineTextStyle,
    fallback: Option<AnsiColorEnum>,
) -> Style {
    use anstyle::Effects;

    let mut resolved = Style::default();
    
    // Foreground color
    if let Some(color) = style.color.or(fallback) {
        resolved = resolved.fg(ratatui_color_from_ansi(color));
    }
    
    // Background color (NEW)
    if let Some(color) = style.bg_color {
        resolved = resolved.bg(ratatui_color_from_ansi(color));
    }
    
    // Effects bitmask
    let effects = style.effects;
    if effects.contains(Effects::BOLD) {
        resolved = resolved.add_modifier(Modifier::BOLD);
    }
    if effects.contains(Effects::ITALIC) {
        resolved = resolved.add_modifier(Modifier::ITALIC);
    }
    if effects.contains(Effects::UNDERLINE) {
        resolved = resolved.add_modifier(Modifier::UNDERLINED);
    }
    if effects.contains(Effects::DIMMED) {
        resolved = resolved.add_modifier(Modifier::DIM);
    }
    if effects.contains(Effects::REVERSE) {
        resolved = resolved.add_modifier(Modifier::REVERSED);
    }
    // Note: Strikethrough requires CROSSED_OUT, check ratatui version
    
    resolved
}
```

**Why**:
- Handles background colors
- Supports all ratatui Modifiers (DIM, UNDERLINED, REVERSED)
- Graceful handling of effects (some may not be supported in all terminals)

---

## Step 7: Update All Call Sites

After the above changes, find and update code that creates `InlineTextStyle` directly.

**Search for**: `InlineTextStyle {` in `vtcode-core/src/ui/tui/**/*.rs`

**Example fix**:
```rust
// OLD
let style = InlineTextStyle {
    color: Some(color),
    ..InlineTextStyle::default()
};

// NEW (if only color changes)
let style = InlineTextStyle {
    color: Some(color),
    bg_color: None,
    effects: Effects::empty(),
};

// Or use builder pattern if implemented
let style = InlineTextStyle::default().with_color(color);
```

---

## Testing

### Unit Tests

Add to `vtcode-core/src/ui/tui/style.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use anstyle::Effects;

    #[test]
    fn test_convert_style_with_bold_and_color() {
        let ansi_style = AnsiStyle::new()
            .bold()
            .fg_color(Some(anstyle::Color::Ansi(AnsiColor::Red)));
        
        let inline = convert_style(ansi_style);
        assert!(inline.effects.contains(Effects::BOLD));
        assert!(inline.color.is_some());
    }

    #[test]
    fn test_ratatui_style_from_inline_with_bg() {
        let style = InlineTextStyle {
            color: Some(AnsiColorEnum::Ansi(AnsiColor::White)),
            bg_color: Some(AnsiColorEnum::Ansi(AnsiColor::Black)),
            effects: Effects::BOLD,
        };
        
        let ratatui_style = ratatui_style_from_inline(&style, None);
        assert_eq!(ratatui_style.fg, Some(Color::White));
        assert_eq!(ratatui_style.bg, Some(Color::Black));
    }
}
```

### Integration Test

Create `vtcode-core/examples/style_parsing.rs`:

```rust
//! Example: Parse and apply styles from Git and LS_COLORS syntax

use vtcode_core::ui::tui::ThemeConfigParser;
use anstyle::Style;

fn main() -> anyhow::Result<()> {
    // Parse Git style
    let git_style = ThemeConfigParser::parse_git_style("bold red")?;
    println!("Git 'bold red': {:?}", git_style);

    // Parse LS_COLORS style
    let ls_style = ThemeConfigParser::parse_ls_colors("01;34")?;
    println!("LS_COLORS '01;34' (bold blue): {:?}", ls_style);

    // Parse hex color (Git syntax)
    let hex_style = ThemeConfigParser::parse_git_style("#0000ee ul")?;
    println!("Git '#0000ee ul': {:?}", hex_style);

    Ok(())
}
```

Run with: `cargo run --example style_parsing`

---

## Verification Checklist

- [ ] Cargo.toml updated with `anstyle-git` and `anstyle-ls`
- [ ] `InlineTextStyle` expanded with `bg_color` and `effects`
- [ ] `InlineTextStyle::default()` implementations updated
- [ ] `theme_parser.rs` module created and exported
- [ ] `convert_style()` updated
- [ ] `ratatui_style_from_inline()` updated
- [ ] All `InlineTextStyle` construction sites updated
- [ ] Code compiles: `cargo check`
- [ ] Clippy passes: `cargo clippy`
- [ ] Tests pass: `cargo test`
- [ ] Example runs: `cargo run --example style_parsing`

---

## Rollback Plan

If issues arise:

1. **Revert types.rs**: Keep `bool bold` and `bool italic`, add `effects` as optional field
2. **Revert convert_style()**: Only extract bold/italic from effects
3. **Keep theme_parser.rs**: It's additive and safe
4. **Test incrementally**: Update one module at a time, test after each

---

## Next Steps

After Phase 1 is complete:
- **Phase 2**: Apply parsers to diff rendering, update tests
- **Phase 3**: Implement file colorization with LS_COLORS
- **Phase 4**: Add user config file support (`.vtcoderc` or `vtcode.toml` extensions)
