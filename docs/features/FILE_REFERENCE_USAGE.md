# File Reference Feature - Usage Guide

## Quick Start

The file reference feature allows you to quickly browse and insert file references into your chat messages using a keyboard-driven interface.

## How to Use

### Opening the File Browser

Type `@` in the chat input to open the file browser modal:

```
❯ @
```

The file browser will appear immediately, showing a loading state while files are being indexed.

### Display Modes

The file browser supports two display modes:

#### Tree View (Default)
Shows files in a hierarchical tree structure:
```
▶ src/
  ├─ ▶ agent/
  │   ├─ mod.rs
  │   └─ runloop.rs
  ├─ main.rs
  └─ lib.rs
▶ tests/
  └─ test.rs
```

#### List View
Shows files in a flat list:
```
▶ src/
  agent/mod.rs
  agent/runloop.rs
  main.rs
  lib.rs
▶ tests/
  test.rs
```

### Keyboard Controls

#### Navigation
- **↑/↓**: Move selection up/down
- **PgUp/PgDn**: Jump between pages
- **Home**: Jump to first item
- **End**: Jump to last item

#### Actions
- **Enter**: Select file and insert reference
- **Tab**: Select file and insert reference (alternative)
- **t**: Toggle between tree and list view
- **Esc**: Close file browser without selecting

#### Filtering
Type any text after `@` to filter files:
```
❯ @main
```
This will show only files matching "main" in their path.

### Visual Indicators

#### Directories
- Prefix: `▶`
- Style: Bold
- Suffix: `/` (trailing slash)
- Example: `▶ src/`

#### Files
- Prefix: `  ` (two spaces for indentation)
- Style: Normal
- Example: `  main.rs`

### File Selection

When you select a file (Enter or Tab), the reference is inserted at the cursor position:

```
Before: ❯ @main
After:  ❯ @main.rs
```

The file browser closes automatically after selection.

## Features

### Ignore Files Support

The file browser respects your project's ignore files:
- `.gitignore`
- `.ignore`
- `.git/info/exclude`

This means you won't see:
- `node_modules/`
- `target/`
- `.git/`
- Other ignored files/directories

### Smart Sorting

Files are sorted intelligently:
1. Directories appear first
2. Files appear after directories
3. Within each category, items are sorted alphabetically (case-insensitive)

### Relative Paths

The file browser displays relative paths from your workspace root:
- Display: `src/main.rs`
- Not: `/Users/you/project/src/main.rs`

This keeps the interface clean and readable.

### Pagination

For large file lists, the browser automatically paginates:
```
File Browser (Page 1/5)
```

Use PgUp/PgDn to navigate between pages.

## Examples

### Example 1: Basic File Selection

```
1. Type: @
2. Navigate: ↓↓↓ (to select src/main.rs)
3. Select: Enter
4. Result: @src/main.rs
```

### Example 2: Filtered Search

```
1. Type: @test
2. Browser shows only files matching "test"
3. Navigate: ↓ (to select tests/integration.rs)
4. Select: Enter
5. Result: @tests/integration.rs
```

### Example 3: Tree Navigation

```
1. Type: @
2. Toggle: t (switch to tree view)
3. Navigate: ↓↓↓ (browse tree structure)
4. Select: Enter
5. Result: @path/to/file.rs
```

### Example 4: Cancel Selection

```
1. Type: @
2. Navigate: ↓↓↓
3. Cancel: Esc
4. Result: @ (browser closes, no selection)
```

## Tips and Tricks

### Tip 1: Quick Filter
Start typing immediately after `@` to filter files:
```
@config  # Shows only files matching "config"
```

### Tip 2: Toggle Views
Press `t` to switch between tree and list views to find the view that works best for your workflow.

### Tip 3: Use Pagination
For large projects, use PgUp/PgDn to quickly jump through pages instead of holding ↓.

### Tip 4: Home/End Keys
Use Home to jump to the first file or End to jump to the last file in the list.

### Tip 5: Ignore Files
Add files to `.gitignore` to keep your file browser clean and focused on relevant files.

## Performance

The file browser is optimized for performance:
- **Fast Indexing**: 1-2 seconds for typical projects
- **Efficient Filtering**: Instant results as you type
- **Memory Efficient**: Minimal memory overhead
- **Respects Ignore Files**: Only indexes relevant files

### Performance Comparison

**Before (without ignore files)**:
- 50,000+ files indexed
- 10+ seconds to load
- Cluttered with node_modules, target, etc.

**After (with ignore files)**:
- 500-1000 files indexed
- 1-2 seconds to load
- Clean, focused file list

## Troubleshooting

### Problem: Too many files shown
**Solution**: Add unwanted directories to `.gitignore`

### Problem: File not appearing
**Solution**: Check if the file is in `.gitignore` or another ignore file

### Problem: Slow loading
**Solution**: Add large directories (node_modules, target) to `.gitignore`

### Problem: Can't find file
**Solution**: Use the filter by typing after `@`, e.g., `@filename`

## Configuration

The file browser respects your project's configuration:
- Workspace root is automatically detected
- Ignore files are automatically loaded
- No additional configuration needed

## Keyboard Reference Card

```
┌─────────────────────────────────────────┐
│         File Browser Controls           │
├─────────────────────────────────────────┤
│ Navigation                              │
│   ↑/↓         Move selection            │
│   PgUp/PgDn   Jump pages                │
│   Home/End    Jump to first/last        │
│                                         │
│ Actions                                 │
│   Enter/Tab   Select file               │
│   t           Toggle tree/list view     │
│   Esc         Close browser             │
│                                         │
│ Filtering                               │
│   @text       Filter files by text      │
└─────────────────────────────────────────┘
```

## Advanced Usage

### Multiple File References

You can insert multiple file references in a single message:

```
❯ I need to refactor @src/main.rs and @src/lib.rs
```

Just type `@` again after inserting the first reference.

### Combining with Other Features

File references work seamlessly with other chat features:
```
❯ /ask How can I improve @src/main.rs?
```

### Workspace Context

The file browser is workspace-aware:
- Only shows files from your current workspace
- Respects workspace-specific ignore files
- Uses workspace root for relative paths

## Best Practices

1. **Keep .gitignore Updated**: Add build artifacts and dependencies to keep the file list clean
2. **Use Filters**: Type after `@` to quickly find files
3. **Choose Your View**: Use tree view for structure, list view for speed
4. **Use Pagination**: Don't scroll through hundreds of files, use PgUp/PgDn
5. **Close When Done**: Press Esc if you change your mind

## Summary

The file reference feature provides a fast, keyboard-driven way to browse and insert file references into your chat messages. With support for tree and list views, smart filtering, and ignore files, it's designed to help you work efficiently in any project size.

**Key Features**:
- ✅ Instant file browser with `@`
- ✅ Tree and list views
- ✅ Smart filtering
- ✅ Ignore files support
- ✅ Keyboard-driven navigation
- ✅ Fast and efficient

Happy coding!
