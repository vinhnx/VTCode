# Fix: Async Runtime Panic in PTY Session Reader

## Problem

**Error**: `Cannot start a runtime from within a runtime`

**Location**: `vtcode-core/src/tools/registry/executors.rs:2098`

**Stack Trace**:
```
thread 'main' panicked at vtcode-core/src/tools/registry/executors.rs:2098:52:
Cannot start a runtime from within a runtime. This happens because a function 
(like `block_on`) attempted to block the current thread while the thread is 
being used to drive asynchronous tasks.
```

## Root Cause

The `execute_read_pty_session` function is declared as `async fn`, which means it runs within an async runtime context. Within this async context, the code was attempting to call `tokio::runtime::Handle::current().block_on()`, which tries to create a blocking context within an already-running async runtime.

**Problematic Code**:
```rust
async fn execute_read_pty_session(&mut self, args: Value) -> Result<Value> {
    // ... async operations ...
    
    // WRONG: Can't use block_on inside async function
    let rt = tokio::runtime::Handle::current();
    let (truncated_output, _) = rt.block_on(async {
        truncate_content_by_tokens(&output, max_tokens, &token_budget).await
    });
}
```

## Solution

Since the function is already running in an async context, we can directly `await` the async function instead of trying to block on it.

**Fixed Code**:
```rust
async fn execute_read_pty_session(&mut self, args: Value) -> Result<Value> {
    // ... async operations ...
    
    // CORRECT: await directly in async context
    let (truncated_output, _) =
        truncate_content_by_tokens(&output, max_tokens, &token_budget).await;
}
```

## Changes Made

**File**: `vtcode-core/src/tools/registry/executors.rs`

**Lines**: 2096-2098

**Before**:
```rust
// This needs to be inside an async block since truncate_content_by_tokens is async
let rt = tokio::runtime::Handle::current();
let (truncated_output, _) = rt.block_on(async {
    truncate_content_by_tokens(&output, max_tokens, &token_budget).await
});
```

**After**:
```rust
// Since we're already in an async context, we can await directly
let (truncated_output, _) =
    truncate_content_by_tokens(&output, max_tokens, &token_budget).await;
```

## Why This Works

### Async Context Understanding

- **`block_on()`**: Used to run async code from **synchronous** contexts. It blocks the current thread until the async operation completes.
- **`await`**: Used to wait for async code from **within** async contexts. It yields control without blocking the thread.

### Key Principle

When you're already inside an async function (marked with `async fn`), you're already running within an executor's runtime. Attempting to create another blocking context violates tokio's runtime model:

```
[Main Thread - tokio runtime running]
  ↓
[async fn execute_read_pty_session running]
  ↓
❌ rt.block_on() - WRONG: Tries to block a thread being used by runtime
✅ await - CORRECT: Yields control to runtime
```

## Testing

✅ Code compiles without errors
✅ No runtime panics
✅ "Full Auto Trust" feature now works correctly
✅ PTY session reading with token truncation functions properly

## Related Concepts

### Async Patterns in Rust

1. **Sync Code → Async Function**: Use `block_on()` or `tokio::spawn_blocking()`
2. **Async Code → Async Function**: Use `await`
3. **Async Code → Sync Function**: Not directly possible; refactor as needed

### Tokio Runtime Rules

- Only one runtime per thread
- Cannot call `block_on()` from within an async task
- Use `spawn_blocking()` for synchronous operations within async context
- Use `await` for async operations within async context

## Performance Impact

- **No impact**: This is a direct API change with identical performance
- **Removed unnecessary allocation**: No longer creating Handle and runtime state
- **Cleaner code**: Fewer function calls and better readability

## Conclusion

This fix resolves the panic by respecting Rust's async runtime semantics. The "Full Auto Trust" feature now works correctly, and token truncation during PTY session reading proceeds smoothly.
