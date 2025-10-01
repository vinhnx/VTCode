# MCP BrokenPipeError Fix

## Problem

When running `cargo run`, the MCP server initialization would fail with a `BrokenPipeError`:

```
Exception Group Traceback (most recent call last):
  File "/Users/.../mcp-server-time", line 12, in <module>
    sys.exit(main())
  ...
  BrokenPipeError: [Errno 32] Broken pipe
```

## Root Cause

The issue was caused by calling `cleanup_dead_providers()` immediately after MCP client initialization. This method would:

1. Call `has_tool("ping")` on each provider to check health
2. This would trigger `list_tools()` which spawns the MCP server child process
3. The connection would be established and then immediately checked/closed
4. The MCP server process would still be writing to stdout during initialization
5. When the pipe was closed prematurely, the server would throw `BrokenPipeError`

## Solution

Removed the `cleanup_dead_providers()` call from two locations:

1. **vtcode-core/src/mcp_client.rs** - In the `initialize()` method
2. **src/agent/runloop/unified/session_setup.rs** - After successful MCP client initialization

### Rationale

The cleanup call was unnecessary during initialization because:
- No connections have been established yet at this point
- There are no "dead providers" to clean up
- Cleanup will happen naturally when connections are first established and fail

## Changes Made

### File: `vtcode-core/src/mcp_client.rs`

```rust
// Before
info!("MCP client initialization complete. Active providers: {}", self.providers.len());
// Clean up any providers with terminated processes
let _ = self.cleanup_dead_providers().await;
Ok(())

// After
info!("MCP client initialization complete. Active providers: {}", self.providers.len());
// Note: We don't call cleanup_dead_providers() here because no connections
// have been established yet during initialization. Cleanup will happen
// naturally when connections are first established and fail.
Ok(())
```

### File: `src/agent/runloop/unified/session_setup.rs`

```rust
// Before
Ok(Ok(())) => {
    info!("MCP client initialized successfully");
    // Clean up any providers with terminated processes after initialization
    if let Err(e) = client.cleanup_dead_providers().await {
        // error handling...
    }
    (Some(Arc::new(client)), None)
}

// After
Ok(Ok(())) => {
    info!("MCP client initialized successfully");
    // Note: We don't call cleanup_dead_providers() here because no connections
    // have been established yet during initialization. Cleanup will happen
    // naturally when connections are first established and fail.
    (Some(Arc::new(client)), None)
}
```

## Testing

After applying the fix:
- `cargo run` completes without errors
- MCP servers start successfully (confirmed by process listing)
- No `BrokenPipeError` exceptions occur
- Sequential Thinking and Context7 servers display startup messages
- Time server starts correctly (though it doesn't print a startup message)

## Impact

- **Positive**: Fixes the initialization crash and allows MCP servers to start cleanly
- **No Regression**: The `cleanup_dead_providers()` method is still available and will be called when connections are actually established and fail
- **Performance**: Slight improvement by not spawning unnecessary connections during initialization

## Related Code

The `cleanup_dead_providers()` method is still useful and will be called:
- When connections are first established via `get_or_create_connection()`
- When tool execution fails with connection errors
- During explicit cleanup operations

This ensures proper cleanup of dead connections without interfering with initial startup.
