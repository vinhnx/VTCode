# Streaming Timeout Handling Implementation

## Summary

Enhanced streaming timeout error handling and added configurable streaming-specific timeout settings to VTCode.

## Changes Made

### 1. Configuration Changes

**Files Modified:**
- `vtcode-config/src/timeouts.rs` - Added `streaming_ceiling_seconds` field
- `vtcode.toml` - Added `streaming_ceiling_seconds = 600` config
- `vtcode.toml.example` - Added documentation for streaming timeout

**New Configuration Field:**
```toml
[timeouts]
streaming_ceiling_seconds = 600  # 10 minutes, can be increased for slow networks
```

Default value: **600 seconds (10 minutes)**
- Can be increased for users with slow networks or large codebases
- Can be set to 0 to disable the timeout (not recommended)

### 2. Error Message Improvement

**File Modified:**
- `vtcode-core/src/llm/providers/gemini.rs`

**Before:**
```
Streaming timeout during operation after Duration { ... }
```

**After:**
```
Streaming request timeout after 333s. Try reducing input length or increasing timeout in config.

Streaming timeout troubleshooting:
- Reduce input length or complexity
- Increase timeout in config: [timeouts] streaming_ceiling_seconds
- Check network connectivity
- Check network stability for streaming connections
- Consider using non-streaming mode for very long inputs
```

The new error message:
- Shows the actual timeout duration in seconds for clarity
- Provides specific, actionable troubleshooting steps
- References the correct configuration field (`streaming_ceiling_seconds`)
- Mentions multiple solutions (timeout increase, input reduction, network checks)

### 3. Documentation

**New File:**
- `docs/config/STREAMING_TIMEOUT.md`

Comprehensive guide covering:
- How streaming timeouts work
- When to adjust timeout values
- Troubleshooting common timeout scenarios
- Performance considerations
- Related configuration options

## Configuration Recommendations

### For Users Experiencing Timeouts

1. **Slow Networks (Recommended first step):**
   ```toml
   streaming_ceiling_seconds = 1200  # 20 minutes
   ```

2. **Very Large Codebases:**
   ```toml
   streaming_ceiling_seconds = 1500  # 25 minutes
   ```

3. **Unreliable Networks:**
   ```toml
   streaming_ceiling_seconds = 1200  # 20 minutes
   warning_threshold_percent = 85    # Warn at 85% instead of default 80%
   ```

## Testing

All configuration changes have been tested:
- ✅ Configuration parsing works correctly
- ✅ Default values are safe and reasonable
- ✅ Validation enforces minimum timeout (15 seconds if enabled)
- ✅ Error messages format correctly
- ✅ All existing tests pass

## User-Facing Impact

When users encounter streaming timeouts, they will now see:
1. Clear duration information (e.g., "timeout after 333s")
2. Specific reference to the configuration field they need to modify
3. Multiple solutions to try (not just "increase timeout")
4. Guidance for diagnosing network issues

## Implementation Notes

- The streaming timeout is configurable per the global `[timeouts]` section
- Default of 600 seconds (10 minutes) balances reasonable waiting time with resource management
- Users can adjust based on their network conditions and codebase size
- The configuration takes effect immediately after restarting VTCode
