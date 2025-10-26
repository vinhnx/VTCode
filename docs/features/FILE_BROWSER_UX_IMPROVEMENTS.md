# File Browser UX Improvements

## Overview
Enhanced the file browser modal to show more items and provide better visual feedback about pagination.

## Changes Made

### 1. Increased Page Size
**Before:** 10 items per page
**After:** 20 items per page

```rust
// vtcode-core/src/ui/tui/session/file_palette.rs
const PAGE_SIZE: usize = 20;  // Changed from 10
```

**Benefit:** Users can see more files at once without scrolling, improving efficiency.

### 2. Added Continuation Indicator
When there are more items beyond the current page, a visual indicator is shown at the bottom:

```
  ... (30 more items)
```

**Implementation:**
- Added `has_more_items()` method to check if pagination continues
- Displays dimmed, italicized text showing how many items remain
- Automatically adjusts modal height to accommodate indicator

**Visual Style:**
- Prefix: `  ...` (indented like files)
- Style: Dim + Italic
- Content: `({N} more items)` where N = remaining items

## Example Display

```
┌─ File Browser (Page 1/3) ──────────┐
│ ↑↓ Navigate · PgUp/PgDn Page · ... │
│ Showing 50 files (List view)       │
│                                     │
│ ▶ src/                              │
│ ▶ tests/                            │
│   README.md                         │
│   Cargo.toml                        │
│   main.rs                           │
│   ... (17 more files here)          │
│   ... (30 more items)               │  ← New continuation indicator
└─────────────────────────────────────┘
```

## User Benefits

1. **More Visible Items**: 2x increase in items shown per page (10 → 20)
2. **Clear Feedback**: Users know immediately if there are more items
3. **Less Scrolling**: Fewer page navigation actions needed
4. **Better Context**: Item count helps users understand list size

## Technical Details

### New Method
```rust
pub fn has_more_items(&self) -> bool {
    let end = ((self.current_page + 1) * PAGE_SIZE).min(self.filtered_files.len());
    end < self.filtered_files.len()
}
```

### Rendering Logic
```rust
// Add continuation indicator if there are more items
if palette.has_more_items() {
    let continuation_text = format!("  ... ({} more items)", 
        palette.total_items() - (palette.current_page_number() * 20));
    let continuation_style = self.default_style()
        .add_modifier(Modifier::DIM | Modifier::ITALIC);
    list_items.push(ListItem::new(Line::from(Span::styled(
        continuation_text,
        continuation_style,
    ))));
}
```

### Modal Height Calculation
```rust
let has_continuation = palette.has_more_items();
let modal_height = items.len() 
    + instructions.len() 
    + 2  // borders
    + if has_continuation { 1 } else { 0 };  // continuation indicator
```

## Testing

### Updated Tests
Modified `test_pagination()` to reflect new PAGE_SIZE:

```rust
#[test]
fn test_pagination() {
    let files: Vec<String> = (0..50).map(|i| format!("file{}.rs", i)).collect();
    palette.load_files(files);

    // With PAGE_SIZE=20, 50 files = 3 pages (20 + 20 + 10)
    assert_eq!(palette.total_pages(), 3);
    assert_eq!(palette.current_page_items().len(), 20);
    assert!(palette.has_more_items());  // ← New assertion

    palette.page_down();
    assert!(palette.has_more_items());  // Still more on page 2

    palette.page_down();
    assert!(!palette.has_more_items()); // No more on last page
}
```

### Test Results
✅ All 12 file_palette tests passing
✅ Cargo check succeeds

## Files Modified

1. **vtcode-core/src/ui/tui/session/file_palette.rs**
   - Changed `PAGE_SIZE` from 10 to 20
   - Added `has_more_items()` method
   - Updated pagination test

2. **vtcode-core/src/ui/tui/session.rs**
   - Added continuation indicator rendering
   - Adjusted modal height calculation

## Future Enhancements

Potential improvements:
- [ ] Show page indicator in continuation text (e.g., "Page 2/5 has 15 more")
- [ ] Add keyboard shortcut to jump to last page
- [ ] Make PAGE_SIZE configurable via settings
- [ ] Add smooth scrolling animation when changing pages

## User Impact

**Positive:**
- Faster file browsing (less pagination needed)
- Better awareness of list size
- Improved discoverability of pagination

**None Negative:**
- Taller modals on small screens (but still responsive)
- Minimal performance impact (one extra calculation)

## Conclusion

The file browser now provides a better user experience with more visible items and clear pagination feedback through the continuation indicator.
