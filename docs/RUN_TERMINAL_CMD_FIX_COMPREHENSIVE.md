# Comprehensive Fix: run_terminal_cmd Parameter Handling

## Problem
The `run_terminal_cmd` tool was repeatedly failing with error: `run_terminal_cmd requires a 'command' array`, causing:
1. Initial tool call failure with validation error
2. Infinite retry loops as the agent would keep calling the same tool with the same malformed parameters
3. Loss of other parameters (cwd, timeout, etc.) during conversion

## Root Causes

### 1. Incorrect Parameter Format Conversion
**Location**: `src/agent/runloop/text_tools.rs` - `convert_harmony_args_to_tool_format()`

Command strings from LLM tools (Bash, container.exec) were being incorrectly wrapped:
```rust
// WRONG - wrapping string as single-element array:
"command": ["cargo fmt"]  

// CORRECT - pass string directly for the handler to parse:
"command": "cargo fmt"
```

### 2. Incomplete Parameter Handling
The function only handled `cmd` parameter, not `command` parameter:
- If LLM tool called with `command` instead of `cmd`, the function returned unmodified `parsed` object
- This resulted in missing `command` key entirely
- No error message was provided, just a confusing validation failure downstream

### 3. Loss of Secondary Parameters
When converting, other important parameters were lost:
- `cwd` (working directory)
- `timeout` / `timeout_secs`
- `tty` / `mode` flags
- Other tool-specific options

### 4. Poor Error Handling
Validation errors were returning the malformed data along with error message:
```rust
// WRONG:
{
    "command": [],  // or malformed data
    "_validation_error": "command executable cannot be empty"
}

// CORRECT: Return only error message without bad data:
{
    "_validation_error": "command executable cannot be empty"
}
```

## Solution

### Change 1: Enhanced Parameter Conversion
**File**: `src/agent/runloop/text_tools.rs` - `convert_harmony_args_to_tool_format()`

Improvements:
1. **Preserve all parameters** - Copy non-command parameters from original object to result
2. **Multiple parameter sources** - Try `cmd` (array/string), then fallback to `command` (array/string)
3. **Proper validation** - Return clean error without malformed data
4. **Explicit error for missing command** - Clear message if no command parameter exists

```rust
// Preserve other parameters
if let Some(map) = parsed.as_object() {
    for (key, value) in map {
        if key != "cmd" && key != "command" {
            result.insert(key.to_string(), value.clone());
        }
    }
}

// Try multiple parameter sources
if let Some(cmd) = parsed.get("cmd").and_then(|v| v.as_array()) {
    // Handle cmd array
} else if let Some(cmd_str) = parsed.get("cmd").and_then(|v| v.as_str()) {
    // Handle cmd string
} else if let Some(cmd) = parsed.get("command").and_then(|v| v.as_array()) {
    // Handle command array
} else if let Some(cmd_str) = parsed.get("command").and_then(|v| v.as_str()) {
    // Handle command string
} else {
    // Return explicit error
}
```

### Change 2: Early Validation Error Detection
**File 1**: `vtcode-core/src/tools/registry/legacy.rs` - `run_terminal_cmd()` function

**File 2**: `vtcode-core/src/tools/registry/executors.rs` - `TerminalCommandPayload::parse()` function

Added early checks for `_validation_error` field:
```rust
if let Some(err_msg) = args.get("_validation_error").and_then(|v| v.as_str()) {
    return Err(anyhow!("{}", err_msg));
}
```

This ensures:
- Validation errors from parameter conversion are caught immediately
- Clear, actionable error messages are returned to the agent
- The agent receives distinct errors that don't encourage infinite retries

## Parameter Flow After Fix

```
LLM Tool Call (bash/container.exec/exec)
    ↓
parse_tool_name_from_reference()
    → Identifies "run_terminal_cmd"
    ↓
convert_harmony_args_to_tool_format()
    ✓ Try "cmd" parameter (array or string)
    ✓ Fallback to "command" parameter (array or string)
    ✓ Preserve other parameters (cwd, timeout, mode, etc.)
    ✓ Return clean error if command missing
    ↓
Validation Check in Handlers
    ✓ Check for _validation_error field
    ✓ Return early with clear error message
    ↓
Either:
    → Error case: Proper error returned (no infinite loops)
    → Success case: run_terminal_cmd handler processes command
        → Parses string to array if needed
        → Validates command
        → Executes with all preserved parameters
```

## Impact

### Before Fix
- Tool calls with string commands failed with cryptic error
- No parameters (cwd, timeout) preserved during conversion
- Infinite retry loops possible due to validation errors
- Confusing "requires a 'command' array" message when parameter was actually present

### After Fix
- All parameter variations handled (cmd/command, array/string)
- Secondary parameters (cwd, timeout, mode) preserved
- Clear validation errors prevent infinite loops
- Tool calls succeed with proper parameter handling

## Files Modified
1. `src/agent/runloop/text_tools.rs` - Improved parameter conversion logic
2. `vtcode-core/src/tools/registry/legacy.rs` - Early validation error detection
3. `vtcode-core/src/tools/registry/executors.rs` - Early validation error detection

## Testing
- `cargo check` - Passes
- `cargo fmt` - Passes
- No compilation warnings
