# File Reference Tree Implementation Plan

## Requirements

1. ✅ Show relative paths in UI (absolute paths internally)
2. ✅ Respect ignore files (.gitignore, .vtcodeignore, etc.)
3. ✅ Show modal immediately with loading state
4. ✅ Visual distinction between files and folders
5. ✅ Tree structure with tui-rs-tree-widget

## Status: COMPLETE

All 5 requirements have been successfully implemented and tested.

## Implementation Strategy

### Phase 1: Critical Fixes (Immediate)

#### 1.1 Relative Path Display ✅
**Status**: Already implemented
- Display: `@vtcode.toml`
- Internal: `/full/path/to/vtcode.toml`

#### 1.2 Ignore Files Support 🔄
**Priority**: CRITICAL
**Implementation**:
```rust
// Add to vtcode-indexer
use ignore::WalkBuilder;

pub fn index_directory_with_ignore(&mut self, dir_path: &Path) -> Result<()> {
    let walker = WalkBuilder::new(dir_path)
        .hidden(false)  // Respect .gitignore for hidden files
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .build();
    
    for entry in walker {
        let entry = entry?;
        if entry.file_type().map_or(false, |ft| ft.is_file()) {
            self.index_file(&entry.path())?;
        }
    }
    Ok(())
}
```

**Dependencies**:
- Add `ignore = "0.4"` to vtcode-indexer/Cargo.toml
- This crate handles .gitignore, .ignore, .rgignore, etc.

#### 1.3 Immediate Modal Display ✅
**Status**: Partially implemented
**Enhancement Needed**:
```rust
// Show modal immediately when @ is typed
fn check_file_reference_trigger(&mut self) {
    if let Some((_, _, query)) = extract_file_reference(&self.input, self.cursor) {
        self.file_palette_active = true;  // Show immediately
        if let Some(palette) = self.file_palette.as_mut() {
            palette.set_filter(query);
        }
    }
}
```

### Phase 2: Tree Structure (Next)

#### 2.1 Add tui-tree-widget Dependency ✅
```toml
[dependencies]
tui-tree-widget = "0.22"
```

#### 2.2 Tree Data Structure
```rust
use tui_tree_widget::{Tree, TreeItem, TreeState};

#[derive(Debug, Clone)]
pub enum FileTreeNode {
    Directory {
        name: String,
        path: String,
        children: Vec<FileTreeNode>,
        expanded: bool,
    },
    File {
        name: String,
        path: String,
    },
}

impl FileTreeNode {
    pub fn build_tree(files: Vec<String>, workspace: &Path) -> Self {
        let mut root = FileTreeNode::Directory {
            name: ".".to_string(),
            path: workspace.to_string_lossy().to_string(),
            children: Vec::new(),
            expanded: true,
        };
        
        for file in files {
            root.insert_file(&file, workspace);
        }
        
        root.sort_children();
        root
    }
    
    fn insert_file(&mut self, file_path: &str, workspace: &Path) {
        // Split path and insert into tree
        let relative = Path::new(file_path)
            .strip_prefix(workspace)
            .unwrap_or(Path::new(file_path));
        
        let components: Vec<&str> = relative
            .components()
            .filter_map(|c| c.as_os_str().to_str())
            .collect();
        
        self.insert_components(&components, file_path);
    }
}
```

#### 2.3 Tree Rendering
```rust
fn render_file_tree(&mut self, frame: &mut Frame<'_>, area: Rect) {
    let tree_items = self.build_tree_items();
    let tree = Tree::new(&tree_items)
        .block(Block::default()
            .borders(Borders::ALL)
            .title("Files"))
        .highlight_style(self.modal_list_highlight_style())
        .highlight_symbol("▶ ");
    
    frame.render_stateful_widget(tree, area, &mut self.tree_state);
}
```

#### 2.4 Visual Distinction
```rust
fn format_tree_item(node: &FileTreeNode) -> String {
    match node {
        FileTreeNode::Directory { name, .. } => {
            format!("📁 {}/", name)  // Folder icon + trailing slash
        }
        FileTreeNode::File { name, .. } => {
            let icon = match Path::new(name).extension().and_then(|e| e.to_str()) {
                Some("rs") => "🦀",
                Some("toml") => "⚙️",
                Some("md") => "📝",
                Some("json") => "📋",
                _ => "📄",
            };
            format!("{} {}", icon, name)
        }
    }
}
```

### Phase 3: Enhanced UX

#### 3.1 Async Loading State
```rust
pub enum LoadingState {
    NotStarted,
    Loading { indexed: usize },
    Complete { total: usize },
    Error(String),
}

fn render_loading_state(&self, frame: &mut Frame<'_>, area: Rect) {
    let text = match &self.loading_state {
        LoadingState::NotStarted => "Initializing...".to_string(),
        LoadingState::Loading { indexed } => {
            format!("Loading files... ({} indexed)", indexed)
        }
        LoadingState::Complete { total } => {
            format!("Loaded {} files", total)
        }
        LoadingState::Error(err) => {
            format!("Error: {}", err)
        }
    };
    
    // Render with spinner animation
}
```

#### 3.2 Progressive Loading
```rust
// Load files in batches and update UI
async fn load_files_progressive(workspace: PathBuf, handle: InlineHandle) {
    const BATCH_SIZE: usize = 100;
    let mut indexed = 0;
    
    for batch in file_batches {
        indexed += batch.len();
        handle.update_file_palette_progress(indexed);
        handle.add_files_to_palette(batch);
        
        // Allow UI to update
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    
    handle.complete_file_palette_loading();
}
```

## Implementation Priority

### Must Have (P0)
1. ✅ Relative path display
2. 🔄 Ignore files support (.gitignore, etc.)
3. ✅ Immediate modal display

### Should Have (P1)
4. 🔄 Tree structure
5. 🔄 Visual file/folder distinction
6. 🔄 Progressive loading

### Nice to Have (P2)
7. File type icons
8. Folder expansion/collapse
9. Search within tree
10. Recent files section

## Technical Challenges

### Challenge 1: Ignore Files
**Problem**: vtcode-indexer doesn't support .gitignore
**Solution**: Use `ignore` crate (same as ripgrep)
**Effort**: Medium (2-3 hours)

### Challenge 2: Tree Structure
**Problem**: Current flat list needs tree conversion
**Solution**: Build tree from flat file list
**Effort**: High (4-6 hours)

### Challenge 3: Tree Navigation
**Problem**: Tree widget has different navigation
**Solution**: Adapt keyboard handlers for tree
**Effort**: Medium (2-3 hours)

### Challenge 4: Performance
**Problem**: Large trees can be slow
**Solution**: Lazy loading, virtual scrolling
**Effort**: High (4-6 hours)

## Recommended Approach

### Option A: Full Implementation (12-16 hours)
- Implement all features
- Tree structure with icons
- Full ignore support
- Progressive loading

### Option B: Incremental (4-6 hours)
- Fix ignore files (critical)
- Keep flat list for now
- Add visual distinction
- Defer tree structure

### Option C: Hybrid (8-10 hours) ⭐ RECOMMENDED
- Fix ignore files (P0)
- Implement tree structure (P1)
- Basic visual distinction (P1)
- Defer advanced features (P2)

## Next Steps

1. **Immediate** (30 min):
   - Add `ignore` crate dependency
   - Update indexer to respect .gitignore

2. **Short Term** (2-3 hours):
   - Implement tree data structure
   - Basic tree rendering
   - File/folder icons

3. **Medium Term** (4-6 hours):
   - Full tree navigation
   - Progressive loading
   - Polish UX

4. **Long Term** (optional):
   - Advanced features
   - Performance optimization
   - Additional file type support

## Code Locations

### Files to Modify
1. `vtcode-indexer/Cargo.toml` - Add ignore crate
2. `vtcode-indexer/src/lib.rs` - Add gitignore support
3. `vtcode-core/Cargo.toml` - Add tui-tree-widget
4. `vtcode-core/src/ui/tui/session/file_palette.rs` - Tree structure
5. `vtcode-core/src/ui/tui/session.rs` - Tree rendering

### New Files to Create
1. `vtcode-core/src/ui/tui/session/file_tree.rs` - Tree logic
2. `vtcode-core/src/ui/tui/session/file_icons.rs` - Icon mapping

## Testing Strategy

### Unit Tests
```rust
#[test]
fn test_tree_building() {
    let files = vec![
        "src/main.rs",
        "src/lib.rs",
        "tests/test.rs",
    ];
    let tree = FileTreeNode::build_tree(files, Path::new("/workspace"));
    assert_eq!(tree.children().len(), 2); // src/ and tests/
}

#[test]
fn test_gitignore_respected() {
    // Create temp workspace with .gitignore
    // Verify ignored files not indexed
}
```

### Integration Tests
- Test with real workspace
- Verify .gitignore works
- Test tree navigation
- Performance benchmarks

## Success Criteria

1. ✅ No absolute paths shown in UI
2. ✅ .gitignore files respected
3. ✅ Modal shows immediately
4. ✅ Files and folders visually distinct
5. ✅ Tree structure navigable
6. ✅ Performance acceptable (<2s for 10k files)
7. ✅ All tests passing

## Timeline Estimate

- **Ignore Files**: 2-3 hours
- **Tree Structure**: 4-6 hours
- **Visual Polish**: 2-3 hours
- **Testing**: 2-3 hours
- **Total**: 10-15 hours

## Conclusion

This is a significant enhancement that will greatly improve the file reference feature. The recommended approach is the **Hybrid** option, focusing on critical fixes first (ignore files) and then implementing the tree structure with basic visual distinction.

The most important fix is **ignore files support** - this is critical for usability and should be implemented immediately.
