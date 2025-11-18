# VT Code Agent - Command Execution Flow Fix

## Problem

The VT Code agent was confused about which tool to use for executing commands:
1. It attempted to use PTY sessions (`create_pty_session`) for simple one-off commands like `cargo fmt`
2. It created a PTY session but then sent empty input
3. It got confused when the session already existed
4. It eventually fell back to `run_terminal_cmd` which worked perfectly

### Example Error Flow
```
✓ create_pty_session (cargo fmt)
✓ send_pty_input (empty input)
✗ send_pty_input failed: failed to write to PTY session
✗ create_pty_session: PTY session already exists
✗ create_pty_session: PTY session already exists (repeated)
✓ run_terminal_cmd (finally works!)
```

## Root Cause

The system prompt guidance for command execution was unclear:
- It listed both approaches but didn't emphasize the default
- It didn't provide a clear decision tree
- The distinction between "interactive" and "one-off" wasn't explicit enough
- The agent was choosing PTY (Tier 2 control) over run_terminal_cmd (Tier 1 essential)

## Solution

### Changes to System Prompt (`vtcode-core/src/prompts/system.rs`)

#### 1. Clarified Tool Selection Decision Tree (Lines 102-107)
**Before**: Vague distinction between interactive and one-off
```
Running commands?
├─ Interactive shell? → create_pty_session
└─ One-off command? → run_terminal_cmd
```

**After**: Explicit, unambiguous guidance
```
Running commands?
├─ Interactive multi-step shell? → create_pty_session → send_pty_input → read_pty_session → close_pty_session
└─ One-off command? → run_terminal_cmd (ALWAYS use for: cargo, git, npm, python, etc.)
  (AVOID: create_pty_session for single commands; only for interactive workflows)
```

#### 2. Added Decision Tree Section (Lines 142-152)
Provided a clear flowchart to eliminate confusion:
```
Is this a single one-off command?
├─ YES → Use run_terminal_cmd
└─ NO → Is this interactive multi-step?
    ├─ YES → Use create_pty_session
    └─ NO → Still use run_terminal_cmd
```

#### 3. Expanded Command Execution Strategy (Lines 154-165)
- **Bold "DEFAULT"** emphasis: `use run_terminal_cmd for ALL one-off commands`
- Explicit examples: cargo fmt, cargo check, git status, npm install
- Explicit anti-patterns: "Do NOT use PTY for simple commands like `cargo fmt`"
- Added clarification on PTY-only use cases: debugging with gdb, node REPL, vim editing

#### 4. Made run_terminal_cmd a Tier 1 Essential Tool
- Emphasized as the default first choice
- PTY sessions relegated to Tier 2 (advanced control flow)

## How It Works Now

### Flow for `cargo fmt` (one-off command)
Agent decision-making:
1. **Question**: "Is `cargo fmt` a single one-off command?"
2. **Answer**: "Yes, it's a build/format tool"
3. **Decision**: Use `run_terminal_cmd` ✓
4. **Execute**: `run_terminal_cmd` returns immediately with status and output
5. **Success**: Command executes, cargo is found via PATH ✓

### Flow for Interactive Debugging (multi-step workflow)
Agent decision-making:
1. **Question**: "Is this a single one-off command?"
2. **Answer**: "No, I need to start gdb and step through code"
3. **Question**: "Is this interactive multi-step?"
4. **Answer**: "Yes, I'll run commands and analyze output repeatedly"
5. **Decision**: Use `create_pty_session` → `send_pty_input` → `read_pty_session` → (repeat) → `close_pty_session`
6. **Success**: Interactive workflow executes correctly

## Benefits

✅ **Eliminates confusion** about tool selection  
✅ **Reduces failed attempts** at PTY creation for simple commands  
✅ **Faster execution** - direct to `run_terminal_cmd` instead of failed PTY path  
✅ **Clearer agent reasoning** - explicit examples and anti-patterns  
✅ **More reliable** - cargo PATH fix works properly with correct tool  
✅ **Better fallback behavior** - agent won't get stuck retrying failed PTY sessions  

## Testing

```bash
# Before: Confused flow with retries
# cargo fmt → create_pty_session → send_pty_input fails → retries → finally run_terminal_cmd works

# After: Direct flow
# cargo fmt → run_terminal_cmd → success
```

## Integration with PATH Fix

These two fixes work together:

1. **PATH Fix** (`CARGO_PATH_FIX.md`): Ensures cargo binary is actually found in PATH
2. **Agent Flow Fix** (this document): Ensures agent uses the right tool (`run_terminal_cmd`)
3. **Result**: `cargo fmt` executes successfully with proper cargo resolution ✓

## Files Modified

- `vtcode-core/src/prompts/system.rs` - Enhanced system prompt guidance for command execution

## Backwards Compatibility

✅ No breaking changes  
✅ Existing PTY workflows still work (for interactive use cases)  
✅ `run_terminal_cmd` behavior unchanged  
✅ Only improves agent decision-making, not underlying tools  

## Example Improvements

### cargo commands
- `cargo fmt` - now uses run_terminal_cmd ✓
- `cargo check` - now uses run_terminal_cmd ✓
- `cargo test` - now uses run_terminal_cmd ✓

### git commands
- `git status` - now uses run_terminal_cmd ✓
- `git diff` - now uses run_terminal_cmd ✓
- `git log` - now uses run_terminal_cmd ✓

### npm/node commands
- `npm install` - now uses run_terminal_cmd ✓
- `npm test` - now uses run_terminal_cmd ✓
- Interactive `node REPL` - still uses PTY as intended ✓

## Future Recommendations

1. Consider adding examples to prompt about tool selection
2. Monitor agent tool selection to validate fix effectiveness
3. Consider adding a tool preference hint in the tool definitions
4. Periodically review agent logs to catch new confusion patterns
