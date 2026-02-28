# ANSI Escape Sequences in VT Code

## Overview

VT Code extensively uses ANSI escape sequences for terminal control, PTY output processing, and TUI rendering. This document maps the ANSI reference to VT Code's implementation.

## Key Modules

### 1. ANSI Parser (`vtcode-commons/src/ansi.rs`)

**Purpose**: Strip ANSI escape sequences from text

```rust
pub fn strip_ansi(text: &str) -> String
pub fn strip_ansi_bytes(input: &[u8]) -> Vec<u8>
```

**Used in**:

-   PTY output cleaning (`vtcode-core/src/tools/pty.rs:208`)
-   Tool output formatting (`vtcode-core/src/tools/registry/executors.rs`)
-   TUI session rendering (`vtcode-tui/src/core_tui/session/text_utils.rs`)

**Patterns Handled**:

-   CSI sequences: `ESC[...m` and C1 `CSI` (`0x9B`)
-   Cursor control: `ESC[H`, `ESC[A/B/C/D`
-   Erase functions: `ESC[J`, `ESC[K`
-   OSC sequences: `ESC]...BEL/ST` and C1 `OSC` (`0x9D`)
-   DCS/PM/APC/SOS strings with `ST` terminators (`ESC \` or C1 `ST` `0x9C`)
-   VT100 recovery rules for malformed streams:
    `ESC` aborts current control sequence; `CAN`/`SUB` abort sequence processing.
-   XTerm-compatible control processing:
    `strip_ansi()` operates on decoded text (`ESC`-prefixed control forms),
    while `strip_ansi_bytes()` handles raw 8-bit C1 control bytes.

`vtcode-core/src/utils/ansi_parser.rs` and `vtcode-tui/src/utils/ansi_parser.rs` both re-export this shared implementation.

### 2. ANSI Style Utilities (`vtcode-core/src/utils/anstyle_utils.rs`)

**Purpose**: Convert ANSI styles to Ratatui styles for TUI rendering

**Key Functions**:

```rust
pub fn ansi_color_to_ratatui_color(color: &AnsiColorType) -> Color
pub fn ansi_effects_to_ratatui_modifiers(effects: Effects) -> Modifier
pub fn ansi_style_to_ratatui_style(style: AnsiStyle) -> Style
```

**Color Support**:

-   8/16 colors (ANSI standard)
-   256 colors (8-bit)
-   RGB/Truecolor (24-bit)

### 3. ANSI Renderer (`vtcode-core/src/utils/ansi.rs`)

**Purpose**: Render styled text to terminal

```rust
pub struct AnsiRenderer
pub enum MessageStyle
```

**Used in**: Tool policy prompts, status messages

## ANSI Sequences Used in VT Code

### Colors (Most Common)

| Usage            | ANSI Sequence      | VT Code Context       |
| ---------------- | ------------------ | --------------------- |
| Error messages   | `ESC[31m` (Red)    | Tool execution errors |
| Success messages | `ESC[32m` (Green)  | Successful operations |
| Warnings         | `ESC[33m` (Yellow) | Policy warnings       |
| Info             | `ESC[34m` (Blue)   | General information   |
| Dim text         | `ESC[2m`           | Secondary information |
| Bold text        | `ESC[1m`           | Emphasis              |
| Reset            | `ESC[0m`           | Clear all styles      |

### Cursor Control

| Usage       | ANSI Sequence    | VT Code Context       |
| ----------- | ---------------- | --------------------- |
| Hide cursor | `ESC[?25l`       | During TUI operations |
| Show cursor | `ESC[?25h`       | After TUI exit        |
| Clear line  | `ESC[2K`         | Progress updates      |
| Move cursor | `ESC[{n}A/B/C/D` | TUI navigation        |

### Screen Modes

| Usage              | ANSI Sequence | VT Code Context        |
| ------------------ | ------------- | ---------------------- |
| Alt buffer enable  | `ESC[?1049h`  | External editor launch |
| Alt buffer disable | `ESC[?1049l`  | Return from editor     |
| Save screen        | `ESC[?47h`    | Before external app    |
| Restore screen     | `ESC[?47l`    | After external app     |

## PTY Output Processing

### Flow

```
PTY Command Output (with ANSI)
    ↓
Streaming Callback (raw bytes)
    ↓
UTF-8 Decode
    ↓
Extract Last Line (with ANSI)
    ↓
strip_ansi() for display
    ↓
Update Progress Reporter
    ↓
TUI Renders (clean text)
```

### Practical CLI redraw pattern

Many terminal apps (progress bars, spinners, interactive prompts) use this pattern:

- `\r` (carriage return) to return to line start
- `ESC[2K` to clear current line
- rewritten content on the same line

VT Code strips ANSI control sequences but preserves line-control characters like `\r`/`\n`/`\t`.

### Important limitation

`strip_ansi()` is a lexical stripper, not a terminal emulator. It removes control sequences but does not apply cursor movement effects (`CUU`/`CUD`/`CUF`/`CUB`) to reconstruct a final visual frame.

### Implementation

**Location**: `vtcode-core/src/tools/pty.rs`

```rust
// Line 208: Clean PTY output for token counting
let cleaned_text = crate::utils::ansi_parser::strip_ansi(text);
```

**Location**: `src/agent/runloop/unified/tool_pipeline.rs`

```rust
// Streaming callback extracts last line
if let Ok(s) = std::str::from_utf8(chunk) {
    if let Some(last_line) = s.lines()
        .filter(|l| !l.trim().is_empty())
        .last()
    {
        // This may contain ANSI codes
        // strip_ansi() is called before display
    }
}
```

## TUI Rendering

### Color Mapping

**8/16 Colors** (from reference):

```
Black=30/40   → Ratatui::Black
Red=31/41     → Ratatui::Red
Green=32/42   → Ratatui::Green
Yellow=33/43  → Ratatui::Yellow
Blue=34/44    → Ratatui::Blue
Magenta=35/45 → Ratatui::Magenta
Cyan=36/46    → Ratatui::Cyan
White=37/47   → Ratatui::White
```

**Bright Colors** (90-97):

```
BrightRed=91     → Ratatui::LightRed
BrightGreen=92   → Ratatui::LightGreen
BrightYellow=93  → Ratatui::LightYellow
BrightBlue=94    → Ratatui::LightBlue
```

**256 Colors** (`ESC[38;5;{ID}m`):

```rust
// Converted via ansi_color_to_ratatui_color()
// Supports full 256-color palette
```

**RGB Colors** (`ESC[38;2;{r};{g};{b}m`):

```rust
AnsiColorType::Rgb(rgb_color) =>
    Color::Rgb(rgb_color.r(), rgb_color.g(), rgb_color.b())
```

### Effects Mapping

| ANSI Effect   | Code     | Ratatui Modifier        |
| ------------- | -------- | ----------------------- |
| Bold          | `ESC[1m` | `Modifier::BOLD`        |
| Dim           | `ESC[2m` | `Modifier::DIM`         |
| Italic        | `ESC[3m` | `Modifier::ITALIC`      |
| Underline     | `ESC[4m` | `Modifier::UNDERLINED`  |
| Blink         | `ESC[5m` | `Modifier::SLOW_BLINK`  |
| Reverse       | `ESC[7m` | `Modifier::REVERSED`    |
| Hidden        | `ESC[8m` | `Modifier::HIDDEN`      |
| Strikethrough | `ESC[9m` | `Modifier::CROSSED_OUT` |

## Common Patterns

### 1. Stripping ANSI for Display

```rust
use vtcode_core::utils::ansi_parser::strip_ansi;

let raw_output = "\x1b[31mError\x1b[0m: failed";
let clean = strip_ansi(raw_output);
// clean == "Error: failed"
```

### 2. Converting ANSI to Ratatui Style

```rust
use vtcode_core::utils::anstyle_utils::ansi_style_to_ratatui_style;
use anstyle::Style as AnsiStyle;

let ansi_style = AnsiStyle::new()
    .fg_color(Some(AnsiColor::Red))
    .bold();
let ratatui_style = ansi_style_to_ratatui_style(ansi_style);
```

### 3. Rendering Styled Text

```rust
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

let mut renderer = AnsiRenderer::stdout();
renderer.render("Error", MessageStyle::Error);
// Outputs: \x1b[31mError\x1b[0m
```

## Testing

### ANSI Parser Tests

**Location**: `vtcode-core/src/utils/ansi_parser.rs`

```rust
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
```

### Style Conversion Tests

**Location**: `vtcode-core/src/utils/anstyle_utils.rs`

```rust
#[test]
fn test_ansi_color_conversion() {
    assert_eq!(
        ansi_color_to_ratatui_color(&AnsiColorEnum::Ansi(AnsiColor::Red)),
        Color::Red
    );
}

#[test]
fn test_ansi_style_to_ratatui_style() {
    let ansi_style = anstyle::Style::new()
        .fg_color(Some(AnsiColorEnum::Ansi(AnsiColor::Green)))
        .bg_color(Some(AnsiColorEnum::Ansi(AnsiColor::Blue)))
        .bold();
    let ratatui_style = ansi_style_to_ratatui_style(ansi_style);
    // Verify colors and modifiers
}
```

## Best Practices

### 1. Always Strip ANSI for Token Counting

```rust
//  Good
let cleaned = strip_ansi(&pty_output);
let token_count = count_tokens(&cleaned);

//  Bad - ANSI codes inflate token count
let token_count = count_tokens(&pty_output);
```

### 2. Preserve ANSI for Raw Output

```rust
// When saving to file or passing to external tools
let raw_output = pty_result.output; // Keep ANSI intact
fs::write("output.log", raw_output)?;
```

### 3. Use Ratatui Styles in TUI

```rust
//  Good - Use Ratatui's style system
let style = Style::default().fg(Color::Red).add_modifier(Modifier::BOLD);
Span::styled("Error", style)

//  Bad - Don't embed ANSI in TUI text
Span::raw("\x1b[31mError\x1b[0m") // Ratatui will render this literally
```

### 4. Handle Non-UTF8 Gracefully

```rust
//  Good
if let Ok(s) = std::str::from_utf8(chunk) {
    let clean = strip_ansi(s);
    // Process clean text
}

//  Bad
let s = String::from_utf8(chunk).unwrap(); // May panic
```

## Reference Implementation

For complete ANSI sequence reference, see:

-   `docs/reference/ansi-escape-sequences.md` - Full ANSI reference
-   `vtcode-core/src/utils/ansi_parser.rs` - Stripping implementation
-   `vtcode-core/src/utils/anstyle_utils.rs` - Style conversion
-   `vtcode-core/src/utils/ansi.rs` - Rendering utilities

## Future Enhancements

1. **Parse ANSI for Structured Output**

    - Extract color information for semantic analysis
    - Detect error patterns by color (red = error)

2. **Preserve Formatting in Logs**

    - Option to keep ANSI in log files
    - HTML export with color preservation

3. **Custom Color Schemes**

    - User-configurable color mappings
    - Theme support for TUI

4. **Advanced Cursor Control**
    - Implement cursor position tracking
    - Support for complex TUI layouts

## Summary

VT Code has comprehensive ANSI support:

-   Stripping for clean text processing
-   Parsing for style extraction
-   Conversion to Ratatui styles
-   Rendering for terminal output
-   Full color support (8/16/256/RGB)
-   All standard text effects
-   Cursor and screen control

The implementation follows best practices and is well-tested.
