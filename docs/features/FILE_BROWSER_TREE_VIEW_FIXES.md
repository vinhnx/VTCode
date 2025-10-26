# File Browser: Tree View Fixes & Improvements

## Overview
Fixed several UX issues with the file browser tree view and set it as the default display mode.

## Changes Made

### 1. ✅ Fixed @ Symbol Detection (No False Positives)

**Issue:** Concern that system might confuse "@" with "a" 

**Status:** Already working correctly! The `extract_file_reference` function properly requires the "@" symbol.

**Test Added:**
```rust
#[test]
fn test_no_false_positive_with_a() {
    // Standalone "a" without @ - should NOT trigger
    let input = "a";
    assert_eq!(extract_file_reference(input, 1), None);
    
    // "a" in middle of text - should NOT trigger
    let input = "write a function";
    assert_eq!(extract_file_reference(input, 7), None);
}
```

**Result:** ✅ File browser only triggers with actual "@" symbol

### 2. ✅ Tree View is Now Default

**Before:** Default was List view
**After:** Default is Tree view

**Change:**
```rust
// vtcode-core/src/ui/tui/session/file_palette.rs
display_mode: DisplayMode::Tree,  // Default to tree view
```

**Rationale:** Tree view provides better visual hierarchy for project structure

### 3. ✅ Removed Duplicate Arrow Icons

**Issue:** Folders showed duplicate arrows (e.g., "▶ ▶ src/")

**Root Cause:** Code was adding "▶" prefix, but tui-tree-widget adds its own arrow

**Fix:**
```rust
// BEFORE
let display_text = if self.is_dir {
    format!("▶ {}/", self.name)  // ❌ Duplicate arrow!
} else {
    format!("  {}", self.name)
};

// AFTER  
let display_text = if self.is_dir {
    format!("{}/", self.name)  // ✅ Widget adds arrow
} else {
    self.name.clone()
};
```

**Result:** Clean display with single arrow per folder

### 4. ⚠️ Folder Styling (Limited by Library)

**Desired:** Make folders bold using ANSI styling

**Challenge:** The `tui-tree-widget` library doesn't support per-item styling

**Current Solution:** 
- Folders have "/" suffix for visual distinction
- Tree widget's own arrow provides additional visual cue
- Base tree style can be modified globally but not per-item

**Possible Future Solutions:**
- Fork/extend tui-tree-widget to support Ratatui Spans
- Switch to a different tree widget library
- Build custom tree rendering

**Current Display:**
```
▶ src/       ← Arrow + "/" = clearly a folder
  main.rs    ← No prefix = clearly a file
```

### 5. ✅ Tree Expansion (Already Synchronous)

**Issue:** Request for async tree expansion handling

**Current Behavior:** 
- Tree is built from already-indexed files
- Expansion/collapse is handled synchronously by TreeState
- No file system access during expansion (data pre-loaded)

**No Action Needed:** System already works optimally!

**How it Works:**
1. File indexer loads all files upfront
2. Tree structure is built in memory
3. TreeState manages expand/collapse state
4. No async operations required

## Visual Comparison

### Before (Duplicate Arrows)
```
┌─ File Browser (Tree View) ─┐
│ ▶ ▶ src/                    │  ← Duplicate!
│   ▶ ▶ models/               │  ← Duplicate!
│       user.rs               │
│   ▶ ▶ views/                │  ← Duplicate!
└─────────────────────────────┘
```

### After (Clean)
```
┌─ File Browser (Tree View) ─┐
│ ▶ src/                      │  ← Single arrow
│   ▶ models/                 │  ← Single arrow
│       user.rs               │
│   ▶ views/                  │  ← Single arrow
└─────────────────────────────┘
```

## Updated Tests

### Tests Modified for Tree Default
Since tree is now default, tests that rely on list-specific behavior need to explicitly set list mode:

```rust
#[test]
fn test_pagination() {
    let mut palette = FilePalette::new(PathBuf::from("/workspace"));
    palette.display_mode = DisplayMode::List;  // Force list mode
    // ... rest of test
}
```

**Tests Updated:**
- `test_pagination` - Forces list mode
- `test_circular_navigation` - Forces list mode

## Testing

✅ **All tests passing:** 29 file-related tests (17 file_palette + 3 file_tree + 9 others)
✅ **Cargo check:** No errors
✅ **New test:** `test_no_false_positive_with_a`

## Keyboard Navigation (Unchanged)

Tree mode navigation remains the same:
- **↑/↓** - Navigate items
- **←/→** - Collapse/Expand folders
- **Enter** - Toggle expand/collapse
- **PgUp/PgDn** - Collapse/Expand (alternative)
- **Tab** - Select file
- **t** - Toggle to list view
- **Esc** - Close browser

## Files Modified

1. **vtcode-core/src/ui/tui/session/file_palette.rs**
   - Changed default display mode to Tree
   - Added test for @ symbol false positives
   - Fixed tests to explicitly use list mode where needed

2. **vtcode-core/src/ui/tui/session/file_tree.rs**
   - Removed duplicate arrow prefix
   - Simplified display text generation

3. Created: **docs/features/FILE_BROWSER_TREE_VIEW_FIXES.md**

## Known Limitations

### Per-Item Styling
The current tui-tree-widget library doesn't support per-item text styling (bold, colors, etc.). To implement bold folders, we would need to:

1. **Option A:** Fork tui-tree-widget and add Span support
2. **Option B:** Switch to a different tree widget
3. **Option C:** Build custom tree rendering from scratch

**Workaround:** The "/" suffix and tree arrows provide sufficient visual distinction.

## User Impact

✅ **Cleaner UI** - No duplicate arrows
✅ **Better default** - Tree view shows structure better
✅ **No regressions** - @ detection still works perfectly
✅ **Same performance** - Tree expansion remains fast

## Future Enhancements

Potential improvements:
- [ ] Custom tree widget with per-item styling support
- [ ] Folder icons (if terminal supports Unicode)
- [ ] Color-coded file types
- [ ] Collapsible tree state persistence
- [ ] Breadcrumb navigation in tree mode

## Conclusion

File browser tree view is now cleaner (no duplicate arrows), set as default, and properly handles @ symbol detection. Per-item styling is limited by the library but visual distinction is achieved through "/" suffixes and tree structure.
