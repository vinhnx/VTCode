# Loop Hang Detection

## Overview

Loop hang detection is a safety feature that identifies when the model is stuck in repetitive behavior, specifically when making identical tool calls with the same arguments repeatedly. This prevents the model from wasting API calls and user time on unproductive loops.

## How It Works

The loop detector tracks tool call signatures (combination of tool name and arguments). When the same signature appears more than the configured threshold, detection is triggered.

### Detection Flow

1. **Record Tool Call**: Each time the model makes a tool call, its signature is recorded
2. **Check Threshold**: If the same signature exceeds the threshold count, loop is detected
3. **Show Prompt**: An interactive prompt is displayed asking the user whether to:
   - Keep detection enabled (continue with loop detection)
   - Disable detection for the current session (allow the model to continue)
4. **Reset State**: After the prompt, the detector resets to allow fresh monitoring

## Configuration

Loop hang detection is configured via the `[model]` section in `vtcode.toml`:

```toml
[model]
# Enable/disable loop detection globally
# Default: false (detection is enabled)
skip_loop_detection = false

# Threshold before detection triggers
# Default: 3 (detect after 3rd identical call)
loop_detection_threshold = 3

# Show interactive prompt vs. silent halt
# Default: true (show prompt)
loop_detection_interactive = true
```

### Configuration Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `skip_loop_detection` | bool | `false` | Set to `true` to globally disable loop detection |
| `loop_detection_threshold` | usize | `3` | Number of identical calls before triggering detection |
| `loop_detection_interactive` | bool | `true` | Whether to show user prompt (vs. silent halt) |

## User Experience

When a loop is detected during an active session:

```
A potential loop was detected

This can happen due to repetitive tool calls or other model behavior. Do you want to keep loop
detection enabled or disable it for this session?

 1. Keep loop detection enabled (esc)
  2. Disable loop detection for this session

Note: To disable loop detection checks for all future sessions, set "model.skipLoopDetection"
to true in your settings.json.
```

### Options

- **Option 1 (Keep Enabled)**: Detection remains active, the current tool call is skipped, and the model is allowed to continue without the stuck call
- **Option 2 (Disable for Session)**: Loop detection is disabled for the remainder of the current session, allowing the model to proceed without future detection interruptions

## Implementation Details

### LoopDetector Struct

```rust
pub struct LoopDetector {
    repeated_calls: HashMap<String, usize>,  // signature -> count
    threshold: usize,                         // detection threshold
    enabled: bool,                            // globally enabled?
    interactive: bool,                        // show prompt?
}
```

### Core Methods

- `new(threshold, enabled, interactive)` - Constructor with configuration
- `record_tool_call(signature)` - Record a tool call, returns `true` if loop detected
- `reset()` - Clear all tracking state
- `is_enabled()` - Check if detection is currently enabled
- `is_interactive()` - Check if interactive prompts should be shown
- `enable()` / `disable()` - Runtime control of detection
- `get_count(signature)` - Get repetition count for a specific signature

### Integration Points

- **Initialization**: `src/agent/runloop/unified/turn/session.rs` (line ~939)
  - Read config values from `vt_cfg.model.*` (skip_loop_detection, loop_detection_threshold, loop_detection_interactive)
  - Create `LoopDetector` instance with threshold and settings
  - Track session-level override with `loop_detection_disabled_for_session` flag

- **Usage**: During tool call processing (line ~1278)
  - Build signature from tool name + serialized arguments
  - Call `loop_detector.record_tool_call(signature)`
  - If detection triggered and not disabled for session:
    - Display context info to user
    - Call `show_loop_detection_prompt_tui()` for interactive prompt
    - Handle user response (KeepEnabled or DisableForSession)
    - Reset detector state
    - Skip problematic tool call if KeepEnabled, or continue if DisableForSession

- **Reset**: After each detection trigger
  - `loop_detector.reset()` clears tracking to allow fresh monitoring

### User Response Flow

1. **LoopDetectionResponse::KeepEnabled**
   - Detection remains active
   - Current problematic tool call is skipped
   - Model gets chance to course-correct
   - Detector resets for fresh monitoring

2. **LoopDetectionResponse::DisableForSession**
   - Session-level flag is set
   - All future loop detection checks are bypassed
   - Current tool call is processed normally
   - User message confirms the change

## Testing

Unit tests verify core functionality:

```bash
cargo test --bin vtcode loop_detection
```

Tests cover:
- Threshold detection (4th identical call triggers detection)
- Disabling detection (disabled detector never triggers)
- State reset (clearing state allows fresh monitoring)
- Differentiation between similar signatures (each signature tracked independently)
- Interactive flag checking
- Enable/disable functionality
- Response enum equality

All 7 tests passing:
- `test_loop_detector_threshold`
- `test_loop_detector_disabled`
- `test_loop_detector_reset`
- `test_loop_detector_different_signatures`
- `test_loop_detector_interactive_flag`
- `test_loop_detector_enable_disable`
- `test_loop_detection_response_enum`

## Implementation Status

  **COMPLETED** - Full TUI integration with user interaction

### What's Implemented

1.   Loop detection core module (`loop_detection.rs`)
   - LoopDetector struct with HashMap-based signature tracking
   - Threshold-based detection algorithm
   - Enable/disable control
   - State reset functionality

2.   TUI-based interactive prompt (`show_loop_detection_prompt_tui()`)
   - Uses `dialoguer` crate for selection UI
   - Two options presented to user
   - Proper error handling with fallback behavior

3.   Session integration (`turn/session.rs`)
   - Configuration-driven initialization
   - Real-time loop detection during tool call processing
   - Session-level override support (disable for remainder of session)
   - User message feedback for each response type
   - Proper control flow (skip call on KeepEnabled, continue on DisableForSession)

4.   Comprehensive unit tests
   - Core detection logic verified
   - State management tested
   - Response enum validated

## Future Enhancements

Potential improvements for future versions:

1. **Pattern Analysis**: Detect other types of loops (e.g., same function with different args)
2. **Automatic Recovery**: Suggest alternative strategies when loop detected
3. **Metrics/Analytics**: Track loop detection frequency for diagnostics
4. **Session Persistence**: Remember user's choice across multiple sessions (stored in vtcode.toml)
5. **Timeout-based Detection**: Complement signature-based detection with time-based detection
6. **Auto-recovery**: Automatically modify parameters or retry with different approach

## Related Configuration

- **`tools.max_repeated_tool_calls`**: Different mechanism that limits repeated calls with identical args (halts when exceeded)
- **`tools.max_tool_loops`**: Limits total tool execution per turn (step count limit)

Loop hang detection works alongside these but focuses on the semantic meaning of repetitive tool signatures.
