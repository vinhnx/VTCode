# Git Color Brightness Reduction

## Summary

Lowered the brightness of git background colors and text colors in the vtcode system to reduce eye strain during extended development sessions. All changes use ANSI 50% brightness reduction.

## Changes Made

### 1. Diff Background Colors (RGB)
**File**: `vtcode-core/src/utils/diff_styles.rs`

- **Added lines background**: Reduced from `RGB(0, 64, 0)` → `RGB(0, 32, 0)` (50% darker)
- **Removed lines background**: Reduced from `RGB(64, 0, 0)` → `RGB(32, 0, 0)` (50% darker)
- Foreground text colors remain unchanged for readability

### 2. Git Status Text Colors (ANSI Dimmed)
**File**: `vtcode-core/src/ui/git_config.rs`

- **Added files**: Changed from `green` → `green:dimmed` (50% brightness via DIMMED effect)
- **Modified files**: Changed from `red` → `red:dimmed` (50% brightness via DIMMED effect)
- **Deleted files**: Changed from `red` → `red:dimmed` (50% brightness via DIMMED effect)

### 3. Enhanced Color Name Parser
**File**: `vtcode-core/src/utils/style_helpers.rs`

Added support for color modifiers in `style_from_color_name()`:
- Syntax: `"color:modifier"` (e.g., `"green:dimmed"`, `"red:dimmed"`)
- Currently supports `:dimmed` modifier (case-insensitive)
- Can be extended for other modifiers like `:bold`, `:italic` in future

## Technical Details

### ANSI Dimmed Effect
The DIMMED effect in ANSI terminal colors reduces text brightness to approximately 50% of normal, making colors less harsh on the eyes while maintaining visibility and contrast.

### RGB Background Reduction
For diff backgrounds, RGB values were halved:
- `64 >> 1 = 32` (integer division)
- This provides a 50% brightness reduction while maintaining color hue

## Testing

All changes include:
- Unit tests for dimmed color variants
- Tests confirming 50% brightness reduction
- Backward compatibility maintained (colors without modifiers work as before)

## Configuration

Users can continue using git config color settings, which will override defaults:
- Git's color settings in `.git/config` take precedence
- Default dimmed colors apply only when git config doesn't specify colors
- Theme-based colors can also override these defaults

## Future Enhancements

The color modifier system is extensible:
- Add `:bright` for increased brightness (e.g., `"green:bright"`)
- Add effect combinations (e.g., `"red:dimmed:bold"`)
- Add custom RGB color support
