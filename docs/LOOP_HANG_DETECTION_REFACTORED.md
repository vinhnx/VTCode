# Loop Hang Detection - Refactored Implementation

## Summary of Improvements

The loop hang detection system has been refactored for **better accuracy, user experience, and maintainability**. All tests passing (11/11).

## Key Improvements Implemented

### 1. **Better API - Return Tuple with Count**

**Before:**
```rust
pub fn record_tool_call(&mut self, signature: &str) -> bool
```

**After:**
```rust
pub fn record_tool_call(&mut self, signature: &str) -> (bool, usize)
```

**Benefit**: The caller now gets both whether a loop was detected AND the repeat count, enabling richer user feedback without an extra API call.

### 2. **Selective Signature Reset**

**New Method:**
```rust
pub fn reset_signature(&mut self, signature: &str) {
    self.repeated_calls.remove(signature);
}
```

**Benefit**: When user selects "Keep detection enabled", only the problematic signature is reset. Other signatures retain their counts, allowing the detector to maintain context about overall model behavior. This prevents clearing useful data.

**Usage in session.rs:**
```rust
// KeepEnabled: Only reset the looping signature
loop_detector.reset_signature(&signature_key);

// DisableForSession: Clear everything for fresh start
loop_detector.reset();
```

### 3. **Enhanced User Prompt with Context**

**Before:**
```
A potential loop was detected. Do you want to keep loop detection enabled or disable it for this session?
```

**After:**
```
A potential loop was detected.

Looping tool call: 'read_file::{"path": "/Users/..."}'
Repeat count: 5

What would you like to do?

Options:
  1. Keep loop detection enabled (esc)
  2. Disable loop detection for this session
```

**Implementation:**
```rust
fn show_loop_detection_prompt_tui(
    signature: &str,
    repeat_count: usize
) -> Result<LoopDetectionResponse>
```

Signature is truncated to 100 chars if too long, making the UI readable while informative.

### 4. **Cleaner Session Integration**

**Before:**
```rust
if !loop_detection_disabled_for_session && loop_detector.record_tool_call(&signature_key) {
    // Show verbose context messages
    renderer.line(MessageStyle::Output, "")?;
    renderer.line(MessageStyle::Error, "A potential loop was detected")?;
    // ... etc
    
    // Get user's choice
    match prompt_for_loop_detection(loop_detection_interactive) {
        // ...
    }
}
```

**After:**
```rust
let (is_loop_detected, repeat_count) = if !loop_detection_disabled_for_session {
    loop_detector.record_tool_call(&signature_key)
} else {
    (false, 0)
};

if is_loop_detected {
    // Prompt includes context directly
    match prompt_for_loop_detection(loop_detection_interactive, &signature_key, repeat_count) {
        Ok(LoopDetectionResponse::KeepEnabled) => {
            renderer.line(MessageStyle::Info, "Loop detection remains enabled. Skipping this tool call.")?;
            loop_detector.reset_signature(&signature_key); // Selective reset
            continue;
        }
        // ...
    }
}
```

**Benefits:**
- More readable code flow
- No redundant context messages (prompt includes them)
- Clearer intent (tuple unpacking shows what's happening)
- Selective reset is explicit

## Test Coverage

All 11 tests passing:

```
 test_loop_detector_threshold           - Core detection at count > threshold
 test_loop_detector_disabled            - Disabled detector never triggers  
 test_loop_detector_reset               - Full state reset works
 test_loop_detector_different_signatures- Each signature tracked independently
 test_loop_detector_interactive_flag    - Interactive flag correctness
 test_loop_detector_enable_disable      - Runtime enable/disable
 test_loop_detection_response_enum      - Response enum validation
 test_peek_count                        - Peek functionality
 test_would_trigger                     - Predictive check
 test_non_interactive_mode              - Non-interactive detection works
 test_selective_reset                   - NEW: Selective reset functionality
```

### New Test: Selective Reset

```rust
#[test]
fn test_selective_reset() {
    let mut detector = LoopDetector::new(2, true, true);

    // Record two different signatures
    detector.record_tool_call("sig1"); // count = 1
    detector.record_tool_call("sig1"); // count = 2
    detector.record_tool_call("sig2"); // count = 1
    detector.record_tool_call("sig2"); // count = 2

    assert_eq!(detector.get_count("sig1"), 2);
    assert_eq!(detector.get_count("sig2"), 2);

    // Selectively reset only sig1
    detector.reset_signature("sig1");

    assert_eq!(detector.get_count("sig1"), 0); // Reset
    assert_eq!(detector.get_count("sig2"), 2); // Untouched
}
```

## Files Modified

1. **`src/agent/runloop/unified/loop_detection.rs`**
   - Changed `record_tool_call()` return type from `bool` to `(bool, usize)`
   - Added `reset_signature()` method
   - Enhanced `show_loop_detection_prompt_tui()` with context parameters
   - Updated `prompt_for_loop_detection()` signature
   - Added test for selective reset
   - Updated 10 existing tests to use new tuple return type

2. **`src/agent/runloop/unified/turn/session.rs`**
   - Simplified loop detection check logic
   - Use tuple unpacking for clarity
   - Implement selective reset on KeepEnabled
   - Pass signature and count to prompt function
   - Removed redundant context message rendering

## Behavioral Changes

### For Users

- **Better Visibility**: When a loop is detected, you now see which specific tool call is looping and how many times it repeated
- **Better Recovery**: Choosing "Keep enabled" only resets that one problematic call, allowing the detector to continue monitoring other patterns
- **Same functionality**: The overall behavior is identical—loop detection still works the same way

### For Developers

- **Cleaner API**: `record_tool_call()` returns both status and count in one call
- **More flexible**: Can selectively reset signatures instead of clearing everything
- **Better encapsulation**: Context is passed to prompt function instead of rendering separately

## Performance Impact

**None**: The refactoring is zero-cost. No additional allocations or computations.

## Backward Compatibility

 **Breaking API Change**: 

The `record_tool_call()` return type changed from `bool` to `(bool, usize)`. Code calling this method must be updated:

```rust
// Old code
if detector.record_tool_call(sig) {
    // ...
}

// New code
let (detected, count) = detector.record_tool_call(sig);
if detected {
    // use count for context
}
```

All internal code has been updated. This is internal API only (not user-facing).

## Future Enhancement Opportunities

Based on this refactoring, we can now easily add:

1. **Per-Signature Thresholds**: Different thresholds for different tool types
2. **Signature-Specific Cooldowns**: Track when each signature last triggered detection
3. **Metrics Export**: Export detection statistics with access to repeat counts
4. **Weighted Detection**: Give more weight to recent calls vs. old ones

## Status

  **COMPLETE** - Refactored and tested
- All 11 tests passing
- Code compiles without errors
- Ready for integration

## Summary

The refactored loop hang detection is **more user-friendly, architecturally cleaner, and more flexible** while maintaining the same safety guarantees. The implementation demonstrates good API design principles (return relevant data, don't repeat work) and makes the codebase easier to extend in the future.
