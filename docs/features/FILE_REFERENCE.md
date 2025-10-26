# File Reference Feature (@-Symbol)

## Overview

The file reference feature allows users to target specific files in their codebase using the "@" symbol in the VT Code TUI. This provides context-aware operations by explicitly specifying which files are relevant to the current query.

## Usage

### Basic Syntax

- `@` - Opens file browser showing all available files with pagination (10 items per page)
- `@filename` - Filters to show matching files (e.g., `@main.rs`)
- `@path/to/file` - Filters by relative path (e.g., `@src/main.rs`)

### Navigation

- **Arrow Keys (↑/↓)**: Navigate through file list
- **Page Up/Down**: Jump between pages (10 items per batch)
- **Enter**: Select file and insert reference into input
- **Esc**: Close file browser without selection
- **Tab**: Auto-complete with first match

### Examples

```
@src/main.rs
```
References the main.rs file in the src directory.

```
@vtcode.toml
```
References the vtcode.toml configuration file.

```
@ 
```
Opens file browser showing all available files.

## UI Design

### Modal Approach (Recommended)

The file browser appears as a centered modal overlay that:
- Provides focused interaction without blocking chat history
- Shows clear visual separation from the main interface
- Displays pagination info (e.g., "Page 2/5")
- Includes search/filter feedback

**Advantages:**
- Less visual clutter
- Focused user attention
- Clear interaction model
- Better for large file lists

## Implementation Details

### Components

1. **File Palette Module** (`vtcode-core/src/ui/tui/file_palette.rs`)
   - Manages file list state
   - Handles filtering and pagination
   - Integrates with vtcode-indexer

2. **Session Integration** (`vtcode-core/src/ui/tui/session.rs`)
   - Detects "@" symbol in input
   - Triggers file palette display
   - Handles file selection events

3. **Indexer Integration** (`vtcode-indexer`)
   - Provides file list from workspace
   - Respects .gitignore patterns
   - Filters binary files

### Data Flow

1. User types "@" in input field
2. Session detects trigger and spawns file palette
3. File palette queries indexer for file list
4. User filters/navigates through files
5. User selects file (Enter key)
6. File path is inserted into input at cursor position
7. File is added to context for next operation

## Benefits

- **Precise Targeting**: Explicitly specify which files are relevant
- **Context-Aware**: Operations understand the scope of work
- **Discoverability**: Browse and search files in large projects
- **Efficiency**: Quick file selection without leaving the chat interface

## Future Enhancements

- Multi-file selection (e.g., `@file1.rs @file2.rs`)
- Glob pattern support (e.g., `@src/**/*.rs`)
- Recent files quick access
- Fuzzy matching for file names
- File preview in modal
