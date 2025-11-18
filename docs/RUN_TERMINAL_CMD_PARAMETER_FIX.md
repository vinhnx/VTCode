# run_terminal_cmd Parameter Format Fix

## Issue
The `run_terminal_cmd` tool was failing with error: `run_terminal_cmd requires a 'command' array` when receiving command strings from LLM tools like Bash or container.exec.

## Root Cause
In `src/agent/runloop/text_tools.rs`, the `convert_harmony_args_to_tool_format()` function was incorrectly wrapping command strings as single-element arrays:

```rust
// WRONG:
serde_json::json!({
    "command": [cmd_str]  // Wrapping string as array element
})

// CORRECT:
serde_json::json!({
    "command": cmd_str    // Pass string directly
})
```

The `run_terminal_cmd` handler in `vtcode-core/src/tools/registry/legacy.rs` (lines 143-151) properly handles command string parsing and conversion to array format. By wrapping the string as `["string"]`, we were creating invalid input like `command: ["cargo fmt"]` instead of `command: "cargo fmt"`.

## Fix Applied
Changed `convert_harmony_args_to_tool_format()` to pass command strings directly to `run_terminal_cmd` instead of wrapping them in an array. The legacy handler will:

1. Detect the command is a string (line 143)
2. Split it into parts using `split()` function (line 144)
3. Convert to JSON array (line 149)
4. Proceed with execution

## Files Changed
- `src/agent/runloop/text_tools.rs` - Lines 478-489

## Testing
- `cargo check` passes
- `cargo fmt` validation passes
- Error should no longer appear when using Bash/container.exec tools with command strings
