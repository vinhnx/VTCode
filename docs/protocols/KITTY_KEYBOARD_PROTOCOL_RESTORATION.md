# Kitty Keyboard Protocol Restoration

## Overview

This document describes the complete restoration of the Kitty keyboard protocol support in VT Code. The protocol allows modern terminals to send richer keyboard event information (press/release/repeat, alternate keys, etc.) to improve the user input experience.

Reference: https://sw.kovidgoyal.net/kitty/keyboard-protocol/

## Architecture

The Kitty keyboard protocol is integrated at multiple layers of the TUI initialization pipeline:

```
VTCodeConfig (vtcode.toml)
    ↓
KeyboardProtocolConfig (ui.keyboard_protocol)
    ↓
keyboard_protocol_to_flags() conversion
    ↓
TuiOptions / ModernTuiConfig
    ↓
Terminal mode setup (enable_terminal_modes / ModernTui.enter())
    ↓
PushKeyboardEnhancementFlags / PopKeyboardEnhancementFlags
```

## Files Modified/Restored

### Core Configuration

#### crates/codegen/vtcode-config/src/root.rs
- **Status**: ✓ Already present
- **Details**: Contains `KeyboardProtocolConfig` struct with fields:
  - `enabled`: Master toggle for keyboard protocol
  - `mode`: Preset modes ("default", "full", "minimal", "custom")
  - Individual flag controls for custom mode
- **Defaults**: Enabled by default, using "default" mode
- **Environment overrides**: `VTCODE_KEYBOARD_PROTOCOL_ENABLED`, `VTCODE_KEYBOARD_PROTOCOL_MODE`

#### crates/codegen/vtcode-core/src/config/mod.rs
- **Status**: ✓ Already present
- **Details**: 
  - Exports `KeyboardProtocolConfig`
  - Implements `keyboard_protocol_to_flags()` function that converts config to `KeyboardEnhancementFlags`
  - Includes comprehensive test suite for all modes

### TUI Layer - Modern Implementation

#### crates/codegen/vtcode-core/src/ui/tui/modern_tui.rs
- **Status**: ✓ Restored
- **Changes**:
  - Added imports for keyboard protocol types
  - Added `keyboard_flags: KeyboardEnhancementFlags` field to `ModernTui` struct
  - Added `keyboard_flags()` builder method
  - Updated `enter()` to push keyboard flags if configured
  - Updated `exit()` to pop keyboard flags if configured
  - Updated `suspend()` to pop keyboard flags if configured

#### crates/codegen/vtcode-core/src/ui/tui/modern_integration.rs
- **Status**: ✓ Restored
- **Changes**:
  - Added `keyboard_protocol: KeyboardProtocolConfig` field to `ModernTuiConfig`
  - Import `keyboard_protocol_to_flags` function
  - Convert config to flags before creating `ModernTui` instance
  - Pass flags through builder chain

### TUI Layer - Runner Implementation

#### crates/codegen/vtcode-core/src/ui/tui/runner.rs
- **Status**: ✓ Restored
- **Changes**:
  - Added `keyboard_protocol: KeyboardProtocolConfig` field to `TuiOptions`
  - Extended `TerminalModeState` with `keyboard_enhancements_pushed` flag
  - Updated `enable_terminal_modes()` to:
    - Accept keyboard flags parameter
    - Push keyboard enhancement flags if enabled
    - Track state for cleanup
  - Updated `restore_terminal_modes()` to:
    - Pop keyboard enhancement flags if they were pushed
    - Execute in correct order (keyboard flags first)
  - Updated `run_tui()` to convert config and pass to mode setup

### Public API Layer

#### crates/codegen/vtcode-core/src/ui/tui.rs
- **Status**: ✓ Updated
- **Changes**:
  - Added `keyboard_protocol: KeyboardProtocolConfig` parameter to `spawn_session_with_prompts()`
  - Updated `spawn_session()` to pass default config
  - Pass keyboard protocol through to `TuiOptions`

### Integration Point - Session Setup

#### src/agent/runloop/unified/session_setup.rs
- **Status**: ✓ Updated
- **Changes**:
  - Updated call to `spawn_session_with_prompts()` to pass actual keyboard protocol config
  - Extract from `vt_cfg.ui.keyboard_protocol` when available
  - Fall back to default when config not available

### Documentation & Comments

#### crates/codegen/vtcode-core/src/ui/tui/alternate_screen.rs
- **Status**: ✓ Comments already present
- **Details**: 
  - Documentation correctly mentions keyboard enhancement flags in lifecycle comments
  - Actual protocol implementation happens at higher TUI layer (modern_tui.rs, runner.rs)

#### crates/codegen/vtcode-core/src/ui/tui/panic_hook.rs
- **Status**: ✓ Already present
- **Details**:
  - Properly imports and uses `PopKeyboardEnhancementFlags` in terminal restoration

## Configuration

### Default Behavior

By default, the keyboard protocol is:
- **Enabled**: true
- **Mode**: "default" (includes DISAMBIGUATE_ESCAPE_CODES, REPORT_EVENT_TYPES, REPORT_ALTERNATE_KEYS)

### Configuration Modes

```toml
[ui.keyboard_protocol]
enabled = true
mode = "default"  # Options: "default", "full", "minimal", "custom"
```

#### Mode Details

- **default**: 
  - DISAMBIGUATE_ESCAPE_CODES (resolve Esc key ambiguity)
  - REPORT_EVENT_TYPES (press/release/repeat events)
  - REPORT_ALTERNATE_KEYS (alternate key layouts)

- **full**: 
  - All from "default" plus
  - REPORT_ALL_KEYS_AS_ESCAPE_CODES (modifier-only keys)

- **minimal**:
  - DISAMBIGUATE_ESCAPE_CODES only

- **custom**:
  - Individually controlled flags:
    - `disambiguate_escape_codes`
    - `report_event_types`
    - `report_alternate_keys`
    - `report_all_keys`

### Environment Variable Overrides

```bash
export VTCODE_KEYBOARD_PROTOCOL_ENABLED=true
export VTCODE_KEYBOARD_PROTOCOL_MODE=full
```

## Data Flow

1. **Config Load**: `VTCodeConfig` loads from `vtcode.toml` and environment
2. **Session Setup**: `session_setup.rs` reads `vt_cfg.ui.keyboard_protocol`
3. **Session Spawn**: Passes config to `spawn_session_with_prompts()`
4. **TUI Setup**: `run_tui()` or `run_modern_tui()` receives config
5. **Flag Conversion**: `keyboard_protocol_to_flags()` converts to crossterm flags
6. **Terminal Init**: `enable_terminal_modes()` pushes flags to terminal
7. **Terminal Cleanup**: `restore_terminal_modes()` pops flags on exit

## Terminal Support

The Kitty keyboard protocol is supported by:
- Kitty terminal emulator
- WezTerm
- Alacritty (with enabling)
- iTerm2
- Other modern terminals supporting CSI sequences

Terminals that don't support the protocol will safely ignore the ANSI escape sequences, so enabling is safe across environments.

## Testing

The restoration includes:
- Unit tests in `crates/codegen/vtcode-core/src/config/mod.rs` for `keyboard_protocol_to_flags()`
- Tests for all mode conversions (default, full, minimal, custom)
- Tests for disabled protocol
- Tests for invalid mode handling

## Compilation Status

✓ Code compiles without errors
✓ All dependencies resolve correctly
✓ Backward compatible (defaults provided everywhere)

## Future Enhancements

Potential improvements:
1. Runtime configuration changes (toggle keyboard protocol while running)
2. Terminal detection (auto-enable for known-good terminals)
3. Performance profiling with different keyboard protocol settings
4. User documentation on keyboard protocol benefits
