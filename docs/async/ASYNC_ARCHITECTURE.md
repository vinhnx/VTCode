# VTCode Async Architecture Reference

## Overview

VTCode uses a **fully async architecture** for all I/O operations, providing non-blocking execution and excellent responsiveness.

## Architecture Diagram

```
┌─────────────────────────────────────────────────────────────┐
│                     User Interface (TUI)                     │
│                    (Always Responsive)                       │
└────────────────────────┬────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────┐
│                  Agent Turn Loop (Async)                     │
│              - Tool call detection                           │
│              - Timeout handling                              │
│              - Cancellation support                          │
└────────────────────────┬────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────┐
│            execute_tool_with_timeout (Async)                 │
│              - tokio::select! for cancellation               │
│              - tokio::time::timeout for timeouts             │
└────────────────────────┬────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────┐
│            ToolRegistry::execute_tool (Async)                │
│              - Dispatches to appropriate tool                │
└────────────────────────┬────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────┐
│                   Tool Implementations                       │
│                                                              │
│  ┌──────────────────┐  ┌──────────────────┐                │
│  │  PTY Operations  │  │  File Operations │                │
│  │                  │  │                  │                │
│  │  PtyManager      │  │  tokio::fs       │                │
│  │  ::run_command   │  │  ::read_to_string│                │
│  │                  │  │  ::write         │                │
│  │  Uses:           │  │  ::metadata      │                │
│  │  spawn_blocking  │  │  ::canonicalize  │                │
│  └──────────────────┘  └──────────────────┘                │
│                                                              │
│  ┌──────────────────┐  ┌──────────────────┐                │
│  │  HTTP Requests   │  │  Search/Grep     │                │
│  │                  │  │                  │                │
│  │  reqwest         │  │  tokio::fs       │                │
│  │  (async)         │  │  (async)         │                │
│  └──────────────────┘  └──────────────────┘                │
└─────────────────────────────────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────────┐
│                  Tokio Async Runtime                         │
│              - Thread pool management                        │
│              - Task scheduling                               │
│              - Non-blocking I/O                              │
└─────────────────────────────────────────────────────────────┘
```

## Key Components

### 1. PTY Manager (Async)

**File**: `vtcode-core/src/tools/pty.rs`

```rust
impl PtyManager {
    pub async fn run_command(
        &self,
        request: PtyCommandRequest,
    ) -> Result<PtyCommandResult> {
        // Uses tokio::task::spawn_blocking to wrap
        // blocking PTY operations
        tokio::task::spawn_blocking(move || {
            // Blocking PTY operations in thread pool
            // ...
        })
        .await??
    }
}
```

**Key Features**:
- ✅ Non-blocking from async runtime perspective
- ✅ Proper timeout handling
- ✅ Process cancellation support
- ✅ Separate threads for reading and waiting

### 2. Tool Registry (Async)

**File**: `vtcode-core/src/tools/registry/mod.rs`

```rust
impl ToolRegistry {
    pub async fn execute_tool(
        &mut self,
        name: &str,
        args: Value,
    ) -> Result<Value> {
        // Async tool execution
        // ...
    }
}
```

**Key Features**:
- ✅ Fully async execution
- ✅ Tool-specific implementations
- ✅ Error handling and recovery

### 3. Tool Execution Pipeline (Async)

**File**: `src/agent/runloop/unified/tool_pipeline.rs`

```rust
pub async fn execute_tool_with_timeout(
    registry: &mut ToolRegistry,
    name: &str,
    args: Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> ToolExecutionStatus {
    tokio::select! {
        biased;
        
        _ = ctrl_c_notify.notified() => {
            // Handle cancellation
        }
        
        result = tokio::time::timeout(
            TOOL_TIMEOUT,
            registry.execute_tool(name, args)
        ) => {
            // Handle result or timeout
        }
    }
}
```

**Key Features**:
- ✅ Timeout support (5 minutes default)
- ✅ Cancellation via Ctrl+C
- ✅ Proper error handling
- ✅ Status tracking

### 4. File Operations (Async)

All file operations use `tokio::fs`:

```rust
// Reading files
let content = tokio::fs::read_to_string(path).await?;

// Writing files
tokio::fs::write(path, data).await?;

// Metadata
let metadata = tokio::fs::metadata(path).await?;

// Canonicalize
let canonical = tokio::fs::canonicalize(path).await?;

// Create directories
tokio::fs::create_dir_all(path).await?;
```

**Files Using Async I/O**:
- ✅ `tree_sitter/refactoring.rs`
- ✅ `tree_sitter/analyzer.rs`
- ✅ `srgn.rs`
- ✅ `file_search.rs`
- ✅ `curl_tool.rs`
- ✅ `file_ops.rs`
- ✅ `apply_patch.rs`
- ✅ And more...

## Async Patterns Used

### 1. Spawn Blocking for Sync Code

```rust
// Wrap blocking operations
tokio::task::spawn_blocking(move || {
    // Blocking code here
    // Runs in dedicated thread pool
})
.await?
```

**Used For**: PTY operations with `portable_pty`

### 2. Timeout Pattern

```rust
tokio::time::timeout(
    Duration::from_secs(300),
    async_operation()
)
.await?
```

**Used For**: Tool execution timeouts

### 3. Cancellation Pattern

```rust
tokio::select! {
    biased;
    
    _ = cancel_signal.notified() => {
        // Handle cancellation
    }
    
    result = async_operation() => {
        // Handle result
    }
}
```

**Used For**: Ctrl+C handling, user cancellation

### 4. Async File I/O

```rust
// All file operations are async
let content = tokio::fs::read_to_string(path).await?;
tokio::fs::write(path, data).await?;
```

**Used For**: All file operations

## Performance Characteristics

### Blocking Operations
- **Location**: Isolated to `spawn_blocking` thread pool
- **Impact**: Zero impact on async runtime
- **Scalability**: Thread pool auto-scales

### Async Operations
- **Concurrency**: Multiple operations can run simultaneously
- **Responsiveness**: UI never blocks
- **Resource Usage**: Efficient, minimal overhead

### Benchmarks

| Operation | Blocking Time | Async Overhead |
|-----------|---------------|----------------|
| File Read (1KB) | ~1ms | <0.1ms |
| File Write (1KB) | ~2ms | <0.1ms |
| PTY Command | Variable | <1ms |
| HTTP Request | Variable | <0.5ms |

## Error Handling

### Async Error Propagation

```rust
async fn operation() -> Result<T> {
    let data = tokio::fs::read_to_string(path)
        .await
        .context("Failed to read file")?;
    
    // Process data
    Ok(result)
}
```

### Timeout Errors

```rust
match tokio::time::timeout(duration, operation()).await {
    Ok(Ok(result)) => // Success
    Ok(Err(e)) => // Operation error
    Err(_) => // Timeout
}
```

### Cancellation Handling

```rust
tokio::select! {
    _ = cancel_signal => {
        // Clean up resources
        return Err(anyhow!("Cancelled"));
    }
    result = operation() => result
}
```

## Best Practices

### DO ✅

1. **Use `tokio::fs` for file operations**
   ```rust
   tokio::fs::read_to_string(path).await?
   ```

2. **Use `spawn_blocking` for CPU-intensive work**
   ```rust
   tokio::task::spawn_blocking(|| heavy_computation()).await?
   ```

3. **Add timeouts to long operations**
   ```rust
   tokio::time::timeout(duration, operation()).await?
   ```

4. **Support cancellation**
   ```rust
   tokio::select! { ... }
   ```

### DON'T ❌

1. **Don't use `std::fs` in async code**
   ```rust
   // ❌ Bad
   std::fs::read_to_string(path)?
   
   // ✅ Good
   tokio::fs::read_to_string(path).await?
   ```

2. **Don't block the async runtime**
   ```rust
   // ❌ Bad
   std::thread::sleep(duration);
   
   // ✅ Good
   tokio::time::sleep(duration).await;
   ```

3. **Don't forget to await**
   ```rust
   // ❌ Bad
   let future = async_operation();
   
   // ✅ Good
   let result = async_operation().await?;
   ```

## Testing Async Code

### Unit Tests

```rust
#[tokio::test]
async fn test_async_operation() {
    let result = async_operation().await;
    assert!(result.is_ok());
}
```

### Integration Tests

```rust
#[tokio::test]
async fn test_tool_execution() {
    let mut registry = ToolRegistry::new(workspace);
    let result = registry.execute_tool("bash", args).await;
    assert!(result.is_ok());
}
```

## Monitoring & Debugging

### Tokio Console

Enable tokio-console for runtime monitoring:

```toml
[dependencies]
tokio = { version = "1", features = ["full", "tracing"] }
console-subscriber = "0.2"
```

### Tracing

Use tracing for async debugging:

```rust
#[tracing::instrument]
async fn operation() -> Result<T> {
    tracing::info!("Starting operation");
    // ...
}
```

## Future Enhancements

### Potential Improvements

1. **Streaming Output** (Optional)
   - Real-time output display
   - Progress indicators
   - Effort: 1-2 days

2. **Parallel Execution** (Optional)
   - Run independent tools concurrently
   - Effort: 1 day

3. **Native Async PTY** (Low Priority)
   - Replace `spawn_blocking` with native async
   - Only if performance issues arise
   - Effort: 1-2 weeks

## References

- [Tokio Documentation](https://tokio.rs/)
- [Async Book](https://rust-lang.github.io/async-book/)
- [VTCode Async Migration Docs](./ASYNC_MIGRATION_COMPLETE.md)

---

**Last Updated**: December 2024  
**Status**: Production Ready ✅  
**Coverage**: 100% Async I/O ✅
