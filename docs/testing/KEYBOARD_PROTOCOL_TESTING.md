# Testing Kitty Keyboard Protocol in TUI

This guide explains how to test the Kitty keyboard protocol implementation in VT Code's interactive TUI.

## Quick Start

### 1. Run Integration Tests
```bash
# Run all keyboard protocol integration tests
cargo test --package vtcode-core --test keyboard_protocol_integration

# Run with output
cargo test --package vtcode-core --test keyboard_protocol_integration -- --nocapture
```

### 2. Manual Testing in TUI

#### Default Mode (Recommended)
```bash
# Start VT Code with default keyboard protocol mode
cargo run -- chat
```

**Test these interactions:**
- **Escape key**: Type `test` then press Escape alone → should NOT submit (only clears if pressed twice)
- **Ctrl+C**: Type any input then press Ctrl+C → should interrupt/cancel
- **Ctrl+D**: Press Ctrl+D → should exit
- **Arrow keys**: Use Up/Down/Left/Right to navigate and edit input
- **Home/End**: Move to start/end of line
- **Alt+B/F**: Move backward/forward by word
- **Ctrl+A/E** or **Cmd+A/E**: Move to start/end of line (macOS)
- **Shift+Enter**: Insert newline in input
- **Alt+Backspace**: Delete word backward

#### Minimal Mode (Compatibility)
```bash
# Create test config
cat > /tmp/vtcode-minimal.toml << 'EOF'
[ui.keyboard_protocol]
enabled = true
mode = "minimal"
EOF

# Run with minimal protocol
cargo run -- chat 2>&1 | VTCODE_CONFIG_PATH=/tmp/vtcode-minimal.toml cargo run -- chat
```

#### Disabled Mode
```bash
cat > /tmp/vtcode-disabled.toml << 'EOF'
[ui.keyboard_protocol]
enabled = false
EOF

# Run with protocol disabled
VTCODE_CONFIG_PATH=/tmp/vtcode-disabled.toml cargo run -- chat
```

## What Gets Tested

### Configuration
- ✓ Protocol modes: `default`, `minimal`, `full`, `custom`
- ✓ Individual flags: DISAMBIGUATE_ESCAPE_CODES, REPORT_EVENT_TYPES, REPORT_ALTERNATE_KEYS
- ✓ Protocol enable/disable toggle

### Key Events
- ✓ Single keys: character input, arrows, function keys
- ✓ Modifiers: Ctrl, Shift, Alt, Meta/Command
- ✓ Key combinations: Ctrl+C, Shift+Enter, Alt+B, etc.
- ✓ Event filtering: only `KeyEventKind::Press` is processed

### Protocol Features
- ✓ Escape key disambiguation (Esc vs escape sequences)
- ✓ Event type reporting (press/release/repeat)
- ✓ Alternate key layout support
- ✓ Graceful fallback when unsupported

## Debug Logging

Enable detailed logging to see protocol negotiation:

```bash
# Full keyboard protocol debug logging
RUST_LOG=debug,vtcode_core::ui::tui=trace cargo run -- chat

# Watch for protocol messages:
# - "enabled keyboard enhancement flags: ..."
# - "keyboard protocol disabled via configuration"
# - "failed to enable keyboard enhancement flags"
```

## Testing with Different Terminals

The Kitty keyboard protocol is supported by:
- ✓ **Kitty** (full support)
- ✓ **WezTerm** (full support)
- ✓ **Alacritty** (0.13+, full support)
- ✓ **foot** (full support)
- ~ **iTerm2** (partial support)
- ~ **Windows Terminal** (partial support)
- ✗ **Terminal.app** (macOS, no support)

## Keyboard Test Matrix

| Key/Combo | Test | Expected Behavior |
|-----------|------|-------------------|
| Single letter (a-z) | Type `a` | Inserted into input |
| Space | Type ` ` | Inserted into input |
| Escape | Press Esc once | Cancels input (toggleable) |
| Escape x2 | Press Esc twice | Clears input if not empty |
| Ctrl+C | Press Ctrl+C | Interrupts current operation |
| Ctrl+D | Press Ctrl+D | Exits TUI |
| Enter | Type + press Enter | Submits input |
| Shift+Enter | Press Shift+Enter | Inserts newline |
| Backspace | Press Backspace | Deletes char before cursor |
| Alt+Backspace | Press Alt+Backspace | Deletes word before cursor |
| Delete | Press Delete | Deletes char after cursor |
| Left Arrow | Press Left | Moves cursor left |
| Alt+Left | Press Alt+Left | Moves cursor left by word |
| Right Arrow | Press Right | Moves cursor right |
| Alt+Right | Press Alt+Right | Moves cursor right by word |
| Home | Press Home | Moves to line start |
| End | Press End | Moves to line end |
| Cmd+A / Ctrl+A | Press Cmd/Ctrl+A | Moves to line start |
| Cmd+E / Ctrl+E | Press Cmd/Ctrl+E | Moves to line end |
| Ctrl+T | Press Ctrl+T | Toggles timeline view |
| Ctrl+Z | Press Ctrl+Z | Suspends (Unix only) |

## Protocol Detection

VT Code automatically detects if the terminal supports the Kitty keyboard protocol:

```rust
use ratatui::crossterm::terminal::supports_keyboard_enhancement;

if supports_keyboard_enhancement().unwrap_or(false) {
    // Protocol supported
}
```

This check happens at TUI startup and respects the `[ui.keyboard_protocol]` config.

## Troubleshooting

### Keyboard input not working
1. Check terminal support: run in Kitty, WezTerm, or Alacritty
2. Try minimal mode: `mode = "minimal"` in config
3. Disable protocol: `enabled = false` in config

### Double-Escape behavior unexpected
- First Escape: toggles cancel state
- Second Escape: clears input (if not empty)
- This is by design—allows quick input clearing

### Modifiers not working
- Verify terminal reports modifiers correctly
- Some terminal emulators have limited modifier support
- Try without protocol: `enabled = false`

## Related Files

- **Implementation**: `vtcode-core/src/ui/tui/modern_tui.rs`
- **Event Processing**: `vtcode-core/src/ui/tui/session/events.rs`
- **Configuration**: `vtcode-config/src/root.rs`
- **Tests**: `vtcode-core/tests/keyboard_protocol_integration.rs`
- **Protocol Spec**: https://sw.kovidgoyal.net/kitty/keyboard-protocol/

## Running Test Suite

```bash
# Full test suite
cargo test

# Keyboard protocol tests only
cargo test keyboard_protocol

# TUI/event tests
cargo test tui
cargo test event

# With logging
RUST_LOG=debug cargo test keyboard_protocol -- --nocapture
```
