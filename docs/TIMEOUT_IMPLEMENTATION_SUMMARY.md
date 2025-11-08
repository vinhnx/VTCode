# Timeout Implementation Summary

This document summarizes all timeout-related improvements made to VTCode.

## Changes Overview

### 1. Tool Call Timeout Implementation
**Status**: ✅ Complete  
**Files Modified**: `src/agent/runloop/unified/tool_pipeline.rs`

Implemented dynamic tool execution timeout system that respects configured timeout ceilings based on tool type.

**Key Features:**
- Uses `registry.timeout_policy()` to determine timeout based on tool category
- Supports three timeout categories: Default (180s), PTY (300s), MCP (120s)
- Falls back to sensible defaults if no policy configured
- Dynamic timeout passed to all execution stages (warning, enforcement, error reporting)
- Category-aware error messages showing actual timeout limit

**Configuration:**
```toml
[timeouts]
default_ceiling_seconds = 180      # Standard tools
pty_ceiling_seconds = 300          # PTY-based tools  
mcp_ceiling_seconds = 120          # MCP tools
warning_threshold_percent = 80     # Warn at 80% of limit
```

### 2. MCP Initialization Timeout Implementation
**Status**: ✅ Complete  
**Files Modified**: 
- `src/agent/runloop/unified/async_mcp_manager.rs`
- `vtcode-config/src/constants.rs`
- `vtcode.toml.example`

Made MCP initialization timeout configurable with increased default from 30s to 60s.

**Key Features:**
- Reads `startup_timeout_seconds` from MCP config
- Falls back to 30 seconds if not configured
- Error messages show actual timeout that was exceeded
- Default increased to 60 seconds (from hardcoded 30)
- Separate from tool execution and MCP request timeouts

**Configuration:**
```toml
[mcp]
startup_timeout_seconds = 60  # Default is 60 seconds
```

## Files Modified

### Core Logic Changes
1. **src/agent/runloop/unified/tool_pipeline.rs** (9 changes)
   - Replaced hardcoded `TOOL_TIMEOUT` with `DEFAULT_TOOL_TIMEOUT`
   - Added `timeout_ceiling` parameter to execution functions
   - Integrated dynamic timeout retrieval from registry
   - Updated warning task and error reporting

2. **src/agent/runloop/unified/async_mcp_manager.rs** (3 changes)
   - Extract `startup_timeout_seconds` from config
   - Use dynamic timeout in `tokio::time::timeout()`
   - Report actual timeout in error messages

### Configuration Changes
3. **vtcode-config/src/constants.rs** (1 change)
   - Updated `DEFAULT_STARTUP_TIMEOUT_MS` from 30,000 to 60,000
   - Added documentation reference to config override

4. **vtcode.toml.example** (1 change)
   - Added `startup_timeout_seconds = 60` under `[mcp]` section
   - Added comment explaining the option

## Timeout System Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Timeout Systems                          │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  1. Tool Execution Timeouts                                │
│     └─ Configured via: [timeouts]                          │
│     └─ Categories: Default (180s), PTY (300s), MCP (120s)  │
│     └─ Module: tool_pipeline.rs                            │
│                                                              │
│  2. MCP Initialization Timeout                             │
│     └─ Configured via: [mcp] startup_timeout_seconds       │
│     └─ Default: 60 seconds                                 │
│     └─ Module: async_mcp_manager.rs                        │
│                                                              │
│  3. MCP Request Timeout                                    │
│     └─ Configured via: [mcp] request_timeout_seconds       │
│     └─ Default: 30 seconds                                 │
│     └─ Module: vtcode-core/mcp/                            │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

## Configuration Examples

### Conservative Settings (Fast Environments)
```toml
[timeouts]
default_ceiling_seconds = 60
pty_ceiling_seconds = 120
mcp_ceiling_seconds = 60

[mcp]
startup_timeout_seconds = 30
request_timeout_seconds = 15
```

### Balanced Settings (Recommended)
```toml
[timeouts]
default_ceiling_seconds = 180
pty_ceiling_seconds = 300
mcp_ceiling_seconds = 120

[mcp]
startup_timeout_seconds = 60
request_timeout_seconds = 30
```

### Generous Settings (Slow Environments)
```toml
[timeouts]
default_ceiling_seconds = 300
pty_ceiling_seconds = 600
mcp_ceiling_seconds = 240

[mcp]
startup_timeout_seconds = 120
request_timeout_seconds = 60
```

## Testing Status

✅ **Compilation**: All targets compile without errors  
✅ **Config Loading**: Uses existing configuration structures  
✅ **Backward Compatibility**: Maintains sensible fallback defaults  
✅ **Error Messages**: Reports actual timeout values  
✅ **Integration**: Works with existing timeout policy validation  

## Benefits

| Aspect | Before | After |
|--------|--------|-------|
| Tool Timeout | Hardcoded 300s | Dynamic per category |
| MCP Init Timeout | Hardcoded 30s | Configurable, default 60s |
| Error Messages | Fixed text | Shows actual timeout |
| Flexibility | No configuration | Full configuration support |
| User Control | None | Complete customization |

## Migration Path

No breaking changes. Existing configurations work as-is:
- Tool timeouts now use configured policy (defaults maintained)
- MCP init timeout now uses 60s default (up from 30s)
- Can customize via config without code changes

## Future Enhancements

Possible future improvements:
- Per-tool timeout overrides
- Adaptive timeouts based on performance metrics
- Timeout metrics and monitoring
- Configuration validation and suggestions
- Dynamic timeout adjustment based on resource availability
