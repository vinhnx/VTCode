# File Reference Quick Start

## What is it?

The file reference feature lets you quickly select files from your workspace using the "@" symbol.

## How to Use

### 1. Open File Browser
Type `@` in the chat input:
```
@
```
A modal window appears showing all files in your workspace.

### 2. Filter Files
Continue typing to filter:
```
@main
```
Only files containing "main" are shown.

### 3. Navigate
Use keyboard to navigate:
- **↑/↓**: Move selection up/down
- **PgUp/PgDn**: Jump between pages
- **Enter**: Select file
- **Esc**: Cancel

### 4. Select File
Press Enter to insert the file path:
```
Before: @main
After:  @src/main.rs 
```

## Examples

### Reference a specific file
```
@src/main.rs
```

### Find files by name
```
@config
```
Shows: `vtcode.toml`, `config.rs`, etc.

### Browse all files
```
@
```
Shows all files with pagination.

## Tips

- The file browser appears automatically when you type "@"
- It disappears when you delete the "@" symbol
- Files are loaded once when you start a session
- The browser respects your `.gitignore` patterns

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `@` | Open file browser |
| `↑` | Move selection up |
| `↓` | Move selection down |
| `PgUp` | Previous page |
| `PgDn` | Next page |
| `Enter` | Select file |
| `Esc` | Close browser |
| `Backspace` | Update filter / Close if @ deleted |

## Common Use Cases

### 1. Ask about a specific file
```
What does @src/main.rs do?
```

### 2. Request changes to a file
```
Add error handling to @src/lib.rs
```

### 3. Compare files
```
What's the difference between @src/main.rs and @src/main_modular.rs?
```

### 4. Reference configuration
```
Check the settings in @vtcode.toml
```

## Troubleshooting

**File browser doesn't appear?**
- Make sure you typed "@" in the input field
- Check if another modal is open (close it first)

**Can't find a file?**
- Try typing more of the filename: `@src/main`
- Check if the file is in your workspace
- Verify it's not ignored by `.gitignore`

**Selection not working?**
- Make sure files are loaded (wait a moment after session start)
- Try pressing Enter on the highlighted file

## What's Next?

Once you've selected a file, you can:
- Ask questions about it
- Request modifications
- Compare it with other files
- Use it as context for your query

The file reference helps VT Code understand exactly which files you're talking about, making responses more accurate and relevant.
