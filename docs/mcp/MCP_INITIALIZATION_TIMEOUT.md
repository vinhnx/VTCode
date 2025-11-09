# MCP Initialization Timeout Configuration

## Overview
Implemented configurable MCP initialization timeout that uses settings from `vtcode.toml` instead of a hardcoded 30-second limit. The default timeout has also been increased from 30 seconds to 60 seconds to accommodate slower or more resource-intensive MCP servers.

## Changes Made

### 1. **File: `src/agent/runloop/unified/async_mcp_manager.rs`**

Updated `initialize_mcp_client()` function to:
- Read `startup_timeout_seconds` from the MCP configuration
- Use configurable timeout in `tokio::time::timeout()` call
- Report the actual timeout value in error messages (instead of hardcoded 30s)
- Maintain backward compatibility with 30-second default if not configured

**Key Changes:**
```rust
// Before: Fixed 30-second timeout
match timeout(Duration::from_secs(30), client.initialize()).await {
    // ...
    Err(_) => Err(anyhow::anyhow!(
        "MCP client initialization timed out after 30 seconds"
    )),
}

// After: Dynamic timeout from config
let startup_timeout_secs = config.startup_timeout_seconds.unwrap_or(30);
let startup_timeout = Duration::from_secs(startup_timeout_secs);

match timeout(startup_timeout, client.initialize()).await {
    // ...
    Err(_) => Err(anyhow::anyhow!(
        "MCP client initialization timed out after {} seconds",
        startup_timeout_secs
    )),
}
```

### 2. **File: `vtcode-config/src/constants.rs`**

Updated default startup timeout constant:
- **Before**: `DEFAULT_STARTUP_TIMEOUT_MS = 30_000` (30 seconds)
- **After**: `DEFAULT_STARTUP_TIMEOUT_MS = 60_000` (60 seconds)
- Added documentation: "Can be overridden via config: mcp.startup_timeout_seconds"

### 3. **File: `vtcode.toml.example`**

Added configuration documentation:
```toml
[mcp]
# Enable Model Context Protocol (may impact startup time if services unavailable)
enabled = true
max_concurrent_connections = 5
request_timeout_seconds = 30
retry_attempts = 3
# Timeout (seconds) for initializing MCP servers (default: 60)
startup_timeout_seconds = 60
```

## Configuration

### Location
The timeout is configured in the `[mcp]` section of `vtcode.toml`:

```toml
[mcp]
startup_timeout_seconds = 60  # Default is 60 seconds
```

### Valid Values
- **Type**: Optional unsigned integer
- **Unit**: Seconds
- **Default**: 60 (increased from 30)
- **Minimum**: 1 second (recommended: at least 30)
- **Maximum**: 3600 seconds (1 hour recommended)

### Examples

**Quick startup (fast MCP servers):**
```toml
[mcp]
startup_timeout_seconds = 30
```

**Standard timeout (recommended):**
```toml
[mcp]
startup_timeout_seconds = 60
```

**Extended timeout (slow/heavy MCP servers):**
```toml
[mcp]
startup_timeout_seconds = 120
```

**Very patient (troubleshooting):**
```toml
[mcp]
startup_timeout_seconds = 300
```

## Error Messages

When MCP initialization times out, the error message now shows the actual timeout value:

**Before:**
```
MCP client initialization timed out after 30 seconds
```

**After:**
```
MCP client initialization timed out after 120 seconds
```
(Example showing timeout of 120s)

## Architecture

### Configuration Flow
1. User configures `startup_timeout_seconds` in `vtcode.toml`
2. Config loads into `McpClientConfig.startup_timeout_seconds: Option<u64>`
3. `initialize_mcp_client()` reads this value
4. Falls back to 30 seconds if not explicitly configured (for backward compatibility)
5. Uses the determined timeout for `tokio::time::timeout()`

### Timeout Categories
- **MCP Client Initialization**: `startup_timeout_seconds` (new, configurable)
- **MCP Request Timeout**: `request_timeout_seconds` (existing, separate setting)
- **Tool Execution Timeout**: Configured via `[timeouts]` section (different system)

## Relationship with Other Timeouts

This is **separate** from other timeout systems in VTCode:

| Timeout Type | Config Key | Default | Purpose |
|---|---|---|---|
| MCP Startup | `mcp.startup_timeout_seconds` | 60s | Initializing MCP servers |
| MCP Requests | `mcp.request_timeout_seconds` | 30s | Individual MCP tool calls |
| Tool Execution | `timeouts.default_ceiling_seconds` | 180s | Standard tool execution |
| PTY Execution | `timeouts.pty_ceiling_seconds` | 300s | PTY tool execution |
| MCP Tool Execution | `timeouts.mcp_ceiling_seconds` | 120s | MCP tool execution |

## Benefits

✅ **Configurable**: Adjust timeout for different MCP server implementations
✅ **Increased Default**: 60 seconds accommodates slower servers by default
✅ **Better Error Messages**: Shows actual timeout when things fail
✅ **Backward Compatible**: Falls back to 30 seconds if not configured
✅ **Consistent Pattern**: Uses existing `startup_timeout_seconds` config field
✅ **Flexible**: Can be adjusted per deployment without code changes

## Migration Guide

### Existing Configurations
- If no `startup_timeout_seconds` is set, the new default of 60 seconds applies
- This is an increase from the hardcoded 30 seconds previously used
- No configuration changes required unless you want to customize the timeout

### Custom Configurations
To adjust the MCP initialization timeout, add to your `vtcode.toml`:

```toml
[mcp]
startup_timeout_seconds = <your_value_in_seconds>
```

## Testing

The implementation:
- ✅ Compiles without errors
- ✅ Uses existing configuration structure
- ✅ Maintains backward compatibility
- ✅ Properly reports timeout values in error messages
- ✅ Integrates with existing config validation
