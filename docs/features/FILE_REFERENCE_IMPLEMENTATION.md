# File Reference Feature - Implementation Details

## Overview

This document describes the implementation of the file reference feature using the "@" symbol in VT Code TUI.

## Architecture

### Components

#### 1. File Palette Module (`vtcode-core/src/ui/tui/session/file_palette.rs`)

The core module that manages file listing, filtering, and pagination.

**Key Structures:**
- `FileEntry`: Represents a single file with path and display name
- `FilePalette`: Manages the file list state, filtering, and pagination

**Key Functions:**
- `load_files()`: Loads the list of files from the indexer
- `set_filter()`: Applies a filter query to narrow down files
- `move_selection_up/down()`: Navigate through the file list
- `page_up/down()`: Jump between pages (10 items per page)
- `get_selected()`: Returns the currently selected file
- `extract_file_reference()`: Parses "@" references from input text

**Pagination:**
- 10 items per page
- Automatic page calculation based on filtered results
- Page navigation with PgUp/PgDn keys

#### 2. Session Integration (`vtcode-core/src/ui/tui/session.rs`)

Integrates the file palette into the TUI session.

**New Fields:**
- `file_palette: Option<FilePalette>`: The file palette instance
- `file_palette_active: bool`: Whether the palette is currently active

**Key Methods:**
- `load_file_palette()`: Initializes the palette with workspace files
- `check_file_reference_trigger()`: Detects "@" symbol and activates palette
- `close_file_palette()`: Deactivates the palette
- `handle_file_palette_key()`: Processes keyboard input for palette navigation
- `insert_file_reference()`: Inserts selected file path into input
- `render_file_palette()`: Renders the file browser modal

**Input Detection:**
- Triggered when user types "@" character
- Automatically filters as user continues typing
- Deactivates when "@" reference is removed

#### 3. Type Definitions (`vtcode-core/src/ui/tui/types.rs`)

**New Command:**
```rust
InlineCommand::LoadFilePalette { files: Vec<String> }
```
Sent from the runloop to load files into the palette.

**New Event:**
```rust
InlineEvent::FileSelected(String)
```
Emitted when user selects a file (currently not used for special handling).

**New Handle Method:**
```rust
pub fn load_file_palette(&self, files: Vec<String>)
```
Public API to load files into the palette.

#### 4. Runloop Integration (`src/agent/runloop/unified/turn.rs`)

**File Loading:**
- `load_workspace_files()`: Async function that uses `SimpleIndexer` to scan workspace
- Spawned as a background task when session starts
- Sends files to TUI via `handle.load_file_palette()`

**Indexer Integration:**
- Uses `vtcode-indexer` crate
- Respects `.gitignore` patterns
- Filters binary files automatically
- Indexes entire workspace recursively

## User Interaction Flow

1. **Session Start:**
   - Background task spawns to index workspace files
   - Files are loaded into the file palette (hidden initially)

2. **User Types "@":**
   - `check_file_reference_trigger()` detects the "@" symbol
   - File palette activates and displays modal
   - Shows all files with pagination

3. **User Continues Typing (e.g., "@src/main"):**
   - Filter is automatically applied
   - File list updates to show matching files
   - Pagination recalculates

4. **User Navigates:**
   - Arrow keys (↑/↓): Move selection
   - PgUp/PgDn: Jump pages
   - Esc: Close palette without selection

5. **User Selects File (Enter):**
   - Selected file path replaces "@..." in input
   - Space is added after the file reference
   - Cursor moves to end of inserted text
   - Palette closes

6. **User Deletes "@":**
   - Palette automatically deactivates
   - Modal disappears

## UI Design

### Modal Approach (Implemented)

The file browser appears as a centered modal overlay:

```
┌─ File Browser (Page 1/3) ─────────────┐
│ ↑↓ Navigate · PgUp/PgDn Page · ...    │
│ Filter: src/main                       │
├────────────────────────────────────────┤
│ src/main.rs                            │ ← Selected (highlighted)
│ src/main_modular.rs                    │
│ tests/main_test.rs                     │
│                                        │
└────────────────────────────────────────┘
```

**Advantages:**
- Focused interaction without blocking chat history
- Clear visual separation from main interface
- Better for large file lists
- Consistent with existing modal patterns (slash commands, model picker)

## Key Features

### 1. Real-time Filtering
- Filter updates as user types after "@"
- Case-insensitive matching
- Matches anywhere in file path

### 2. Pagination
- 10 items per page
- Page indicator shows current/total pages
- Smooth navigation between pages

### 3. Keyboard Navigation
- **↑/↓**: Move selection up/down
- **PgUp/PgDn**: Jump between pages
- **Enter**: Select file
- **Esc**: Cancel without selection
- **Backspace**: Update filter (or close if "@" deleted)

### 4. Automatic Activation
- Detects "@" symbol in input
- Activates immediately
- Deactivates when "@" removed

### 5. Smart Insertion
- Replaces "@query" with "@filepath"
- Adds space after insertion
- Maintains cursor position

**Path Resolution:**
- **Display**: Shows relative paths (e.g., `@src/main.rs`)
- **User Input**: Inserts relative paths for readability
- **System Processing**: Automatically resolves to absolute paths downstream
- **Tools/Context**: Receive full absolute paths (e.g., `/workspace/src/main.rs`)

## Integration Points

### Indexer (`vtcode-indexer`)
- Provides file list from workspace
- Respects ignore patterns
- Filters binary files
- Efficient recursive scanning

### TUI Session
- Manages palette state
- Handles keyboard input
- Renders modal overlay
- Coordinates with other UI elements

### Runloop
- Spawns background indexing task
- Sends files to TUI
- Handles file selection events

## Testing

### Manual Testing Steps

1. **Basic Activation:**
   ```
   Type: @
   Expected: File browser modal appears
   ```

2. **Filtering:**
   ```
   Type: @main
   Expected: Only files containing "main" are shown
   ```

3. **Navigation:**
   ```
   Type: @
   Press: ↓ ↓ ↓
   Expected: Selection moves down
   ```

4. **Pagination:**
   ```
   Type: @ (in large project)
   Press: PgDn
   Expected: Next page of files shown
   ```

5. **Selection:**
   ```
   Type: @main
   Press: Enter
   Expected: "@main" replaced with "@src/main.rs " (with space)
   ```

6. **Cancellation:**
   ```
   Type: @
   Press: Esc
   Expected: Modal closes, "@" remains in input
   ```

7. **Auto-deactivation:**
   ```
   Type: @main
   Press: Backspace (until @ is deleted)
   Expected: Modal disappears
   ```

### Unit Tests

The `file_palette.rs` module includes unit tests for:
- File reference extraction
- Pagination logic
- Filtering behavior

Run tests with:
```bash
cargo test file_palette
```

## Future Enhancements

### 1. Multi-file Selection
Allow selecting multiple files:
```
@src/main.rs @src/lib.rs @tests/test.rs
```

### 2. Glob Pattern Support
Support wildcard patterns:
```
@src/**/*.rs
@tests/*.rs
```

### 3. Recent Files
Quick access to recently used files:
```
@recent:main.rs
```

### 4. Fuzzy Matching
Smarter matching algorithm:
```
@smrs → src/main.rs
```

### 5. File Preview
Show file preview in modal:
```
┌─ File Browser ─────────────────────────┐
│ src/main.rs                            │
├────────────────────────────────────────┤
│ Preview:                               │
│ fn main() {                            │
│     println!("Hello");                 │
│ }                                      │
└────────────────────────────────────────┘
```

### 6. Directory Navigation
Browse directory structure:
```
@src/
  ├─ main.rs
  ├─ lib.rs
  └─ agent/
      └─ runloop/
```

### 7. File Type Icons
Visual indicators for file types:
```
📄 README.md
🦀 src/main.rs
⚙️  vtcode.toml
```

## Performance Considerations

### Indexing
- Background task doesn't block UI
- Indexer respects ignore patterns
- Efficient for large workspaces

### Filtering
- O(n) linear search through files
- Acceptable for typical workspace sizes
- Could be optimized with trie or fuzzy matching

### Rendering
- Only renders visible page (10 items)
- Modal overlay is lightweight
- No performance impact on main chat

## Troubleshooting

### Files Not Showing
- Check if indexer task completed
- Verify workspace path is correct
- Check `.gitignore` patterns

### Modal Not Appearing
- Ensure "@" is typed in input
- Check if another modal is active
- Verify file palette was loaded

### Selection Not Working
- Ensure files are loaded
- Check keyboard input handling
- Verify Enter key is not captured elsewhere

## Code Style Notes

Following VT Code conventions:
- No emojis in code or UI
- Error handling with `anyhow::Result<T>`
- snake_case for functions/variables
- Descriptive variable names
- Early returns over nested ifs
