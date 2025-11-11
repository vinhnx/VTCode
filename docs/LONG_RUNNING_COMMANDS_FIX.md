# Fix: Handling Long-Running Commands Properly

## Problem

When running long-running commands like `cargo check` via `run_terminal_cmd`, the agent would:
1. Call `run_terminal_cmd` 
2. Get back a response with `session_id` (because the 5-second timeout expired)
3. Get confused and think "I should keep reading from this session"
4. Repeatedly call `read_pty_session` in a confused polling loop
5. Show confusing "Thinking:" messages indicating repeated/stuck reasoning

**Root cause**: The response didn't clearly indicate whether the command was still running or had completed, causing the agent to misinterpret the `session_id` as a signal to poll manually.

## Solution

### 1. Added `status` Field to Response

**File**: `vtcode-core/src/tools/registry/executors.rs:2347-2387`

The PTY command response now includes an explicit `status` field:

```json
{
  "success": true,
  "command": ["cargo", "check"],
  "output": "...",
  "code": 0,
  "status": "completed",  // NEW: "completed" or "running"
  "mode": "pty",
  "session_id": null,  // null if completed
  ...
}
```

**When command completes**:
- `status: "completed"`
- `code: 0` (or exit code)
- `session_id: null`

**When command is still running** (timeout exceeded):
- `status: "running"`
- `code: null`
- `session_id: "run-abc123"` (for UI progress tracking, NOT for agent polling)

### 2. Updated System Prompt

**File**: `vtcode-core/src/prompts/system.rs:119-129`

Added clear guidance:

```markdown
**Command Execution Strategy**:
- One-off commands → run_terminal_cmd (for git, cargo, python, npm, node, etc.)
  - Response contains `"status": "completed"` or `"status": "running"`
  - If status is `"completed"` → command finished; use the `code` field (0=success, 1+=error) and output
  - If status is `"running"` → command is still executing (long-running like cargo check); backend continues polling automatically; DO NOT call read_pty_session; just inform user and move on
  - The backend waits up to 5 seconds internally; longer commands will return "running" status with partial output
  - ⚠️ IMPORTANT: Do NOT keep polling manually or call read_pty_session if you see session_id; the backend handles it
```

### 3. Updated Documentation

**File**: `docs/long-running-commands.md`

Complete rewrite to explain:
- The new automatic backend polling mechanism
- How agents should interpret the `status` field
- What **NOT** to do (manual polling)
- Why trust the backend is better

## How It Works Now

### For Agents

```
1. Call: run_terminal_cmd(command="cargo check", timeout_secs=300)

2. Receive response with status field:
   - status: "completed" → Use code + output, done
   - status: "running" → Inform user ("Still processing..."), move on
   
3. Trust that backend handles the rest automatically
   - Command continues running even if agent moves on
   - User sees progress in UI
   - No agent confusion
```

### For Backend

The automatic polling was already there in `collect_ephemeral_session_output()`:
- Polls every 50ms for exit status
- Waits up to 5 seconds
- Drains PTY buffer after each read
- Returns early after 2 seconds if output available

**No changes needed in the polling logic** — just added the `status` field to make the behavior explicit to agents.

## Testing

```bash
# Test that it compiles
cargo check --lib

# Test with a long-running command
vtcode
> run cargo check
# Should see: status: "completed" or "running" depending on how fast your PC is
```

## Impact

- ✅ Agents no longer get confused about long-running commands
- ✅ Clear distinction between "still running" and "done"
- ✅ No more infinite polling loops in agent reasoning
- ✅ Better user feedback (progress shown without agent being confused)
- ✅ No breaking changes (backward compatible — `code` field still there)

## Files Changed

1. `vtcode-core/src/tools/registry/executors.rs` — Added `status` field
2. `vtcode-core/src/prompts/system.rs` — Updated guidance  
3. `docs/long-running-commands.md` — Rewritten documentation
4. `docs/LONG_RUNNING_COMMANDS_FIX.md` — This file
