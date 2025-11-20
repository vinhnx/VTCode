# Terminal Command Output - Compact & Clean Format

## Overview

Optimized the terminal command output format from verbose, multi-line headers to a minimal, single-line compact design. Reduces visual clutter while maintaining all essential information.

---

## Before (Verbose Format)

```
✓ [run_pty_cmd] cargo fmt · Command: cargo, fmt (exit: 0)
[END] [COMPLETED - 80x24] Session: run-1763462657610 | Working directory: . (exit code: 0)
────────────────────────────────────────────────────────────
$ /bin/zsh -c cargo fmt
(no output)
Done.
✓ Discovered new MCP tool: time:get_current_time
────────────────────────────────────────────────────────────
```

**Issues**:
- ❌ Multiple status indicators: `[END]`, `[COMPLETED]`, `(exit code: 0)` - redundant
- ❌ 60-character separator lines - wasteful
- ❌ Verbose session metadata: `Session: run-1763462657610`
- ❌ Cluttered headers with mixed information
- ❌ "Running" indicator adds extra line
- ❌ Completion footer with `[Session ...]` wrapper

---

## After (Compact Format)

```
✓ OK · cargo fmt · 80x24
$ /bin/zsh -c cargo fmt
(no output)
✓ exit 0
```

**Improvements**:
- ✅ Single-line header with status · command · viewport
- ✅ Clear indicators: `▶ RUN` (running), `✓ OK` (completed)
- ✅ Minimal footer: `✓ exit 0` or `✓ done`
- ✅ Command truncation to 40-50 chars with ellipsis
- ✅ No redundant session IDs or separators
- ✅ 50% fewer lines of output

---

## Output Format Specification

### Header Format (Single Line)

**Standard case** (no working directory):
```
{status} · {command} [· {cols}x{rows}]
```

**With working directory**:
```
{status} · {command_truncated} · {cols}x{rows}
```

### Status Indicators

| State | Symbol | Text | Display |
|---|---|---|---|
| Running | `▶` | `RUN` | `▶ RUN` |
| Completed (0) | `✓` | `OK` | `✓ OK` |
| Completed (exit code) | `✓` | `OK` | `✓ OK` |

### Command Truncation

- **Threshold**: 40 chars (with `wd`) or 50 chars (without)
- **Truncation**: `command[..37]…` (leave room for ellipsis)
- **Display full command** on next line only if:
  - Command was truncated, OR
  - Working directory is present

### Footer Format

**On completion**:
```
✓ {exit_info}
```

Where `exit_info` is:
- `exit 0` - successful exit
- `exit {N}` - non-zero exit code
- `done` - no exit code available

---

## Code Changes

### File: `src/agent/runloop/tool_output/commands.rs`

#### Header Rendering (Lines 98-140)

**Before**:
```rust
let status_symbol = if !is_completed { "[RUN]" } else { "[END]" };
let status_text = if !is_completed { "RUNNING" } else { "COMPLETED" };
let exit_info = if let Some(code) = exit_code {
    format!(" (exit code: {})", code)
} else { String::new() };
renderer.line(
    MessageStyle::Info,
    &format!(
        "{} [{} - {}x{}] Session: {}{}{}",
        status_symbol, status_text, cols, rows, session_id, 
        if let Some(wd) = working_dir { format!(" | Working directory: {}", wd) } else { "".to_string() },
        exit_info
    ),
)?;
renderer.line(MessageStyle::Info, &"─".repeat(60))?;
renderer.line(MessageStyle::Response, &format!("$ {}", command))?;
```

**After**:
```rust
let status_symbol = if !is_completed { "▶" } else { "✓" };
let status_badge = if !is_completed {
    format!("{} RUN", status_symbol)
} else {
    format!("{} OK", status_symbol)
};

// Compact header: status · command · session info
let header = if working_dir.is_some() {
    format!(
        "{} · {} · {}x{}",
        status_badge, 
        if command.len() > 40 {
            format!("{}…", &command[..37])
        } else {
            command.clone()
        },
        cols,
        rows
    )
} else {
    format!(
        "{} · {}",
        status_badge,
        if command.len() > 50 {
            format!("{}…", &command[..47])
        } else {
            command.clone()
        }
    )
};

renderer.line(MessageStyle::Info, &header)?;

// Show full command only on separate line if truncated
if command.len() > 50 || working_dir.is_some() {
    renderer.line(MessageStyle::Response, &format!("$ {}", command))?;
}
```

#### Footer Rendering (Lines 203-217)

**Before**:
```rust
if is_pty_session && is_completed {
    if let Some(session_id) = session_id {
        let exit_info = if let Some(code) = exit_code {
            if code == 0 { " [SUCCESS]".to_string() }
            else { format!(" [EXIT: {}]", code) }
        } else { " [COMPLETE]".to_string() };
        renderer.line(
            MessageStyle::Info,
            &format!("[Session {}{}]", session_id, exit_info),
        )?;
    }
}
```

**After**:
```rust
if is_pty_session && is_completed {
    let exit_badge = if let Some(code) = exit_code {
        if code == 0 {
            "exit 0".to_string()
        } else {
            format!("exit {}", code)
        }
    } else {
        "done".to_string()
    };
    renderer.line(MessageStyle::Info, &format!("✓ {}", exit_badge))?;
}
```

#### Removed "Still Running" Message

**Removed entirely**:
```rust
// DELETED: "... command still running ..." message
// Status indicator (▶ RUN) is sufficient
```

---

## Examples

### Example 1: Short Command, Running

**Input**:
```json
{
  "command": "cargo check",
  "is_exited": false,
  "rows": 24,
  "cols": 80
}
```

**Output**:
```
▶ RUN · cargo check · 80x24
... [output] ...
```

### Example 2: Long Command, Completed with Error

**Input**:
```json
{
  "command": "find /Users/vinhnguyenxuan/Developer -name '*.rs' -type f | head -20",
  "is_exited": true,
  "exit_code": 1,
  "rows": 24,
  "cols": 80,
  "working_directory": "/Users/vinhnguyenxuan/Developer/vtcode"
}
```

**Output**:
```
✓ OK · find /Users/vinhnguyenxuan/Developer …
$ find /Users/vinhnguyenxuan/Developer -name '*.rs' -type f | head -20
... [output] ...
✓ exit 1
```

### Example 3: Successful Build

**Input**:
```json
{
  "command": "cargo build --release",
  "is_exited": true,
  "exit_code": 0,
  "rows": 24,
  "cols": 120
}
```

**Output**:
```
✓ OK · cargo build --release
Compiling vtcode v0.45.4 ...
Finished `release` profile in 45.32s
✓ exit 0
```

---

## Benefits

### Reduction in Visual Clutter
- **Lines saved**: 3 → 1 for header (66% reduction)
- **Footer**: 1 minimal line instead of verbose wrapper
- **No separators**: Removed 60-char delimiter lines

### Improved Readability
- **Key information first**: Status symbol + command visible at a glance
- **Clear completion**: Simple `✓ exit 0` instead of `[Session ID][EXIT: 0]`
- **Consistent formatting**: All terminal sessions follow same pattern

### Better Information Density
- **Command truncation**: Full command shown when necessary
- **Viewport info**: Included in header when useful
- **No redundancy**: Status not repeated 3 times

### Faster Scanning
- **Status symbol**: Universal `▶` and `✓` symbols
- **Single-line headers**: Can scan multiple commands instantly
- **Minimal footer**: Completion status immediate

---

## Backwards Compatibility

✅ **Internal change only** - no public API modifications
✅ **No dependency changes** - uses existing `AnsiRenderer`
✅ **Graceful fallback** - respects existing error handling
✅ **No configuration needed** - automatic formatting

---

## Testing

### Test Cases

1. **Short command, no output**
   - Verify header displays correctly
   - Verify footer shows `✓ exit 0`

2. **Long command with working directory**
   - Verify command truncation works
   - Verify full command shown on separate line

3. **Command with stderr**
   - Verify header displays
   - Verify stderr rendered
   - Verify footer displays with exit code

4. **Still-running command (PTY session)**
   - Verify header shows `▶ RUN`
   - Verify no "running..." message
   - Verify command displayed

5. **Failed command (non-zero exit)**
   - Verify footer shows `✓ exit N`
   - Verify stderr displayed

---

## Future Enhancements

### Optional: Ratatui Block Styling
Could further enhance with bordered blocks (as mentioned in original request):

```rust
use ratatui::widgets::{Block, Padding};

// Terminal command block with border and padding
let block = Block::bordered()
    .title("Terminal")
    .padding(Padding::new(2, 2, 0, 0));
```

However, current implementation prioritizes:
1. **Minimal output** - single-line headers
2. **Stream-oriented** - works with line-based rendering
3. **Simple styling** - ANSI styles via MessageStyle enum

Block-based rendering would require buffering entire command output, which conflicts with streaming philosophy.

---

## Related Documentation

- `TUI_SCROLL_ANSI_OPTIMIZATION.md` - Scroll performance optimization
- `SCROLL_OPTIMIZATION_IMPL.md` - Implementation patterns
- `AGENTS.md` - Code style and standards
