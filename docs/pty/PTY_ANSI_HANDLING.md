# PTY Output ANSI Handling

## Overview

VT Code implements comprehensive ANSI code stripping for PTY command output to ensure clean, machine-readable output for agent processing.

## Architecture

### 1. Prevention at Source (Preferred)

Environment variables set in `vtcode-core/src/tools/pty.rs::set_command_environment()`:

```rust
// Disable automatic color output from build tools
builder.env("NO_COLOR", "1");                    // Universal color disable
builder.env("CLICOLOR", "0");                    // Disable ANSI for ls, etc.
builder.env("CLICOLOR_FORCE", "0");              // Don't force colors
builder.env("LS_COLORS", "");                    // No color for ls

// Rust/Cargo specific
builder.env("CARGO_TERM_COLOR", "never");        // Disable cargo colors
builder.env("RUSTFLAGS", "-C color=never");      // Disable rustc colors
```

**Benefits**: Prevents ANSI codes from being generated in the first place—most reliable approach.

### 2. Capture-Time Stripping

`PtyScrollback::push_text()` in `vtcode-core/src/tools/pty.rs:206-209`:

```rust
fn push_text(&mut self, text: &str) {
    // Strip ANSI codes from the text to prevent ANSI styling in output
    let cleaned_text = crate::utils::ansi_parser::strip_ansi(text);
    let text_bytes = cleaned_text.len();
    // ... rest of buffer management
}
```

**Triggers for every line of PTY output** via UTF-8 validation loop.

### 3. Return-Path Stripping

Multiple locations in `vtcode-core/src/tools/registry/executors.rs`:

| Function                     | Lines      | Output Field                    |
| ---------------------------- | ---------- | ------------------------------- |
| `execute_run_pty_command()`  | 3371       | `output`                        |
| `execute_read_pty_session()` | 2120       | `output`                        |
| `execute_send_pty_input()`   | 2022       | `output`                        |
| `snapshot_to_map()`          | 2911, 2920 | `screen_contents`, `scrollback` |

Example:

```rust
response.insert("output".to_string(), Value::String(strip_ansi(&output)));
```

### 4. UI Rendering Layer

`src/agent/runloop/tool_output/streams.rs::render_stream_section()`:

```rust
if !allow_ansi {
    output = strip_ansi_codes(&output); // Secondary stripping for safety
}
```

## ANSI Parser Implementation

Two complementary parsers handle edge cases:

### `crate::utils::ansi_parser::strip_ansi()` (Primary)

Handles:

-   **CSI sequences**: `ESC[...letter` (colors, styles, cursor movement)
-   **OSC sequences**: `ESC]...BEL` or `ESC]...ST` (hyperlinks, titles)
-   **DCS/PM/APC**: `ESC P/^/_...ST` (device control strings)
-   **2-char escapes**: `ESC[`, `ESC]`, etc.
-   **Incomplete sequences**: Gracefully handles partial ANSI at EOF

Tests in `vtcode-core/src/utils/ansi_parser.rs` verify:

-   Basic colors: `\x1b[31mRed\x1b[0m` → `Red`
-   Bold: `\x1b[1;32mbold\x1b[0m` → `bold`
-   Cargo output with warnings/errors
-   OSC with ST terminator: `\x1b]8;;file://\x1b\\` (hyperlinks)

### `streams.rs::strip_ansi_codes()` (Secondary)

Character-by-character parsing with:

-   Peekable iterator for lookahead
-   Proper ST (`ESC\` / `\u{0007}`) termination
-   Character set selection (`ESC(0`, `ESC)B`)
-   Single-char sequences (cursor save, reset, etc.)

## Data Flow

```
Raw PTY Output (with potential ANSI codes)
    ↓
[1] Environment vars prevent ANSI generation
    ↓
[2] PtyScrollback::push_text() strips codes
    ↓
[3] Session storage (clean text)
    ↓
[4] Tool returns output via executors.rs
    ↓
[5] Return-path strip_ansi() (belt-and-suspenders)
    ↓
[6] UI renderer (secondary safety strip)
    ↓
Clean, machine-readable output to agent
```

## Testing

### Existing Tests

-   `vtcode-core/src/utils/ansi_parser.rs`: 10+ tests covering CSI, OSC, edge cases
-   `vtcode-core/src/tools/pty.rs`: Scrollback size, overflow, metrics tests
-   Implicit: Every `strip_ansi()` call prevents malformed output

### Manual Verification

```bash
cargo check 2>&1 | od -c | grep "\\033"  # Should be empty
cargo build 2>&1 | grep -P '\x1b\['       # Should be empty
```

Both commands should produce **zero matches** after these changes.

## Performance Impact

-   **Minimal**: Single-pass linear scan over output text
-   **Token-aware truncation** already processes all output for token counting
-   **Environment variables**: No runtime cost (set once at spawn)

## Edge Cases Handled

| Case                                | Handling                                     |
| ----------------------------------- | -------------------------------------------- |
| Incomplete UTF-8 at output boundary | Replacement char `U+FFFD` + continue         |
| ANSI at token boundary              | Strip before truncation, then truncate       |
| Multiple nested styles              | All sequences stripped (not rendered anyway) |
| Hyperlinks (`ESC]8` OSC)            | Stripped (just text remains)                 |
| 256-color codes (`ESC[38;5;123m`)   | Stripped (part of CSI sequence)              |
| True color (`ESC[38;2;R;G;Bm`)      | Stripped (part of CSI sequence)              |
| Mixed line endings                  | Preserved as-is (`\r\n`, `\n`, `\r`)         |

## Related Files

-   `vtcode-core/src/tools/pty.rs` - PTY management and ANSI prevention
-   `vtcode-core/src/utils/ansi_parser.rs` - Core stripping logic
-   `vtcode-core/src/tools/registry/executors.rs` - Return-path stripping
-   `src/agent/runloop/tool_output/streams.rs` - Render-time stripping
-   `src/acp/zed.rs` - ACP integration with stripping

## Configuration

PTY output ANSI handling is **automatic and not user-configurable**. The system always:

1.  Prevents ANSI generation at command spawn
2.  Strips any escaped sequences at capture
3.  Strips again at output return
4.  Provides clean text to agent

This ensures consistency and reliability across all PTY commands.
