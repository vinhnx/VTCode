# Keyboard Protocol Configuration

VT Code supports the [kitty keyboard protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/) for enhanced keyboard event handling in terminal applications.

## Quick Start

Configure keyboard protocol in `vtcode.toml`:

```toml
[ui.keyboard_protocol]
enabled = true
mode = "default"  # or "full", "minimal", "custom"
```

### Viewing Current Configuration

To view your effective keyboard protocol configuration:

```bash
# Display full configuration (includes keyboard_protocol section)
vtcode config

# Or use the slash command in an interactive session
/config
```

The configuration output will show your current `[ui.keyboard_protocol]` settings along with all other vtcode configuration.

**Example output**:
```toml
[ui.keyboard_protocol]
enabled = true
mode = "default"
disambiguate_escape_codes = true
report_event_types = true
report_alternate_keys = true
report_all_keys = false
```

## Preset Modes

### default (recommended)

The default mode provides the best balance of features and compatibility:
- Disambiguates escape sequences (distinguishes Esc key from escape codes)
- Reports key press/release/repeat events
- Reports alternate key layouts for different keyboard configurations

### full

Enables all available features. Currently equivalent to "default" mode in crossterm 0.28.1:
- All "default" mode features
- Note: `REPORT_ALL_KEYS` flag is not yet supported in crossterm 0.28.1

### minimal

Minimal enhancements for maximum compatibility:
- Only disambiguates escape sequences
- Best for older terminals or when compatibility is critical

### custom

Fine-grained control via individual flags:
```toml
[ui.keyboard_protocol]
mode = "custom"
disambiguate_escape_codes = true
report_event_types = false
report_alternate_keys = true
report_all_keys = false
```

## Environment Variables

Override configuration at runtime:

```bash
# Disable protocol entirely
export VTCODE_KEYBOARD_PROTOCOL_ENABLED=false

# Set preset mode
export VTCODE_KEYBOARD_PROTOCOL_MODE=minimal

# Override individual flags (when mode=custom)
# Not currently implemented, but planned for future versions
```

## Configuration Reference

| Setting | Type | Default | Description |
|---------|------|---------|-------------|
| `enabled` | boolean | `true` | Master toggle for keyboard protocol |
| `mode` | string | `"default"` | Preset mode: default/full/minimal/custom |
| `disambiguate_escape_codes` | boolean | `true` | Resolve Esc key ambiguity |
| `report_event_types` | boolean | `true` | Report press/release/repeat events |
| `report_alternate_keys` | boolean | `true` | Report alternate keyboard layouts |
| `report_all_keys` | boolean | `false` | Report modifier-only keys (not supported yet) |

## Terminal Compatibility

The kitty keyboard protocol is supported by many modern terminals:

| Terminal | Support Level |
|----------|---------------|
| Kitty | ✓ Full support |
| WezTerm | ✓ Full support |
| Alacritty (0.13+) | ✓ Full support |
| foot | ✓ Full support |
| iTerm2 | ~ Partial support |
| Terminal.app (macOS) | ✗ No support |
| Windows Terminal | ~ Partial support |

VT Code automatically detects terminal capabilities and gracefully falls back to legacy keyboard handling when the protocol is not supported.

## Troubleshooting

### Keyboard Input Issues

If you experience keyboard input problems:

1. Try minimal mode:
   ```toml
   [ui.keyboard_protocol]
   mode = "minimal"
   ```

2. Disable the protocol:
   ```toml
   [ui.keyboard_protocol]
   enabled = false
   ```

3. Check terminal compatibility (see table above)

### Debug Logging

Enable debug logging to see keyboard enhancement status:

```bash
export RUST_LOG=debug
vtcode
```

Look for log messages like:
- `enabled keyboard enhancement flags: ...` - Protocol activated
- `keyboard protocol disabled via configuration` - Protocol disabled
- `failed to enable keyboard enhancement flags` - Terminal doesn't support protocol

## Implementation Details

### Protocol Detection

VT Code queries terminal capabilities at startup using `supports_keyboard_enhancement()`. The protocol is only activated if:
1. The terminal reports support for keyboard enhancements
2. The configuration has `enabled = true`
3. At least one flag would be enabled based on the mode

### Backward Compatibility

When the keyboard protocol is disabled or unsupported:
- VT Code uses standard terminal keyboard input handling
- All keyboard shortcuts continue to work as expected
- No user-visible changes in functionality

### Future Enhancements

Planned improvements for future versions:
- Support for `REPORT_ALL_KEYS` flag when crossterm is upgraded
- Per-application keyboard protocol profiles
- Runtime keyboard protocol toggling
- Additional environment variable overrides

## Related Documentation

- [Configuration Precedence](CONFIGURATION_PRECEDENCE.md)
- [Kitty Keyboard Protocol Specification](https://sw.kovidgoyal.net/kitty/keyboard-protocol/)
- [VT Code UI Configuration](UI_CONFIGURATION.md)
