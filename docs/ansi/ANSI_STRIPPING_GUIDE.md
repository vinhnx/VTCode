# ANSI Code Stripping in Tool Output

## Overview

ANSI escape sequences (color codes, text styling) in tool command output are automatically stripped to prevent rendering issues in the terminal UI. This feature is **enabled by default**.

## Configuration

### Default Behavior

-   **Status**: Enabled
-   **Config Option**: `ui.allow_tool_ansi`
-   **Default Value**: `false` (ANSI codes are stripped)

### How to Configure

In `vtcode.toml`:

```toml
[ui]
# Remove ANSI codes from tool output (recommended)
allow_tool_ansi = false  # Default - strips all ANSI escape sequences

# Enable ANSI codes in tool output (may cause display issues)
# allow_tool_ansi = true   # Not recommended - preserves colors but can break layout
```

## What Gets Stripped

When `allow_tool_ansi = false`, the following ANSI sequences are removed:

### CSI (Control Sequence Introducer)

-   Format: `ESC [ ... letter`
-   Examples: `\x1b[31m` (red), `\x1b[1;33m` (bold yellow), `\x1b[0m` (reset)
-   Used by: `cargo check`, `git`, `grep`, `ls`, most CLI tools

### OSC (Operating System Command)

-   Format: `ESC ] ... ST` (where ST = ESC \ or BEL)
-   Examples: Hyperlinks, terminal title changes

### Character Set Designations

-   Format: `ESC ( X`, `ESC ) X`, etc.
-   Used by: Legacy terminals for font selection

### Single Character Sequences

-   Format: `ESC X`
-   Examples: Cursor save/restore, reset, VT52 mode

## Examples

### Before (with ANSI codes)

```
warning: function check_prompt_reference_trigger is never used
--> vtcode-core/src/ui/tui/session/command.rs:321:15
```

(The function name and path would be colored yellow/orange and red respectively)

### After (stripped)

```
warning: function check_prompt_reference_trigger is never used
--> vtcode-core/src/ui/tui/session/command.rs:321:15
```

(Plain text, no colors)

## Affected Tools

ANSI stripping applies to:

-   `run_pty_cmd` - PTY terminal commands (cargo check, cargo build, etc.)
-   `read_pty_session` - Reading PTY session output
-   `list_pty_sessions` - PTY session management
-   `send_pty_input` - PTY input handling
-   All tools that output through PTY sessions

## Implementation Details

### Strip Function

Location: `src/agent/runloop/tool_output/streams.rs:516`

The `strip_ansi_codes()` function:

1. Detects ANSI escape sequences by looking for `\x1b` (ESC) character
2. Parses the escape sequence type
3. Skips the entire sequence without adding it to output
4. Preserves all other text as-is

### Performance

-   **Fast path**: If no `\x1b` found, returns borrowed input (zero-copy)
-   **Slow path**: If ANSI codes present, allocates new string with codes removed

### Token Budget

After ANSI stripping, tool output is subject to token-based truncation:

-   Applies when rendering to LLM context
-   Controlled by `context.model_input_token_budget` (default: 25000 tokens)
-   Uses head+tail strategy to preserve important output sections

## Troubleshooting

### Colors Still Appearing

If colors are still visible despite `allow_tool_ansi = false`:

1. Verify config is loaded:

    ```bash
    vtcode config show ui.allow_tool_ansi
    ```

2. Check if environment overrides config:

    ```bash
    echo $CLICOLOR
    echo $CLICOLOR_FORCE
    unset CLICOLOR
    unset CLICOLOR_FORCE
    ```

3. Rebuild vtcode to ensure latest version:
    ```bash
    cargo build --release
    ```

### Want Colors Back?

Set in `vtcode.toml`:

```toml
[ui]
allow_tool_ansi = true
```

**Warning**: This may cause rendering issues in the inline UI, including:

-   Misaligned text
-   Layout artifacts
-   Color codes appearing in output
-   Broken terminal wrapping

Only enable if using VT Code in a context that properly handles ANSI codes.

## Test Coverage

The ANSI stripping function is comprehensively tested with 16 test cases covering:

**Basic Cases**

-   No ANSI codes (zero-copy fast path)
-   Simple color codes (most common)
-   Multiple sequential codes

    **Real-World Scenarios**

-   `cargo check` output patterns
-   Rust compiler warnings/errors
-   Unicode + ANSI combination
-   Git/ls colored output

    **Edge Cases**

-   256-color mode (38;5;N)
-   True color/24-bit RGB (38;2;R;G;B)
-   Cursor movement (CUU, CUD, etc.)
-   Clear screen commands
-   OSC hyperlinks
-   Incomplete sequences at end
-   Empty strings
-   Only ANSI codes (no text)
-   Newlines preservation

Run tests:

```bash
cargo test --bin vtcode tool_output::streams::ansi_stripping_tests
```

All tests consistently pass, validating the robustness of the implementation.

## Related Settings

-   `ui.tool_output_mode`: "compact" or "full" (different output verbosity)
-   `ui.tool_output_max_lines`: Maximum lines to display (50 default)
-   `context.model_input_token_budget`: Token limit for tool outputs in LLM context
