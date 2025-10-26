# File Browser Slash Command

## Overview

The `/files` slash command provides an alternative way to access the file browser feature, complementing the `@` symbol trigger.

## Usage

### Basic Command
```
/files
```
Opens the file browser modal showing all workspace files.

### With Filter
```
/files main
```
Opens the file browser with "main" pre-filtered, showing only matching files.

### Examples

#### Browse All Files
```
/files
```
Result: File browser opens with all files listed

#### Find Configuration Files
```
/files config
```
Result: File browser opens showing files containing "config"

#### Find Rust Files
```
/files .rs
```
Result: File browser opens showing Rust source files

#### Find Test Files
```
/files test
```
Result: File browser opens showing test-related files

## Comparison: /files vs @ Symbol

### /files Command
- **Trigger**: Type `/files` in chat
- **Use Case**: Explicit file browsing action
- **Visibility**: Shows up in slash command help
- **Discoverability**: Listed with other commands
- **Filter**: Optional argument after command

### @ Symbol
- **Trigger**: Type `@` anywhere in input
- **Use Case**: Inline file reference while typing
- **Visibility**: Contextual, appears when needed
- **Discoverability**: Natural typing flow
- **Filter**: Type after `@` symbol

## When to Use Each

### Use `/files` when:
- You want to browse files explicitly
- You're exploring the workspace
- You prefer command-based interaction
- You want to see the command in help

### Use `@` when:
- You're typing a message and need a file reference
- You want inline, contextual file selection
- You prefer minimal interruption to typing flow
- You're already familiar with the feature

## Integration with Slash Command System

### Registration
The `/files` command is registered in the slash command registry:
```rust
SlashCommandInfo {
    name: "files",
    description: "Browse and select files from workspace (usage: /files [filter])",
}
```

### Handler
Located in `src/agent/runloop/slash_commands.rs`:
```rust
"files" => {
    let initial_filter = if args.trim().is_empty() {
        None
    } else {
        Some(args.trim().to_string())
    };
    
    if renderer.supports_inline_ui() {
        return Ok(SlashCommandOutcome::StartFileBrowser { initial_filter });
    }
    
    // Fallback for non-UI mode
    renderer.line(
        MessageStyle::Error,
        "File browser requires inline UI mode. Use @ symbol instead.",
    )?;
    Ok(SlashCommandOutcome::Handled)
}
```

### Outcome Processing
Located in `src/agent/runloop/unified/turn.rs`:
```rust
SlashCommandOutcome::StartFileBrowser { initial_filter } => {
    // Check for conflicts with other modals
    if model_picker_state.is_some() || palette_state.is_some() {
        // Show error
        continue;
    }
    
    // Activate file palette
    if let Some(filter) = initial_filter {
        handle.set_input(format!("@{}", filter));
    } else {
        handle.set_input("@".to_string());
    }
    
    // Show confirmation
    renderer.line(
        MessageStyle::Info,
        "File browser activated. Use arrow keys to navigate, Enter to select, Esc to close.",
    )?;
    continue;
}
```

## Technical Details

### Command Flow
```
User types: /files main
    ↓
Slash command parser
    ↓
SlashCommandOutcome::StartFileBrowser { initial_filter: Some("main") }
    ↓
Turn handler
    ↓
handle.set_input("@main")
    ↓
File palette activates with filter
    ↓
Modal displays filtered results
```

### Performance
- **No Additional Indexing**: Uses existing file cache
- **Instant Activation**: No delay, just sets input
- **Same Performance**: Identical to @ symbol trigger

### Error Handling
- **Modal Conflict**: Checks for active modals before opening
- **Non-UI Mode**: Provides helpful error message
- **Empty Workspace**: Gracefully handles no files

## Benefits

### Discoverability
- Listed in `/help` command
- Appears in slash command autocomplete
- Documented with other commands

### Consistency
- Follows slash command conventions
- Integrates with existing modal system
- Uses same keyboard shortcuts

### Flexibility
- Can be used alongside @ symbol
- Supports pre-filtering
- Works in all contexts

## Examples in Context

### Scenario 1: Exploring Workspace
```
User: /files
System: File browser activated...
[Modal shows all files]
User: [navigates and selects src/main.rs]
Input: @src/main.rs 
```

### Scenario 2: Finding Specific Files
```
User: /files config
System: File browser activated...
[Modal shows: vtcode.toml, src/config.rs, etc.]
User: [selects vtcode.toml]
Input: @vtcode.toml 
```

### Scenario 3: Quick Reference
```
User: Check @
[Types @ directly, modal appears]
User: [selects file]
```

## Future Enhancements

### Potential Additions
1. **Multiple Filters**: `/files rust test` (AND logic)
2. **Exclude Patterns**: `/files main -test` (exclude tests)
3. **File Type Shortcuts**: `/files --rust` (only .rs files)
4. **Recent Files**: `/files --recent` (recently accessed)
5. **Modified Files**: `/files --modified` (git status)

### Advanced Features
```
/files --type rust --exclude test --recent 10
```
Opens browser showing 10 most recent Rust files, excluding tests.

## Documentation

### Help Text
```
/help
...
/files - Browse and select files from workspace (usage: /files [filter])
...
```

### Quick Reference
```
/files          # Browse all files
/files main     # Filter by "main"
/files src/     # Filter by path
/files .rs      # Filter by extension
```

## Conclusion

The `/files` slash command provides a discoverable, command-based way to access the file browser, complementing the inline `@` symbol trigger. Both methods use the same underlying implementation and provide identical functionality, giving users flexibility in how they interact with the file reference system.
