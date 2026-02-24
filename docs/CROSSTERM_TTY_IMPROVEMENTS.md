# Crossterm TTY Integration Improvements

This document summarizes the improvements made to VT Code's terminal handling based on [crossterm's TTY module best practices](https://docs.rs/crossterm/latest/crossterm/tty/index.html).

## Summary

VT Code now has enhanced TTY detection, terminal mode tracking, and error handling following crossterm's recommended patterns for safe and convenient terminal interaction.

**Key Decision**: Replaced `std::io::IsTerminal` (Rust std lib) with **crossterm's `IsTty`** trait for consistency across the codebase, since crossterm is already a core dependency for terminal operations.

## Changes Made

### 1. New TTY Utility Module (`vtcode-core/src/utils/tty.rs`)

Created a comprehensive TTY utility module that provides:

- **`TtyExt` trait**: Extension trait for consistent TTY detection across the codebase
  - `is_tty_ext()`: Check if a stream is connected to a terminal
  - `supports_color()`: Check if ANSI colors are supported
  - `is_interactive()`: Check if full interactive features are available

- **`TtyCapabilities` struct**: Detects and caches terminal capabilities
  - Color support
  - Cursor manipulation
  - Bracketed paste
  - Focus events
  - Mouse input
  - Keyboard enhancement flags

- **Helper functions**:
  - `is_interactive_session()`: Check if running in interactive TTY context
  - `terminal_size()`: Get current terminal dimensions

### 2. Improved Terminal Mode State Tracking (`vtcode-core/src/ui/tui/runner.rs`)

Enhanced `TerminalModeState` to track all terminal modes:
- Bracketed paste enabled/disabled
- Raw mode enabled/disabled
- Mouse capture enabled/disabled
- Focus change events enabled/disabled
- Keyboard enhancement flags pushed/popped

**Benefits:**
- Proper state preservation and restoration
- Graceful degradation when features aren't supported
- Better error handling with detailed diagnostics

### 3. Enhanced Error Handling (`vtcode-core/src/ui/tui/alternate_screen.rs`)

Improved error handling in alternate screen management:
- TTY detection before applying terminal features
- Conditional feature enabling based on TTY status
- Better error messages with structured logging
- Graceful degradation instead of hard failures

### 4. Consistent TTY Detection Across Codebase

Updated all TTY detection to use crossterm's `IsTty` trait via `TtyExt`:
- `src/main_helpers.rs`: stdin TTY detection for piped input
- `vtcode-core/src/ui/tui/runner.rs`: stderr TTY detection for TUI
- `vtcode-core/src/ui/tui/alternate_screen.rs`: stdout TTY detection

## Key Improvements

### Before
```rust
// Inconsistent TTY detection using std::io::IsTerminal
if std::io::stdin().is_terminal() {
    // ...
}

// Limited mode tracking
struct TerminalModeState {
    focus_change_enabled: bool,
    keyboard_enhancements_pushed: bool,
}
```

### After
```rust
// Consistent TTY detection using crossterm's IsTty via TtyExt
use vtcode_core::utils::tty::TtyExt;

if io::stdin().is_tty_ext() {
    // ...
}

// Comprehensive mode tracking
struct TerminalModeState {
    bracketed_paste_enabled: bool,
    raw_mode_enabled: bool,
    mouse_capture_enabled: bool,
    focus_change_enabled: bool,
    keyboard_enhancements_pushed: bool,
}
```

## Best Practices Applied

Following [crossterm's TTY module documentation](https://docs.rs/crossterm/latest/crossterm/tty/index.html):

1. **Safe TTY Detection**: Using `IsTty` trait instead of manual file descriptor checks
2. **Conditional Behavior**: Checking `is_tty()` before applying terminal-specific features
3. **Cross-Platform**: Abstracting away platform differences for TTY detection
4. **State Preservation**: Tracking all terminal modes for proper restoration
5. **Graceful Degradation**: Continuing operation with limited features when unavailable

## Testing

All changes compile successfully:
```bash
cargo check
# Finished `dev` profile [unoptimized] target(s) in 24.86s
```

## Files Modified

### Core TTY Infrastructure
1. `vtcode-core/src/utils/tty.rs` - New TTY utility module with `TtyExt` trait
2. `vtcode-core/src/utils/mod.rs` - Export tty module

### TUI Terminal Handling
3. `vtcode-core/src/ui/tui/runner.rs` - Improved mode tracking and TTY detection
4. `vtcode-core/src/ui/tui/alternate_screen.rs` - Enhanced error handling
5. `vtcode-core/src/ui/terminal.rs` - Consistent TTY detection
6. `vtcode-core/src/ui/tui.rs` - Consistent TTY detection
7. `vtcode-core/src/commands/ask.rs` - Consistent TTY detection

### CLI Commands
8. `src/main_helpers.rs` - Consistent TTY detection
9. `src/cli/benchmark.rs` - Consistent TTY detection
10. `src/cli/ask.rs` - Consistent TTY detection
11. `src/cli/exec.rs` - Consistent TTY detection
12. `src/interactive_list.rs` - Consistent TTY detection

### Shared Components
13. `vtcode-commons/src/ansi_codes.rs` - Consistent TTY detection
14. `src/agent/runloop/unified/tool_output_handler.rs` - Removed unused import

## Future Enhancements

Potential future improvements:
- Add runtime capability detection for advanced terminal features
- Implement terminal query sequences for precise capability detection
- Add support for kitty keyboard protocol detection
- Cache terminal capabilities for performance

## References

- [Crossterm TTY Module Documentation](https://docs.rs/crossterm/latest/crossterm/tty/index.html)
- [Crossterm GitHub Repository](https://github.com/crossterm-rs/crossterm)
- [Ratatui Documentation](https://docs.rs/ratatui/latest/ratatui/)
