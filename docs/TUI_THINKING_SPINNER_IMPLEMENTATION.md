# TUI Thinking Spinner Implementation

## Overview

Added a visual "thinking" indicator with an animated braille spinner that appears immediately after the user submits a message. The spinner automatically stops when the AI agent starts responding.

## Changes Made

### 1. Added `ThinkingSpinner` State Struct
**File**: `vtcode-core/src/ui/tui/session.rs`

Created a new spinner state struct that manages:
- `is_active`: Whether the spinner is currently animating
- `started_at`: Timestamp when spinner was started
- `spinner_index`: Current animation frame (0-3)
- `last_update`: Last frame update time

Features:
- 80ms frame duration for smooth animation
- Braille pattern animation:  →  →  → 
- Methods: `start()`, `stop()`, `update()`, `get_text()`, `render_frame()`

### 2. Integrated Spinner into Session

Added `thinking_spinner: ThinkingSpinner` field to the `Session` struct and initialized it in `Session::new()`.

### 3. Spinner Activation on Submit

**Location**: `process_key()` function, Enter key handler (line 2315)

When user submits a message:
```rust
self.thinking_spinner.start();
```

### 4. Spinner Deactivation on Agent Response

**Location**: `handle_command()` function (lines 327-336)

When agent messages arrive via `AppendLine` or `Inline` commands:
```rust
if kind == InlineMessageKind::Agent {
    self.thinking_spinner.stop();
}
```

### 5. Rendering Integration

**Location**: `render()` function start (lines 551-560)

Updates spinner animation each frame:
```rust
self.thinking_spinner.update();
if self.thinking_spinner.is_active {
    self.needs_redraw = true;  // Force continuous redraw during thinking
}
```

**Location**: `render_transcript()` function (lines 737-751)

Displays spinner in transcript after message lines:
- Added as a cyan, dimmed line
- Only shown if space available in viewport
- Positioned right after last message line

### 6. Dependencies

**File**: `vtcode-core/Cargo.toml`

Added: `indicatif = { version = "0.18", default-features = false }`

(Indicatif was already in root Cargo.toml but needed to be added to vtcode-core specifically)

## Behavior

1. **Trigger**: Spinner starts immediately when user presses Enter to submit
2. **Animation**: Smooth 80ms frame updates using braille characters
3. **Display**: Shows " Thinking..." with cyan color at dim intensity
4. **Stop**: Stops automatically when first agent message line appears
5. **Performance**: Non-blocking, forces minimal redraws only when needed

## Visual Example

```

 Your message                        
                                      
  Thinking...                         ← Animated spinner
                                      
 Agent response starts here...          ← Spinner stops

```

## Testing

- Compiles cleanly: `cargo check` 
- No warnings or errors
- Builds successfully: `cargo build` 
- Integration with existing TUI flow verified

## Implementation Notes

- Used standard library `time::Instant` for frame timing (no external deps needed for timer)
- Spinner state is part of Session, persists across renders
- No concurrent tasks or threads required
- Integrates seamlessly with existing async/await flow
- Minimal performance impact: only updates when active, only renders one line

## Future Enhancements

- Configurable spinner style/characters
- Persistent display even during scroll
- Multi-spinner support for parallel tool execution
- Spinner themes matching user's color scheme
- Duration statistics (show elapsed time with spinner)
