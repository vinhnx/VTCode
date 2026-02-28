# ANSI Quick Reference for VT Code Development

## Most Common Sequences

### Colors (Foreground)

```
\x1b[30m  Black
\x1b[31m  Red       ← Errors
\x1b[32m  Green     ← Success
\x1b[33m  Yellow    ← Warnings
\x1b[34m  Blue      ← Info
\x1b[35m  Magenta
\x1b[36m  Cyan
\x1b[37m  White
\x1b[39m  Default   ← Reset to default
```

### Bright Colors (90-97)

```
\x1b[91m  Bright Red
\x1b[92m  Bright Green
\x1b[93m  Bright Yellow
\x1b[94m  Bright Blue
\x1b[95m  Bright Magenta
\x1b[96m  Bright Cyan
\x1b[97m  Bright White
```

### Text Styles

```
\x1b[0m   Reset all
\x1b[1m   Bold
\x1b[2m   Dim
\x1b[3m   Italic
\x1b[4m   Underline
\x1b[7m   Reverse (invert fg/bg)
\x1b[9m   Strikethrough
\x1b[22m  Normal intensity (reset bold/dim)
\x1b[23m  Not italic
\x1b[24m  Not underlined
```

### Cursor Control

```
\x1b[H      Home (0,0)
\x1b[{n}A   Up n lines
\x1b[{n}B   Down n lines
\x1b[{n}C   Right n columns
\x1b[{n}D   Left n columns
\x1b[{r};{c}H  Move to row r, column c
\x1b[?25l   Hide cursor
\x1b[?25h   Show cursor
```

### Erase Functions

```
\x1b[2J   Clear entire screen
\x1b[K    Clear from cursor to end of line
\x1b[2K   Clear entire line
\x1b[0J   Clear from cursor to end of screen
\x1b[1J   Clear from cursor to beginning of screen
```

### Screen Modes

```
\x1b[?1049h  Enable alternative buffer
\x1b[?1049l  Disable alternative buffer
\x1b[?47h    Save screen
\x1b[?47l    Restore screen
```

## VT Code Usage Examples

### Stripping ANSI

```rust
use vtcode_core::utils::ansi_parser::strip_ansi;

// Remove all ANSI codes
let clean = strip_ansi("\x1b[31mError\x1b[0m");
// clean == "Error"
```

### In-place Redraw (Progress/Spinner)

```rust
use vtcode_core::utils::ansi_codes::{format_redraw_line, redraw_line_prefix};

let prefix = redraw_line_prefix(); // "\r\x1b[2K"
let frame = format_redraw_line("Building... 42%");
```

### Detecting ANSI Sequences

```rust
// Check if string contains ANSI codes
fn has_ansi(text: &str) -> bool {
    text.contains("\x1b[")
}
```

### Common Patterns in PTY Output

#### Cargo Output

```
\x1b[0m\x1b[1m\x1b[32m   Compiling\x1b[0m vtcode v0.45.6
     ^^^^^^^^^^^^^^ Bold Green "Compiling"

\x1b[0m\x1b[1m\x1b[33mwarning\x1b[0m: unused variable
     ^^^^^^^^^^^^^^ Bold Yellow "warning"

\x1b[0m\x1b[1m\x1b[31merror\x1b[0m: could not compile
     ^^^^^^^^^^^^^^ Bold Red "error"
```

#### Git Output

```
\x1b[32m+\x1b[0m Added line
     ^^^^ Green "+"

\x1b[31m-\x1b[0m Removed line
     ^^^^ Red "-"

\x1b[36m@@\x1b[0m Hunk header
     ^^^^^ Cyan "@@"
```

## Regex Patterns

### Match Any ANSI Sequence

```rust
// Simple pattern (most common)
r"\x1b\[[0-9;]*[a-zA-Z]"

// Comprehensive pattern (all CSI sequences)
r"\x1b\[[0-9;?]*[a-zA-Z]"

// All escape sequences (CSI + OSC + others)
r"\x1b(\[[0-9;?]*[a-zA-Z]|\][^\x07]*\x07|[=>])"
```

### Extract Color Codes

```rust
// Match foreground color: ESC[3Xm or ESC[9Xm
r"\x1b\[(3[0-7]|9[0-7])m"

// Match 256-color: ESC[38;5;{ID}m
r"\x1b\[38;5;(\d+)m"

// Match RGB: ESC[38;2;{r};{g};{b}m
r"\x1b\[38;2;(\d+);(\d+);(\d+)m"
```

## Testing Helpers

### Generate Test Strings

```rust
// Red text
format!("\x1b[31m{}\x1b[0m", "Error")

// Bold green
format!("\x1b[1;32m{}\x1b[0m", "Success")

// Multiple styles
format!("\x1b[1m\x1b[4m\x1b[33m{}\x1b[0m", "Warning")
```

### Verify Stripping

```rust
#[test]
fn test_strip_preserves_text() {
    let input = "\x1b[31mRed\x1b[0m \x1b[32mGreen\x1b[0m";
    assert_eq!(strip_ansi(input), "Red Green");
}
```

## Common Mistakes

### Don't embed ANSI in Ratatui

```rust
// BAD - Ratatui will render escape codes literally
Span::raw("\x1b[31mError\x1b[0m")
```

### Use Ratatui styles instead

```rust
// GOOD
Span::styled("Error", Style::default().fg(Color::Red))
```

### Don't count tokens with ANSI

```rust
// BAD - ANSI codes inflate count
let tokens = count_tokens(&pty_output);
```

### Strip first

```rust
// GOOD
let clean = strip_ansi(&pty_output);
let tokens = count_tokens(&clean);
```

## Debugging ANSI Issues

### View Raw Bytes

```rust
// Print hex representation
for byte in text.bytes() {
    print!("{:02x} ", byte);
}
println!();

// ESC = 0x1b, [ = 0x5b
// So "\x1b[31m" = 1b 5b 33 31 6d
```

### Visualize ANSI Codes

```rust
// Replace ESC with visible marker
let visible = text.replace("\x1b", "␛");
println!("{}", visible);
// Output: ␛[31mRed␛[0m
```

### Check for Incomplete Sequences

```rust
// Detect truncated ANSI codes
fn has_incomplete_ansi(text: &str) -> bool {
    text.ends_with("\x1b") ||
    text.ends_with("\x1b[") ||
    (text.contains("\x1b[") && !text.contains("m"))
}
```

## Performance Tips

### Pre-compile Regex

```rust
use once_cell::sync::Lazy;
use regex::Regex;

static ANSI_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]").unwrap()
});

// Use in hot path
let clean = ANSI_REGEX.replace_all(text, "");
```

### Avoid Repeated Stripping

```rust
//  BAD - strips multiple times
for line in output.lines() {
    let clean = strip_ansi(line);
    process(clean);
}

//  GOOD - strip once
let clean_output = strip_ansi(&output);
for line in clean_output.lines() {
    process(line);
}
```

## Quick Reference Card

```

 ANSI Quick Reference

 Reset:        \x1b[0m
 Bold:         \x1b[1m
 Dim:          \x1b[2m
 Red:          \x1b[31m
 Green:        \x1b[32m
 Yellow:       \x1b[33m
 Blue:         \x1b[34m
 Clear line:   \x1b[2K
 Hide cursor:  \x1b[?25l
 Show cursor:  \x1b[?25h
 Alt buffer:   \x1b[?1049h (enable) \x1b[?1049l (disable)

```

## See Also

-   `docs/reference/ansi-escape-sequences.md` - Full ANSI reference
-   `docs/reference/ansi-in-vtcode.md` - VT Code-specific usage
-   `vtcode-core/src/utils/ansi_parser.rs` - Implementation
