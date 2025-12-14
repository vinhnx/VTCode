# Changes Summary: run_pty_cmd Parameter Handling Fix

## What Was Fixed

Fixed repeated `run_pty_cmd requires a 'command' array` errors and infinite retry loops when LLM tools (Bash, container.exec) call `run_pty_cmd`.

## Changes Made

### 1. src/agent/runloop/text_tools.rs
**Function**: `convert_harmony_args_to_tool_format()`

  **Preserve other parameters** - Copy cwd, timeout, mode, etc. from original to converted args
  **Handle multiple parameter sources** - Try cmd (array/string) → fallback to command (array/string)  
  **Clean error handling** - Return only error message, not malformed data
  **Explicit missing command error** - Clear message if no command parameter exists

### 2. vtcode-core/src/tools/registry/legacy.rs
**Function**: `run_pty_cmd()`

  **Early validation error check** - Detect `_validation_error` field from parameter conversion
  **Immediate error return** - Prevents cascading failures and retries

### 3. vtcode-core/src/tools/registry/executors.rs
**Function**: `TerminalCommandPayload::parse()`

  **Early validation error check** - Same as legacy.rs for consistency
  **Clear error propagation** - Validation errors surface immediately

## Why This Fixes Infinite Loops

**Before:**
- Parameter conversion fails silently
- Tool gets called with invalid parameters
- Tool fails with cryptic error
- Agent retries with same invalid parameters
- Loop repeats indefinitely

**After:**
- Parameter conversion validates and returns clear error
- Error is caught immediately by handlers
- Clear error message is returned to agent
- Agent receives distinct error, not encouraged to retry
- If retry needed, parameters are correctly formatted

## Testing Status
-   cargo check - No errors
-   cargo fmt - Formatting valid
-   Compilation - Success
