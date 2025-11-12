# Loop Hang Detection - Improvements & Better Implementation

## Current Status Review

The loop hang detection is **functional and well-tested**, but there are several improvements that would make it more robust and production-ready.

## Issues Identified

### 1. **No Decay or Recency Bias**
**Current Issue**: A signature that triggered detection 20 turns ago is weighted equally to recent calls.

**Example**:
```
Turn 1-3: read_file(/a) - triggers at 3
[...10 turns later...]
Turn 13-15: read_file(/b) - completely different file!
// But if read_file(/a) was recorded earlier, old data persists
```

**Fix**: Add sliding window or time-based decay to give more weight to recent calls.

### 2. **Per-Signature Reset on Detection**
**Current Issue**: Calling `reset()` clears ALL signatures when only one triggered detection.

```rust
// Line 1297 & 1303 in session.rs
loop_detector.reset(); // Clears everything!
```

**Impact**: If the model is actually making progress with one signature while another is stuck, clearing all tracking throws away useful context.

**Better Approach**: Only reset the triggered signature, or implement selective reset.

### 3. **No Contextual Information in Prompt**
**Current Issue**: User sees generic "potential loop" message without knowing:
- Which signature is looping
- How many times it repeated
- What the tool arguments are

```rust
// Line 1281: Just says "A potential loop was detected"
renderer.line(MessageStyle::Error, "A potential loop was detected")?;
```

**Better**: Show count and preview of the looping signature.

### 4. **No Cooldown Period**
**Current Issue**: If user keeps selecting "DisableForSession", no future checks occur at all.

**Better**: Implement a cooldown period (e.g., detect loop after 8 consecutive calls if just re-enabled) to catch new patterns.

### 5. **JSON Serialization for Signature**
**Current Issue**: Serializing args to JSON for comparison is fragile:
```rust
serde_json::to_string(&args_val).unwrap_or_else(|_| "{}".to_string())
```

- Order of object keys might differ
- Large objects make signatures unwieldy
- Fallback to `"{}"` hides actual arguments

**Better**: Use a hash of arguments or `Debug` representation for resilience.

### 6. **Missing Metrics/Telemetry**
**Current Issue**: No way to know:
- How often loops are detected
- What patterns cause loops
- User response distribution

**Better**: Track statistics for diagnostics.

## Recommended Improvements

### Tier 1 (High Priority)

#### 1a. Selective Reset
```rust
pub fn reset_signature(&mut self, signature: &str) {
    self.repeated_calls.remove(signature);
}
```

Then in session.rs:
```rust
match prompt_for_loop_detection(loop_detection_interactive) {
    Ok(LoopDetectionResponse::KeepEnabled) => {
        loop_detector.reset_signature(&signature_key); // Only this one
        continue;
    }
    Ok(LoopDetectionResponse::DisableForSession) => {
        loop_detection_disabled_for_session = true;
        loop_detector.reset(); // Clear all for fresh start
    }
    // ...
}
```

#### 1b. Enhanced Prompt Context
```rust
fn show_loop_detection_prompt_tui(
    signature: &str, 
    count: usize
) -> Result<LoopDetectionResponse> {
    use dialoguer::Select;
    
    let preview = if signature.len() > 80 {
        format!("{}...", &signature[..80])
    } else {
        signature.to_string()
    };
    
    let prompt = format!(
        "Loop detected: '{}' repeated {} times.\nWhat would you like to do?",
        preview, count
    );
    
    // ... rest of implementation
}
```

### Tier 2 (Medium Priority)

#### 2a. Time-Based Decay (Optional)
```rust
use std::time::{SystemTime, UNIX_EPOCH};

pub struct LoopDetector {
    repeated_calls: HashMap<String, (usize, u64)>, // count, last_seen_timestamp
    // ... rest
}

pub fn record_tool_call(&mut self, signature: &str) -> bool {
    if !self.enabled {
        return false;
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let (count, _) = self.repeated_calls.get(signature).copied().unwrap_or((0, now));
    
    // Reset count if more than 30s have passed
    let effective_count = if now.saturating_sub(last_time) > 30 {
        1
    } else {
        count + 1
    };
    
    self.repeated_calls.insert(signature.to_string(), (effective_count, now));
    effective_count > self.threshold
}
```

#### 2b. Resilient Signature Hashing
```rust
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

fn create_signature(tool_name: &str, args: &serde_json::Value) -> String {
    let mut hasher = DefaultHasher::new();
    tool_name.hash(&mut hasher);
    format!("{:x}", hasher.finish()).hash(&mut hasher);
    
    format!("{}::{:x}", tool_name, hasher.finish())
}
```

This is more compact and resilient than full JSON serialization.

### Tier 3 (Nice to Have)

#### 3a. Simple Statistics Tracking
```rust
pub struct LoopDetectionStats {
    total_detections: u32,
    keep_enabled_count: u32,
    disable_session_count: u32,
}

impl LoopDetector {
    pub fn get_stats(&self) -> LoopDetectionStats { ... }
}
```

#### 3b. Graduated Thresholds
Instead of fixed threshold, increase threshold after each detection:
```rust
let adaptive_threshold = base_threshold + (detections_so_far * 2);
```

## Implementation Priority

| Improvement | Impact | Effort | Priority |
|------------|--------|--------|----------|
| Selective reset | High | Low | **P1** |
| Enhanced prompt context | High | Low | **P1** |
| Resilient signature hashing | Medium | Low | **P2** |
| Time-based decay | Medium | Medium | **P2** |
| Statistics tracking | Low | Medium | **P3** |
| Graduated thresholds | Low | Medium | **P3** |

## Specific Code Changes Needed

### File: `src/agent/runloop/unified/loop_detection.rs`

1. Add `reset_signature()` method
2. Update `record_tool_call()` signature to return count for UI
3. Optionally add timestamp tracking for decay
4. Add stats struct for diagnostics

### File: `src/agent/runloop/unified/turn/session.rs`

1. Pass signature and count to prompt function
2. Use selective reset on KeepEnabled
3. Update prompt message with context
4. Update function signature for `prompt_for_loop_detection()`

## Testing Additions

```rust
#[test]
fn test_selective_reset() {
    let mut detector = LoopDetector::new(2, true, true);
    
    detector.record_tool_call("sig1"); // 1
    detector.record_tool_call("sig1"); // 2
    detector.record_tool_call("sig2"); // 1
    
    detector.reset_signature("sig1");
    
    assert_eq!(detector.get_count("sig1"), 0); // Reset
    assert_eq!(detector.get_count("sig2"), 1); // Untouched
}

#[test]
fn test_prompt_with_context() {
    // Mock user input and verify prompt includes signature info
}
```

## Summary

The current implementation is **solid but conservative**. These improvements would make it:

✅ More user-friendly (better context)  
✅ More accurate (selective reset, time decay)  
✅ More debuggable (statistics, hash resilience)  

Start with **Tier 1** for maximum impact with minimal effort. Tier 2-3 are nice additions for future iterations.
