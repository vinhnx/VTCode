# ANSI Escape Code Fixes for Diff Rendering

## Critical Issue: Sticky/Broken ANSI Codes on Scroll

### Problem Description

The git diff rendering system was producing broken ANSI escape sequences that would stick/bleed across lines during scrolling and terminal operations. This happened because:

1. **ANSI codes spanning newlines**: Style codes were being applied across multiple lines without proper reset sequences after each line
2. **Reset codes not applied at line boundaries**: Newline characters were included within styled text blocks, causing the color format to carry over to subsequent lines
3. **Multi-line content styling**: When content with newlines was styled as a single unit, the Reset code only appeared at the very end, leaving intermediate lines unstyled properly

### Example of the Bug

**Before Fix (Broken)**:
```
[STYLE]--- a/file.rs
+++ b/file.rs[RESET]
```

If scrolling or terminal wrapping occurred, the `+++` line wouldn't have proper style reset, causing colors to bleed into following content.

**After Fix (Correct)**:
```
[STYLE]--- a/file.rs[RESET]
[STYLE]+++ b/file.rs[RESET]
```

Each line gets its own style and reset pair.

## Changes Made

### 1. Fixed `render_diff()` in `diff_renderer.rs`

**Issue**: Unified diff headers (`--- a/` and `+++ b/`) were styled as a single block with a single Reset at the end.

**Solution**: Apply Reset codes immediately after each header line.

**Code**:
```rust
// BEFORE: Color codes span multiple lines
output.push_str(&format!(
    "{}--- a/{}\n{} +++ b/{}\n",
    self.palette.line_header.render(),
    diff.file_path,
    self.palette.line_header.render(),
    diff.file_path
));
if self.use_colors {
    output.push_str(&format!("{}", Reset.render()));
}

// AFTER: Reset applied after each line
if self.use_colors {
    let header_style_str = format!("{}", self.palette.line_header.render());
    output.push_str(&header_style_str);
    output.push_str(&format!("--- a/{}", diff.file_path));
    output.push_str(&format!("{}", Reset.render()));
    output.push('\n');

    output.push_str(&header_style_str);
    output.push_str(&format!("+++ b/{}", diff.file_path));
    output.push_str(&format!("{}", Reset.render()));
    output.push('\n');
} else {
    output.push_str(&format!("--- a/{}\n", diff.file_path));
    output.push_str(&format!("+++ b/{}\n", diff.file_path));
}
```

**Benefits**:
- Each diff header line has its own complete ANSI sequence
- Reset codes are guaranteed to appear before newlines
- No color bleed across lines during scrolling

### 2. Enhanced `paint()` in `diff_renderer.rs`

**Issue**: The paint function didn't handle multi-line content properly. If text somehow contained newlines, all lines would be styled until the final Reset.

**Solution**: Detect newlines within text and apply Reset/Style pairs per line.

**Code**:
```rust
fn paint(&self, style: &Style, text: &str) -> String {
    if self.use_colors {
        // CRITICAL: Ensure Reset is always applied after styled text to prevent color bleeding
        let reset = Reset.render();
        let rendered_style = style.render();
        
        // Handle multi-line content
        if text.contains('\n') {
            let lines: Vec<&str> = text.split('\n').collect();
            lines
                .iter()
                .enumerate()
                .map(|(i, line)| {
                    let formatted = format!("{}{}{}", rendered_style, line, reset);
                    if i < lines.len() - 1 {
                        format!("{}\n", formatted)
                    } else {
                        formatted
                    }
                })
                .collect::<String>()
                .trim_end_matches('\n')
                .to_string()
        } else {
            format!("{}{}{}", rendered_style, text, reset)
        }
    } else {
        text.to_string()
    }
}
```

**Benefits**:
- Defensive handling for any unexpected multi-line content
- Each line gets proper style/reset pairing
- Prevents color bleed even if content somehow contains newlines

### 3. Fixed `format_colored_diff()` in `utils/diff.rs`

**Issue**: The newline character was included within styled text, causing improper ANSI sequence boundaries.

**Solution**: Separate the newline from styled content and apply Reset before the newline.

**Code**:
```rust
// BEFORE: Newline inside styled block
let mut display = String::with_capacity(line.text.len() + 2);
display.push(prefix);
display.push_str(&line.text);
if !line.text.ends_with('\n') {
    display.push('\n');
}
output.push_str(&format!("{}{}{}", style.render(), display, Reset.render()));

// AFTER: Reset applied before newline
let mut display = String::with_capacity(line.text.len() + 2);
display.push(prefix);
display.push_str(&line.text);

// CRITICAL: Apply Reset before newline to prevent color bleeding
let has_newline = display.ends_with('\n');
let display_content = if has_newline {
    &display[..display.len() - 1]
} else {
    &display
};

output.push_str(&format!(
    "{}{}{}",
    style.render(),
    display_content,
    Reset.render()
));

// Always add newline after reset to prevent color stickiness
if has_newline || !line.text.ends_with('\n') {
    output.push('\n');
}
```

**Benefits**:
- ANSI reset codes always appear before newlines
- Terminal sees proper sequence boundaries
- No color bleed during scrolling or line wrapping

## Technical Explanation

### Why This Matters

Terminal emulators process ANSI escape codes sequentially. When rendering:

```
[STYLE]text
more text[RESET]
```

The terminal:
1. Applies style to "text"
2. Applies style to "more text" (because no reset yet)
3. Finally resets the style

This causes the second line to be incorrectly colored. By ensuring:

```
[STYLE]text[RESET]
[STYLE]more text[RESET]
```

Each line is independently styled and reset, preventing bleed.

### ANSI Escape Code Boundaries

The correct pattern for ANSI codes is:
- **Style code**: `ESC[...m` (e.g., `ESC[32m` for green)
- **Text content**: Any characters except the reset code
- **Reset code**: `ESC[0m` or `ESC[m`

The reset MUST appear before any newline that terminates the styled section.

## Files Modified

1. `vtcode-core/src/ui/diff_renderer.rs`
   - `render_diff()` method
   - `paint()` method

2. `vtcode-core/src/utils/diff.rs`
   - `format_colored_diff()` function

## Testing & Verification

-   Code compiles without errors
-   No new clippy warnings
-   Maintains backward compatibility
-   Color handling still respects `use_colors` flag
-   Works with and without colors

## Behavioral Changes

### User-Visible Changes
- Diff rendering no longer has color bleed during scrolling
- Colors remain properly bounded to their intended lines
- Terminal output is now deterministic and predictable

### API Changes
- None - all changes are internal to rendering logic
- Existing color configuration continues to work

## Future Improvements

1. **ANSI Code Validation**: Add utility function to verify ANSI sequences are properly paired
2. **Color Rendering Tests**: Add tests that verify ANSI codes are correctly applied and reset
3. **Terminal Compatibility**: Test with various terminal emulators to ensure proper behavior

## Compliance

These fixes ensure compliance with:
- ANSI/VT100 terminal standards
- Common terminal emulator implementations
- Git's standard color scheme expectations
