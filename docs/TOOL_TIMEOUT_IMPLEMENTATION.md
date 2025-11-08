# Tool Call Timeout Implementation

## Overview
Implemented dynamic tool execution timeout handling that uses configured timeout ceilings from `vtcode.toml` instead of a hardcoded 300-second constant.

## Changes Made

### Modified File: `src/agent/runloop/unified/tool_pipeline.rs`

#### 1. **Replaced Hardcoded Constant**
   - **Before**: `const TOOL_TIMEOUT: Duration = Duration::from_secs(300);`
   - **After**: `const DEFAULT_TOOL_TIMEOUT: Duration = Duration::from_secs(300);`
   - The constant is now used as a fallback only when no timeout policy is configured

#### 2. **Updated `execute_tool_with_timeout()` Function**
   - Now determines the timeout category for the tool (Default, PTY, or MCP)
   - Retrieves the appropriate timeout ceiling from the registry's timeout policy
   - Passes the dynamically determined timeout to the execution function
   - Falls back to `DEFAULT_TOOL_TIMEOUT` if no policy is configured

#### 3. **Updated `execute_tool_with_progress()` Function**
   - Added `tool_timeout: Duration` parameter
   - Uses the provided timeout instead of hardcoded constant
   - Passes timeout to warning task spawning

#### 4. **Updated Timeout Enforcement**
   - Uses `time::timeout(tool_timeout, exec_future)` with the dynamic timeout
   - Correctly reports timeout category in error message

#### 5. **Updated Warning Task**
   - `spawn_timeout_warning_task()` now accepts `tool_timeout` parameter
   - Computes warning delay based on actual timeout ceiling
   - Reports correct timeout limit in warning messages

## Timeout Configuration Structure

The timeout policy is configured in `vtcode.toml` under the `[timeouts]` section:

```toml
[timeouts]
# Default ceiling for standard tools (seconds)
default_ceiling_seconds = 180

# Ceiling for PTY-based tools (seconds)
pty_ceiling_seconds = 300

# Ceiling for MCP tools (seconds)
mcp_ceiling_seconds = 120

# Warning threshold as percentage (0-100)
warning_threshold_percent = 80
```

## Tool Timeout Categories

The timeout applied depends on the tool type:

1. **Default Tools** (`ToolTimeoutCategory::Default`)
   - Standard tools using direct execution
   - Uses `default_ceiling_seconds` from config

2. **PTY Tools** (`ToolTimeoutCategory::Pty`)
   - Tools that require pseudo-terminal execution
   - Uses `pty_ceiling_seconds` from config (falls back to `default_ceiling_seconds`)

3. **MCP Tools** (`ToolTimeoutCategory::Mcp`)
   - Tools provided by MCP (Model Context Protocol)
   - Uses `mcp_ceiling_seconds` from config (falls back to `default_ceiling_seconds`)

## How It Works

1. When `execute_tool_with_timeout()` is called:
   - Determines the tool's timeout category via `registry.timeout_category_for(name)`
   - Retrieves the configured ceiling for that category
   - Falls back to `DEFAULT_TOOL_TIMEOUT` if no policy exists

2. The timeout is enforced via `tokio::time::timeout()`

3. If timeout is exceeded:
   - Tool is cancelled
   - Warning messages displayed
   - Proper error reported with timeout category information

## Benefits

✅ **Configurable Timeouts**: Each tool type can have different limits
✅ **Policy-Based**: Uses the tool registry's timeout policy
✅ **Category-Aware**: Respects timeout categories (Default, PTY, MCP)
✅ **Graceful Fallback**: Uses sensible defaults if no policy configured
✅ **Better Warnings**: Warning messages now show actual timeout limits
✅ **Backward Compatible**: Default 300s timeout maintained for tools without specific config

## Testing

The implementation:
- ✅ Compiles without errors
- ✅ Maintains existing test structure
- ✅ Updates test comments to reflect dynamic timeout usage
- ✅ Integrates with existing timeout policy validation
