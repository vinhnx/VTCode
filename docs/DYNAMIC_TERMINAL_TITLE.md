# Dynamic Terminal Title Implementation

## Overview

VT Code now features **dynamic terminal titles** that update in real-time to reflect the current agent activity and project context. This provides better visibility into what the agent is doing, especially when managing multiple terminal windows.

## Terminal Compatibility

The implementation uses the **standard OSC 2 escape sequence** (`\x1b]2;title\x07`) which is universally supported across all major terminals:

| Terminal | Platform | Support |
|----------|----------|---------|
| **iTerm2** | macOS | ✅ Full |
| **Kitty** | Cross-platform | ✅ Full |
| **Alacritty** | Cross-platform | ✅ Full |
| **Ghostty** | Cross-platform | ✅ Full |
| **Warp** | macOS | ✅ Full |
| **Terminal.app** | macOS | ✅ Full |
| **WezTerm** | Cross-platform | ✅ Full |
| **Windows Terminal** | Windows 10+ | ✅ Full |
| **XTerm** | UNIX | ✅ Full |

### Technical Reference

- **OSC 2**: Standard XTerm escape sequence for setting window title
- **Format**: `ESC ] 2 ; title BEL` (where ESC = `\x1b`, BEL = `\x07`)
- **Compatibility**: Part of XTerm control sequences, supported by all modern terminals
- **Alternative terminators**: Some terminals also support ST (`\x1b\\`) but BEL is more universally compatible

## Title Formats

The terminal title dynamically updates based on the agent's current state:

### Idle State
```
> VT Code (project-name)
```

### Active States

| Activity | Title Format | Example |
|----------|-------------|---------|
| **Running command** | `> VT Code (project) \| Running cmd` | `> VT Code (vtcode) \| Running cargo` |
| **Running tool** | `> VT Code (project) \| Running tool` | `> VT Code (vtcode) \| Running read_file` |
| **Executing** | `> VT Code (project) \| Executing` | `> VT Code (vtcode) \| Executing` |
| **Editing file** | `> VT Code (project) \| Editing filename` | `> VT Code (vtcode) \| Editing main.rs` |
| **Debugging** | `> VT Code (project) \| Debugging` | `> VT Code (vtcode) \| Debugging` |
| **Building** | `> VT Code (project) \| Building` | `> VT Code (vtcode) \| Building` |
| **Testing** | `> VT Code (project) \| Testing` | `> VT Code (vtcode) \| Testing` |
| **Searching** | `> VT Code (project) \| Searching` | `> VT Code (vtcode) \| Searching` |
| **Creating** | `> VT Code (project) \| Creating` | `> VT Code (vtcode) \| Creating` |
| **Checking** | `> VT Code (project) \| Checking` | `> VT Code (vtcode) \| Checking` |
| **Loading** | `> VT Code (project) \| Loading` | `> VT Code (vtcode) \| Loading` |
| **Action Required** | `> VT Code (project) \| Action Required` | `> VT Code (vtcode) \| Action Required` |
| **Thinking** | `> VT Code (project) \| Thinking` | `> VT Code (vtcode) \| Thinking` |

### Detected Actions

The system intelligently detects actions from the status bar text:

- **Running**: Commands, tools, processes
- **Editing**: File modifications, reads, writes
- **Debugging**: Troubleshooting, tracing
- **Building**: Compilation, make
- **Testing**: Test execution, validation
- **Searching**: Find, grep, locate
- **Creating**: Generate, write, add
- **Action Required**: HITL (Human-in-the-loop) states
- **Thinking**: Processing, analyzing

## Implementation Details

### Files Modified

1. **`vtcode-core/src/ui/tui/session.rs`**
   - Added `workspace_root` field
   - Added `last_terminal_title` field

2. **`vtcode-core/src/ui/tui/session/terminal_title.rs`** (NEW)
   - Core terminal title management logic
   - OSC 2 sequence emission
   - Activity detection and title generation

3. **`vtcode-core/src/ui/tui/runner.rs`**
   - Pass workspace root to session
   - Set initial title on startup
   - Clear title on exit

4. **`vtcode-core/src/ui/tui.rs`**
   - Updated `spawn_session()` signature
   - Updated `spawn_session_with_prompts()` signature

5. **`src/agent/runloop/unified/session_setup/ui.rs`**
   - Pass workspace root when spawning session

### Key Functions

```rust
// Set workspace root for title generation
session.set_workspace_root(workspace_root);

// Update title based on current activity (called in main loop)
session.update_terminal_title();

// Clear title on exit
session.clear_terminal_title();
```

### Update Strategy

- **Debounced updates**: Title only changes when state actually changes
- **Efficient**: Uses `last_terminal_title` cache to avoid redundant writes
- **Real-time**: Updated in main event loop after command processing
- **Non-blocking**: Direct stderr writes, no async operations

### Spinner Handling

The implementation properly strips spinner characters (⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏-\\|/.) from status text before extracting action verbs, ensuring clean title display.

### Title Sanitization

Titles are sanitized to remove problematic characters:
- Control characters are removed
- Backslashes are replaced with forward slashes
- Special characters are replaced with spaces
- Multiple spaces are collapsed

### Title Truncation

Titles longer than 128 characters are truncated with ellipsis to prevent display issues in terminals with limited title bar space.

## Usage

The terminal title updates automatically - no configuration needed. The title will:

1. **On startup**: Show `> VT Code (project-name)`
2. **During activity**: Update to reflect current action
3. **On completion**: Revert to project name or idle state
4. **On exit**: Clear to default terminal title

## Benefits

1. **Better context**: See at a glance what each terminal is doing
2. **Window management**: Easier to distinguish multiple VT Code instances
3. **Activity monitoring**: Know when agent is busy vs idle
4. **Professional**: Clean, informative titles that match the TUI status bar

## Troubleshooting

### Title not updating?

1. **Check terminal settings**: Some terminals have "Allow programs to set window title" option
2. **Verify OSC 2 support**: Try manual test: `echo -ne "\x1b]2;Test Title\x07"`
3. **Check permissions**: Terminal must allow escape sequences

### Title stuck?

The title should clear on exit. If stuck:
- Restart terminal, or
- Run: `echo -ne "\x1b]2;\x07"` to clear

### Custom title format?

Currently not configurable, but the implementation is modular. Future versions may support custom formats via config.

## References

- [XTerm Control Sequences](https://invisible-island.net/xterm/ctlseqs/ctlseqs.html)
- [iTerm2 Escape Codes](https://iterm2.com/documentation-escape-codes.html)
- [Crossterm SetTitle](https://docs.rs/crossterm/latest/crossterm/terminal/struct.SetTitle.html)
- [OSC Sequences](https://en.wikipedia.org/wiki/ANSI_escape_code#OSC_(Operating_System_Command)_sequences)

## Future Enhancements

Potential improvements:

- [ ] Custom title format configuration
- [ ] OSC 7 working directory tracking
- [ ] OSC 9 progress reporting for long operations
- [ ] Tab title vs window title differentiation
- [ ] Session name integration (e.g., `/rename` command)
