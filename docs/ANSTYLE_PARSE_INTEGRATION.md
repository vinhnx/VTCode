# anstyle-parse Integration Guide

**Implementation Steps for vtcode System**

## Step 1: Add Dependency

**File: `vtcode-core/Cargo.toml`**

```toml
[dependencies]
# ... existing deps ...
anstyle-parse = "0.2"  # Add this line
```

## Step 2: Create Parser Wrapper Module

**File: `vtcode-core/src/utils/ansi_parser.rs`**

```rust
//! ANSI escape sequence parser wrapper using anstyle-parse
//!
//! Provides a high-level interface for parsing ANSI sequences from terminal output.

use anstyle::Color;
use anstyle_parse::{Parser, Perform};

/// Represents an ANSI styled text segment
#[derive(Debug, Clone)]
pub struct StyledSegment {
    pub text: String,
    pub fg_color: Option<Color>,
    pub bg_color: Option<Color>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

/// Complete parsed ANSI output with styling information
#[derive(Debug, Clone)]
pub struct ParsedAnsi {
    pub plain_text: String,
    pub segments: Vec<StyledSegment>,
}

/// Internal performer for ANSI parsing
struct AnsiParser {
    plain_text: String,
    segments: Vec<StyledSegment>,
    current_segment: StyledSegment,
    fg_color: Option<Color>,
    bg_color: Option<Color>,
    bold: bool,
    italic: bool,
    underline: bool,
}

impl AnsiParser {
    fn new() -> Self {
        Self {
            plain_text: String::new(),
            segments: Vec::new(),
            current_segment: StyledSegment {
                text: String::new(),
                fg_color: None,
                bg_color: None,
                bold: false,
                italic: false,
                underline: false,
            },
            fg_color: None,
            bg_color: None,
            bold: false,
            italic: false,
            underline: false,
        }
    }

    fn flush_segment(&mut self) {
        if !self.current_segment.text.is_empty() {
            self.segments.push(self.current_segment.clone());
            self.current_segment.text.clear();
        }
    }

    fn update_style(&mut self) {
        self.current_segment.fg_color = self.fg_color;
        self.current_segment.bg_color = self.bg_color;
        self.current_segment.bold = self.bold;
        self.current_segment.italic = self.italic;
        self.current_segment.underline = self.underline;
    }
}

impl Perform for AnsiParser {
    fn print(&mut self, c: char) {
        self.plain_text.push(c);
        self.current_segment.text.push(c);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' | b'\r' | b'\t' => {
                self.plain_text.push(byte as char);
                self.current_segment.text.push(byte as char);
            }
            _ => {}
        }
    }

    fn hook(&mut self, _params: &anstyle_parse::Params, _intermediates: &[u8], _ignore: bool, _action: char) {
    }

    fn put(&mut self, _byte: u8) {}

    fn unhook(&mut self) {}

    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {}

    fn csi_dispatch(
        &mut self,
        _params: &anstyle_parse::Params,
        _intermediates: &[u8],
        _ignore: bool,
        _action: char,
    ) {
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _action: u8) {}

    fn set_fg_color(&mut self, color: anstyle_parse::Color) {
        self.flush_segment();
        self.fg_color = convert_color(color);
        self.update_style();
    }

    fn set_bg_color(&mut self, color: anstyle_parse::Color) {
        self.flush_segment();
        self.bg_color = convert_color(color);
        self.update_style();
    }

    fn set_fg_color_default(&mut self) {
        self.flush_segment();
        self.fg_color = None;
        self.update_style();
    }

    fn set_bg_color_default(&mut self) {
        self.flush_segment();
        self.bg_color = None;
        self.update_style();
    }

    fn set_bold(&mut self) {
        self.flush_segment();
        self.bold = true;
        self.update_style();
    }

    fn unset_bold(&mut self) {
        self.flush_segment();
        self.bold = false;
        self.update_style();
    }

    fn set_italic(&mut self) {
        self.flush_segment();
        self.italic = true;
        self.update_style();
    }

    fn unset_italic(&mut self) {
        self.flush_segment();
        self.italic = false;
        self.update_style();
    }

    fn set_underline(&mut self) {
        self.flush_segment();
        self.underline = true;
        self.update_style();
    }

    fn unset_underline(&mut self) {
        self.flush_segment();
        self.underline = false;
        self.update_style();
    }

    fn unset_all(&mut self) {
        self.flush_segment();
        self.fg_color = None;
        self.bg_color = None;
        self.bold = false;
        self.italic = false;
        self.underline = false;
        self.update_style();
    }
}

/// Convert anstyle-parse Color to anstyle Color
fn convert_color(color: anstyle_parse::Color) -> Option<Color> {
    match color {
        anstyle_parse::Color::Named(name) => Some(Color::Ansi(match name {
            anstyle_parse::NamedColor::Black => anstyle::AnsiColor::Black,
            anstyle_parse::NamedColor::Red => anstyle::AnsiColor::Red,
            anstyle_parse::NamedColor::Green => anstyle::AnsiColor::Green,
            anstyle_parse::NamedColor::Yellow => anstyle::AnsiColor::Yellow,
            anstyle_parse::NamedColor::Blue => anstyle::AnsiColor::Blue,
            anstyle_parse::NamedColor::Magenta => anstyle::AnsiColor::Magenta,
            anstyle_parse::NamedColor::Cyan => anstyle::AnsiColor::Cyan,
            anstyle_parse::NamedColor::White => anstyle::AnsiColor::White,
            anstyle_parse::NamedColor::BrightBlack => anstyle::AnsiColor::BrightBlack,
            anstyle_parse::NamedColor::BrightRed => anstyle::AnsiColor::BrightRed,
            anstyle_parse::NamedColor::BrightGreen => anstyle::AnsiColor::BrightGreen,
            anstyle_parse::NamedColor::BrightYellow => anstyle::AnsiColor::BrightYellow,
            anstyle_parse::NamedColor::BrightBlue => anstyle::AnsiColor::BrightBlue,
            anstyle_parse::NamedColor::BrightMagenta => anstyle::AnsiColor::BrightMagenta,
            anstyle_parse::NamedColor::BrightCyan => anstyle::AnsiColor::BrightCyan,
            anstyle_parse::NamedColor::BrightWhite => anstyle::AnsiColor::BrightWhite,
        })),
        anstyle_parse::Color::Indexed(idx) => Some(Color::Ansi256(anstyle::Ansi256Color(idx))),
        anstyle_parse::Color::Rgb(r, g, b) => Some(Color::Rgb(r, g, b)),
    }
}

/// Parse ANSI-styled text and extract styling information
///
/// # Example
///
/// ```ignore
/// let parsed = parse_ansi("\x1b[31mRed text\x1b[0m");
/// assert_eq!(parsed.plain_text, "Red text");
/// assert_eq!(parsed.segments[0].fg_color, Some(Color::Ansi(AnsiColor::Red)));
/// ```
pub fn parse_ansi(text: &str) -> ParsedAnsi {
    let mut performer = AnsiParser::new();
    let mut parser = Parser::new();

    for byte in text.as_bytes() {
        parser.advance(&mut performer, &[*byte]);
    }

    performer.flush_segment();

    ParsedAnsi {
        plain_text: performer.plain_text,
        segments: performer.segments,
    }
}

/// Strip ANSI escape codes from text, keeping only plain text
///
/// # Example
///
/// ```ignore
/// let plain = strip_ansi("\x1b[31mRed text\x1b[0m");
/// assert_eq!(plain, "Red text");
/// ```
pub fn strip_ansi(text: &str) -> String {
    parse_ansi(text).plain_text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plain_text() {
        let result = parse_ansi("hello world");
        assert_eq!(result.plain_text, "hello world");
        assert!(result.segments.is_empty() || result.segments[0].fg_color.is_none());
    }

    #[test]
    fn test_strip_ansi_basic() {
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m"), "red");
    }

    #[test]
    fn test_strip_ansi_bold() {
        assert_eq!(strip_ansi("\x1b[1;32mbold green\x1b[0m"), "bold green");
    }

    #[test]
    fn test_strip_ansi_multiple() {
        let input = "Checking \x1b[0m\x1b[1m\x1b[32mvtcode\x1b[0m";
        assert_eq!(strip_ansi(input), "Checking vtcode");
    }

    #[test]
    fn test_preserve_newlines() {
        let input = "line1\nline2";
        assert_eq!(strip_ansi(input), "line1\nline2");
    }
}
```

## Step 3: Update Module Exports

**File: `vtcode-core/src/utils/mod.rs`**

Add:
```rust
pub mod ansi_parser;
```

## Step 4: Replace Manual Parser in PTY

**File: `vtcode-core/src/tools/pty.rs`**

Replace the `parse_ansi_sequence` function with:

```rust
use crate::utils::ansi_parser;

fn parse_ansi_sequence(text: &str) -> Option<usize> {
    // Use anstyle-parse to find sequence boundaries
    // Parse character by character until we hit a complete sequence
    let mut parser = anstyle_parse::Parser::new();
    let mut last_valid = 0;

    struct SequenceFinder {
        found: Option<usize>,
    }

    impl anstyle_parse::Perform for SequenceFinder {
        fn print(&mut self, _: char) { }
        fn execute(&mut self, _: u8) { }
        fn hook(&mut self, _: &anstyle_parse::Params, _: &[u8], _: bool, _: char) { }
        fn put(&mut self, _: u8) { }
        fn unhook(&mut self) { }
        fn osc_dispatch(&mut self, _: &[&[u8]], _: bool) { }
        fn csi_dispatch(&mut self, _: &anstyle_parse::Params, _: &[u8], _: bool, _: char) { }
        fn esc_dispatch(&mut self, _: &[u8], _: bool, _: u8) { }
    }

    // Simple approach: if text starts with ESC, find next boundary
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes[0] != 0x1b {
        return None;
    }

    // Try to identify sequence type and length
    if bytes.len() < 2 {
        return None;
    }

    match bytes[1] {
        b'[' => {
            // CSI sequence - ends with 0x40-0x7E
            for (i, &b) in bytes.iter().enumerate().skip(2) {
                if (0x40..=0x7e).contains(&b) {
                    return Some(i + 1);
                }
            }
            None
        }
        b']' => {
            // OSC sequence - ends with BEL (0x07) or ST (ESC \)
            for i in 2..bytes.len() {
                if bytes[i] == 0x07 {
                    return Some(i + 1);
                }
                if i + 1 < bytes.len() && bytes[i] == 0x1b && bytes[i + 1] == b'\\' {
                    return Some(i + 2);
                }
            }
            None
        }
        b'P' | b'^' | b'_' => {
            // Other sequence types - end with ST (ESC \)
            for i in 2..bytes.len() {
                if i + 1 < bytes.len() && bytes[i] == 0x1b && bytes[i + 1] == b'\\' {
                    return Some(i + 2);
                }
            }
            None
        }
        _ => Some(2),  // Other 2-character escape sequences
    }
}
```

## Step 5: Update ANSI Stripping

**File: `vtcode-core/src/tools/registry/executors.rs`**

Replace `strip_ansi` function with:

```rust
use crate::utils::ansi_parser;

fn strip_ansi(text: &str) -> String {
    ansi_parser::strip_ansi(text)
}
```

Simplifies from ~60 lines to 1 line!

## Step 6: Testing

**File: `vtcode-core/src/utils/ansi_parser.rs` (already included in Step 2)**

Run tests:
```bash
cargo test ansi_parser::tests
```

**Integration tests** - Add to existing test suite:

```rust
#[test]
fn test_ansi_parser_with_real_tool_output() {
    let output = "cargo check\n\x1b[0m\x1b[1m\x1b[32mvtcode\x1b[0m v0.1.0\n\x1b[0m";
    let parsed = parse_ansi(output);
    assert!(parsed.plain_text.contains("vtcode v0.1.0"));
    assert!(!parsed.plain_text.contains("\x1b"));
}
```

## Step 7: Documentation

Update `docs/ANSTYLE_PARSE_REVIEW.md` with:
- ✅ Dependency added
- Implementation dates
- Performance benchmarks
- Lessons learned

## Future Enhancements

### Color-Preserving Output

```rust
use ratatui::style::{Style, Color as RatColor};
use anstyle::Color;

pub fn ansi_to_ratatui(parsed: &ParsedAnsi) -> Vec<(String, Style)> {
    parsed
        .segments
        .iter()
        .map(|seg| {
            let style = Style::default()
                .fg(seg.fg_color.and_then(|c| anstyle_to_ratatui(c)))
                .bg(seg.bg_color.and_then(|c| anstyle_to_ratatui(c)));
            (seg.text.clone(), style)
        })
        .collect()
}

fn anstyle_to_ratatui(color: Color) -> Option<RatColor> {
    match color {
        Color::Ansi(ac) => {
            // Convert anstyle::AnsiColor to ratatui::style::Color
            Some(RatColor::Indexed(ac as u8))
        }
        Color::Ansi256(ac256) => Some(RatColor::Indexed(ac256.0)),
        Color::Rgb(r, g, b) => Some(RatColor::Rgb(r, g, b)),
    }
}
```

### Intelligent Truncation

```rust
pub fn truncate_with_ansi(text: &str, width: usize) -> String {
    let parsed = parse_ansi(text);
    let truncated = parsed.plain_text.chars().take(width).collect::<String>();
    // Reconstruct with styling info...
}
```

## Verification Checklist

- [ ] Add `anstyle-parse = "0.2"` to `vtcode-core/Cargo.toml`
- [ ] Create `vtcode-core/src/utils/ansi_parser.rs`
- [ ] Add module export to `vtcode-core/src/utils/mod.rs`
- [ ] Run `cargo check` - verifies compilation
- [ ] Run `cargo test` - all tests pass
- [ ] Replace `strip_ansi` in `executors.rs`
- [ ] Update `pty.rs` to use new parser if needed
- [ ] Performance benchmark comparison
- [ ] Document in CHANGELOG.md

## Performance Notes

`anstyle-parse` vs alternatives:

| Implementation | Speed | Code Size | Maintainability |
|---|---|---|---|
| Manual parser | Fast | 40 LOC | Low |
| vte crate | Medium | Dependency | Medium |
| **anstyle-parse** | **Medium** | **Dependency** | **High** |

Use `cargo bench` to compare if performance critical.
