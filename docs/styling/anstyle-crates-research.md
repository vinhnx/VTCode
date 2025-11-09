# Anstyle Git/LS Crates Research & Vtcode Styling Improvements

## Overview

**anstyle-git** and **anstyle-ls** are complementary Rust crates that parse domain-specific color configuration syntaxes into standardized ANSI styles via the `anstyle` crate. This research explores how their parsing approaches can improve vtcode's styling system.

## Crate Analysis

### anstyle-git (v1.1.3)

**Purpose**: Parses Git's color configuration syntax

**Key Features**:
- Parses Git style descriptions (e.g., `"bold red blue"`)
- Supports keywords: `bold`, `dim`, `italic`, `underline`, `reverse`, `strikethrough`
- Supports named colors: `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `white`
- Supports hex colors: `#RRGGBB` (e.g., `#0000ee`)
- Supports foreground and background colors in single declaration
- Returns `anstyle::Style` (composable style objects)

**Example**:
```rust
let style = anstyle_git::parse("bold red blue").unwrap();
// Result: red foreground, blue background, with bold modifier

let hyperlink_style = anstyle_git::parse("#0000ee ul").unwrap();
// Result: RGB(0, 0, 238) foreground, underline
```

**Syntax Grammar**:
- Words separated by whitespace
- First color term is foreground
- Second color term is background
- Effect keywords can appear anywhere
- Hex colors allowed for both foreground and background

### anstyle-ls (v1.0.5)

**Purpose**: Parses `LS_COLORS` environment variable syntax

**Key Features**:
- Parses file type patterns with ANSI codes (e.g., `di=01;34` for directories)
- Supports ANSI 8-bit escape codes (semicolon-separated)
- Common file type codes:
  - `di` = directory
  - `ln` = symlink
  - `so` = socket
  - `*.ext` = file extension patterns
- Returns `anstyle::Style` for each file type classification
- Handles both foreground and background in single code sequence

**Example**:
```rust
let style = anstyle_ls::parse("34;03").unwrap();
// Result: blue foreground (34), italic (03)
```

**Syntax Grammar**:
- ANSI codes: `01` (bold), `03` (italic), `04` (underline), `30-37` (colors), `90-97` (bright colors), `40-47` (bg colors)
- Semicolon-separated sequence of codes
- File type pattern keys precede the colon (e.g., `di=01;34:ln=36`)

## Current Vtcode Styling Architecture

### Existing Approach

**Location**: `vtcode-core/src/ui/tui/style.rs`, `vtcode-core/src/ui/theme.rs`

**Current Stack**:
1. **anstyle** (v1.0) - Already integrated for ANSI style representation
2. **anstyle-parse** (v0.2) - Parses ANSI escape sequences from terminal output
3. **anstyle-crossterm** (v4.0) - Bridge to crossterm TUI library
4. **ratatui** (v0.29) - TUI rendering
5. **catppuccin** (v2.5) - Theme color palettes

**Styling Pipeline**:
```
anstyle::Style (abstract)
  ↓
convert_style() → InlineTextStyle (limited: color, bold, italic only)
  ↓
ratatui_style_from_inline() → ratatui::style::Style
  ↓
TUI rendering
```

**Limitations**:
1. **Incomplete Effect Support**: Only handles `bold` and `italic`, ignores `dim`, `underline`, `strikethrough`, `reverse`
2. **No Config String Parsing**: Hard-coded theme palettes; no support for Git-style or LS_COLORS-style configuration strings
3. **Manual Color Mixing**: Uses custom functions (`mix()`, `lighten()`, `ensure_contrast()`) instead of leveraging ecosystem tools
4. **Limited Background Support**: `InlineTextStyle` doesn't properly model background colors
5. **Cargo.toml**: Missing `anstyle-git` and `anstyle-ls` dependencies

## Recommended Improvements

### 1. Add Missing Anstyle Crates

**Action**: Update `vtcode-core/Cargo.toml`

```toml
[dependencies]
anstyle = "1.0"
anstyle-git = "1.1"
anstyle-ls = "1.0"
anstyle-parse = "0.2"
anstyle-crossterm = "4.0"
anstyle-query = "1.0"
```

**Benefits**:
- Parse Git config colors from `.git/config`
- Parse file listing colors from `LS_COLORS` env var
- Reduce custom parsing code

### 2. Expand InlineTextStyle to Support Full Effects

**Current** (`vtcode-core/src/ui/tui/types.rs`):
```rust
pub struct InlineTextStyle {
    pub color: Option<AnsiColorEnum>,
    pub bold: bool,
    pub italic: bool,
}
```

**Improved**:
```rust
use anstyle::Effects;

pub struct InlineTextStyle {
    pub color: Option<AnsiColorEnum>,
    pub bg_color: Option<AnsiColorEnum>,
    pub effects: Effects,  // Replaces bold, italic
}

impl InlineTextStyle {
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
}
```

### 3. Create a Theme Configuration Parser

**New Module**: `vtcode-core/src/ui/tui/theme_parser.rs`

```rust
use anstyle_git;
use anstyle_ls;
use anstyle::Style;

/// Parse theme configuration from multiple sources
pub struct ThemeConfigParser;

impl ThemeConfigParser {
    /// Parse Git config style strings
    pub fn parse_git_style(input: &str) -> anyhow::Result<Style> {
        anstyle_git::parse(input).map_err(|e| {
            anyhow::anyhow!("Failed to parse Git style: {}", e)
        })
    }
    
    /// Parse LS_COLORS style codes for file colorization
    pub fn parse_ls_colors(input: &str) -> anyhow::Result<Style> {
        anstyle_ls::parse(input).map_err(|e| {
            anyhow::anyhow!("Failed to parse LS_COLORS: {}", e)
        })
    }
    
    /// Parse custom theme string (supports both Git and LS syntax)
    pub fn parse_style(input: &str, dialect: StyleDialect) -> anyhow::Result<Style> {
        match dialect {
            StyleDialect::Git => Self::parse_git_style(input),
            StyleDialect::LS => Self::parse_ls_colors(input),
        }
    }
}

pub enum StyleDialect {
    Git,
    LS,
}
```

### 4. Add LS_COLORS File Coloring Support

**Use Case**: When displaying files in the file picker modal, respect system `LS_COLORS` preferences

**Location**: `vtcode-core/src/ui/tui/session/file_palette.rs`

```rust
use anstyle_ls;
use std::env;

pub struct FileColorizer {
    ls_colors: Option<String>,
}

impl FileColorizer {
    pub fn new() -> Self {
        Self {
            ls_colors: env::var("LS_COLORS").ok(),
        }
    }
    
    pub fn style_for_file(&self, path: &Path) -> Option<anstyle::Style> {
        let ls_colors = self.ls_colors.as_ref()?;
        
        // Determine file type from path
        let file_type = if path.is_dir() {
            "di"  // directory
        } else if path.is_symlink() {
            "ln"  // symlink
        } else {
            // Try extension-based matching
            path.extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| format!("*.{}", ext).leak())
                .unwrap_or("fi")  // fallback: file
        };
        
        // Parse LS_COLORS and extract style for this file type
        anstyle_ls::parse(ls_colors)
            .ok()
            .and_then(|_| Some(anstyle::Style::new()))  // Placeholder
    }
}
```

### 5. Support Git Config Color Parsing

**Use Case**: Parse `.git/config` color settings for diff/status visualization

**Location**: `vtcode-core/src/ui/diff_renderer.rs`

```rust
use anstyle_git;

pub struct GitColorConfig {
    diff_new: anstyle::Style,
    diff_old: anstyle::Style,
    diff_context: anstyle::Style,
}

impl GitColorConfig {
    pub fn from_git_config(config_path: &Path) -> anyhow::Result<Self> {
        // Read .git/config
        let content = std::fs::read_to_string(config_path)?;
        
        // Parse color sections
        // [color "diff"] new = green
        // [color "diff"] old = red
        // [color "diff"] context = default
        
        let diff_new = Self::extract_color_style(&content, "diff", "new")?;
        let diff_old = Self::extract_color_style(&content, "diff", "old")?;
        let diff_context = Self::extract_color_style(&content, "diff", "context")?;
        
        Ok(Self {
            diff_new,
            diff_old,
            diff_context,
        })
    }
    
    fn extract_color_style(
        content: &str,
        section: &str,
        key: &str,
    ) -> anyhow::Result<anstyle::Style> {
        // Find pattern: [color "section"] key = value
        // Parse value with anstyle_git::parse()
        todo!()
    }
}
```

### 6. Update convert_style Function

**Location**: `vtcode-core/src/ui/tui/style.rs`

```rust
use anstyle::{AnsiColor, Color as AnsiColorEnum, Effects, RgbColor, Style as AnsiStyle};

fn convert_style_color(style: &AnsiStyle) -> Option<AnsiColorEnum> {
    style.get_fg_color().and_then(convert_ansi_color)
}

fn convert_style_bg_color(style: &AnsiStyle) -> Option<AnsiColorEnum> {
    style.get_bg_color().and_then(convert_ansi_color)
}

pub fn convert_style(style: AnsiStyle) -> InlineTextStyle {
    let effects = style.get_effects();
    
    InlineTextStyle {
        color: convert_style_color(&style),
        bg_color: convert_style_bg_color(&style),
        effects,  // Now uses full Effects bitmask
    }
}
```

### 7. Update ratatui_style_from_inline

**Location**: `vtcode-core/src/ui/tui/style.rs`

```rust
use anstyle::Effects;
use ratatui::style::{Color, Modifier, Style};

pub fn ratatui_style_from_inline(
    style: &InlineTextStyle,
    fallback: Option<AnsiColorEnum>,
) -> Style {
    let mut resolved = Style::default();
    
    // Foreground color
    if let Some(color) = style.color.or(fallback) {
        resolved = resolved.fg(ratatui_color_from_ansi(color));
    }
    
    // Background color (NEW)
    if let Some(color) = style.bg_color {
        resolved = resolved.bg(ratatui_color_from_ansi(color));
    }
    
    // Effects
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
    // Note: strikethrough not widely supported in ratatui
    
    resolved
}
```

## Implementation Priority

### Phase 1: Foundation (Low Risk)
1. Add `anstyle-git` and `anstyle-ls` to Cargo.toml
2. Create `theme_parser.rs` module with basic parsing functions
3. Update `InlineTextStyle` to include `bg_color` and `effects`

### Phase 2: Integration (Medium Risk)
4. Update `convert_style()` and `ratatui_style_from_inline()` for full effects
5. Add Git config color parsing to diff renderer
6. Update tests for new styling capabilities

### Phase 3: Features (Lower Risk, High Value)
7. Implement `FileColorizer` for LS_COLORS support
8. Add environment variable parsing for system colors
9. Support custom theme files with Git/LS syntax

## Benefits Summary

| Aspect | Current | Improved |
|--------|---------|----------|
| **Effect Support** | bold, italic only | bold, dim, italic, underline, strikethrough, reverse |
| **Configuration** | Hard-coded palettes | Parse Git/LS configs + custom files |
| **Background Colors** | Not supported | Full support via `bg_color` |
| **System Integration** | None | Read LS_COLORS, .git/config, custom configs |
| **Code Reuse** | Custom parsing | Leverage `anstyle-git`, `anstyle-ls` |
| **WCAG Compliance** | Partial (contrast checking) | Enhanced with full effect control |

## Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|-----------|
| Ratatui doesn't support all effects | Medium | Test with strikethrough, fallback gracefully |
| Breaking changes to InlineTextStyle | High | Add deprecation layer, update all callers systematically |
| Performance of Git config parsing | Low | Cache parsed configs, lazy initialization |
| LS_COLORS parsing on non-Unix systems | Low | Graceful degradation (no-op on Windows) |

## Related Files

- `vtcode-core/src/ui/tui/style.rs` - Style conversion functions
- `vtcode-core/src/ui/tui/types.rs` - InlineTextStyle struct
- `vtcode-core/src/ui/theme.rs` - Theme palette management
- `vtcode-core/src/ui/tui/session/file_palette.rs` - File listing UI
- `vtcode-core/src/ui/diff_renderer.rs` - Diff visualization
- `vtcode-core/Cargo.toml` - Dependencies

## References

- [anstyle-git docs](https://docs.rs/anstyle-git/latest/anstyle_git/)
- [anstyle-ls docs](https://docs.rs/anstyle-ls/latest/anstyle_ls/)
- [anstyle crate](https://docs.rs/anstyle/latest/anstyle/)
- [Git Color Configuration](https://git-scm.com/book/en/v2/Git-Customization-Git-Configuration#Colors)
- [LS_COLORS Format](https://linux.die.net/man/5/dir_colors)
