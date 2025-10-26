# File Reference Tree Panic Fix

## Problem
The tree view was panicking with the error:
```
thread 'tokio-runtime-worker' panicked at vtcode-core/src/ui/tui/session/file_tree.rs:120:70:
Failed to create tree item: Custom { kind: AlreadyExists, error: "The children contain duplicate identifiers" }
```

## Root Cause
The `tui-tree-widget` library requires each `TreeItem` to have a unique identifier. The tree building code was using the `path` field as the identifier, but directory nodes were being created with empty strings (`String::new()`) as their paths, causing duplicates.

```rust
// OLD CODE - BROKEN
let mut new_node = FileTreeNode {
    name: name.to_string(),
    path: if is_last {
        full_path.to_string()
    } else {
        String::new() // ❌ Multiple directories get empty path = duplicate identifiers
    },
    is_dir: !is_last,
    children: Vec::new(),
};
```

## Solution
Construct unique full paths for directory nodes by joining the parent path with the node name:

```rust
// NEW CODE - FIXED
let node_path = if is_last {
    full_path.to_string()
} else {
    // For directories, construct the path from parent path + name
    let parent_path = &self.path;
    if parent_path.is_empty() || parent_path == &workspace.to_string_lossy().to_string() {
        workspace.join(name).to_string_lossy().to_string()
    } else {
        Path::new(parent_path).join(name).to_string_lossy().to_string()
    }
};

let mut new_node = FileTreeNode {
    name: name.to_string(),
    path: node_path, // ✅ Unique path for every node
    is_dir: !is_last,
    children: Vec::new(),
};
```

## Example
For a file structure:
```
workspace/
  src/
    main.rs
  tests/
    test.rs
```

**Before (broken):**
- `src/` → path: `""`
- `tests/` → path: `""` (DUPLICATE!)
- `main.rs` → path: `"/workspace/src/main.rs"`

**After (fixed):**
- `src/` → path: `"/workspace/src"`
- `tests/` → path: `"/workspace/tests"` (UNIQUE!)
- `main.rs` → path: `"/workspace/src/main.rs"`

## Changes Made
**File:** `vtcode-core/src/ui/tui/session/file_tree.rs`

1. Added `workspace` parameter to `insert_components()` method
2. Constructed unique paths for directory nodes
3. Updated recursive calls to pass workspace through

## Testing
✅ All 13 tests passing:
- 10 file_palette tests
- 3 file_tree tests

✅ Cargo check succeeds with no errors

## Impact
- **Before:** Tree view would panic immediately on render
- **After:** Tree view works correctly with proper unique identifiers
- **Performance:** No impact - path construction happens once during tree building
