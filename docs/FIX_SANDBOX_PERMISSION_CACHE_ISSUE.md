# Fix: Sandbox Permission Cache Blocking Retries

## Problem
The vtcode agent would sometimes refuse to run commands with sandbox permission errors on first try, but then succeed on retry. The issue was in the permission cache logic incorrectly treating execution failures as permanent policy denials.

## Root Cause
The `PermissionGrant` cache was conflating two different types of denials:

1. **Policy-based denials** - User or policy explicitly denied access to a tool
2. **Execution failures** - A command failed due to a transient sandbox issue (e.g., temporary permissions)

When a command failed with a permission error, it was cached with `PermissionGrant::Denied`. On the next attempt, `tool_routing.rs` would check the cache and immediately reject execution based on the cached denial, preventing retry.

## Solution

### 1. Added `PermissionGrant::TemporaryDenial` variant
Distinguishes transient execution failures from permanent policy denials:

```rust
pub enum PermissionGrant {
    Once,
    Session,
    Permanent,
    Denied,                    // Explicit policy denial
    TemporaryDenial,           // Execution failure (retryable)
}
```

### 2. Added detection methods
- `is_denied()` - Returns true for policy-based denials only
- `is_temporarily_denied()` - Returns true for execution failures
- `can_use_cached()` - Returns false for `TemporaryDenial` (forces retry)

### 3. Added cleanup method
- `clear_temporary_denials()` - Removes transient denials while preserving policy denials

### 4. Updated tool routing logic
`tool_routing.rs:300-301` now only rejects tools for explicit policy denials, not temporary execution failures. The `can_use_cached()` method explicitly excludes `TemporaryDenial`, forcing the tool to be re-evaluated on next attempt.

## Implementation Details

### Files Modified
1. **vtcode-core/src/acp/permission_cache.rs**
   - Added `TemporaryDenial` variant to `PermissionGrant` enum
   - Added `is_temporarily_denied()` methods for both caches
   - Added `clear_temporary_denials()` methods for both caches
   - Updated `can_use_cached()` to exclude temporary denials
   - Added comprehensive tests

2. **src/agent/runloop/unified/tool_routing.rs**
   - Clarified comments distinguishing policy denials from execution failures
   - Confirmed only policy denials (via `is_denied()`) cause immediate rejection

## Testing
New tests verify:
- Policy denials and temporary denials are correctly distinguished
- `clear_temporary_denials()` preserves policy denials and session grants
- `can_use_cached()` returns false for temporary denials (enabling retries)

## Impact
- **Positive**: Transient sandbox permission errors no longer block retries
- **Safe**: Policy-based denials continue to be respected
- **Backward compatible**: Existing code continues to work as-is

## Future Improvements
1. Consider implementing exponential backoff for retries
2. Add metrics to track temporary denial rates
3. Consider time-based expiration for temporary denials
