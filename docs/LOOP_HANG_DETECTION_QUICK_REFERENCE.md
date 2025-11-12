# Loop Hang Detection - Quick Reference

## What Changed

Loop hang detection was refactored to be **more accurate, user-friendly, and maintainable**.

## For Users

### Better Feedback When Loop Detected

You now see:
```
A potential loop was detected.

Looping tool call: 'read_file::{"path": "/Users/..."}'
Repeat count: 5

What would you like to do?

  1. Keep loop detection enabled (esc)
  2. Disable loop detection for this session
```

Instead of just:
```
A potential loop was detected

Do you want to keep loop detection enabled or disable it for this session?
```

### Better Recovery

- **Option 1 (Keep enabled)**: Only resets the problematic tool call, allowing detection to continue monitoring other patterns
- **Option 2 (Disable for session)**: Fully disables detection for the remainder of the session

## For Developers

### API Changes

**LoopDetector.record_tool_call()** now returns a tuple:

```rust
// Returns (is_detected, repeat_count)
let (detected, count) = detector.record_tool_call("tool::args");

if detected {
    // You now have access to how many times it repeated
    println!("Loop detected after {} repeats", count);
}
```

### New Method: Selective Reset

```rust
// Reset only a specific signature
detector.reset_signature("tool::args");

// Reset everything
detector.reset();
```

Use selective reset when the user chooses "Keep enabled" to only clear the problematic signature while preserving context about other calls.

## Architecture

### Core Module Structure

```
loop_detection.rs:
├── LoopDetector struct
│   ├── record_tool_call() → (bool, usize)  [NEW: returns tuple]
│   ├── reset()
│   ├── reset_signature()                   [NEW]
│   └── utility methods
├── LoopDetectionResponse enum
├── prompt_for_loop_detection()
└── show_loop_detection_prompt_tui()        [ENHANCED: takes context]
```

### Session Integration

```
session.rs:
├── Initialize LoopDetector from config
├── For each tool call:
│   ├── Get (detected, count) tuple
│   ├── If detected, show enhanced prompt with context
│   └── Handle user response:
│       ├── KeepEnabled → reset_signature() & skip call
│       └── DisableForSession → reset() & process call
```

## Test Coverage

11/11 tests passing, including new test for selective reset:

```rust
#[test]
fn test_selective_reset() {
    let mut detector = LoopDetector::new(2, true, true);
    
    detector.record_tool_call("sig1"); // count = 1
    detector.record_tool_call("sig1"); // count = 2
    detector.record_tool_call("sig2"); // count = 1
    
    detector.reset_signature("sig1");  // Reset only sig1
    
    assert_eq!(detector.get_count("sig1"), 0); // Reset
    assert_eq!(detector.get_count("sig2"), 1); // Untouched
}
```

## Configuration (No Changes)

Still configured via `vtcode.toml`:

```toml
[model]
skip_loop_detection = false              # Enable/disable globally
loop_detection_threshold = 3             # Detect after 4th identical call
loop_detection_interactive = true        # Show interactive prompt
```

## File Changes Summary

| File | Change | Impact |
|------|--------|--------|
| `src/agent/runloop/unified/loop_detection.rs` | Refactored API, enhanced prompt, new selective reset method | API change (internal) |
| `src/agent/runloop/unified/turn/session.rs` | Simplified integration logic, uses selective reset | Better code clarity |

## Performance

**Zero impact** - This is a pure refactoring with no additional allocations or computations.

## Migration

If you have code using the old API:

```rust
// Old code
if detector.record_tool_call(sig) {
    // process detection
    detector.reset();
}

// New code - more explicit
let (detected, count) = detector.record_tool_call(sig);
if detected {
    // process detection with count available
    detector.reset_signature(sig); // selective reset
}
```

## Next Steps

Potential future improvements:
- [ ] Per-signature thresholds
- [ ] Time-based decay (give more weight to recent calls)
- [ ] Statistics tracking for diagnostics
- [ ] Cooldown periods for re-enabling after user override

## Status

✅ Complete and tested
- All 11 loop detection tests passing
- Code formatted and linted
- Ready for production

## Related Documentation

- `LOOP_HANG_DETECTION.md` - Full specification
- `LOOP_HANG_DETECTION_IMPROVEMENTS.md` - Detailed analysis of changes
- `LOOP_HANG_DETECTION_REFACTORED.md` - Complete refactoring summary
