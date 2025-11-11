# Handling Long-Running Commands

## Problem (Solved)
Commands like `cargo check` take time to complete. Previously, agents would get confused trying to poll manually.

## Solution: Automatic Backend Polling + Status Field

The backend now automatically polls for completion and returns a clear `status` field:
```json
{
  "status": "completed",  // or "running"
  "code": 0,              // Exit code (only present if completed)
  "output": "...",        // Partial or full output
  "session_id": null      // null if completed, session_id if running
}
```

## For Agents: How to Handle Long-Running Commands

### Step 1: Call run_terminal_cmd (cargo check, etc.)
```
run_terminal_cmd(command="cargo check", timeout_secs=300)
```

### Step 2: Interpret the Response
- **If `status: "completed"`** → Command finished. Check `code` field:
  - `code: 0` → Success
  - `code: 1+` → Error
  - Use the final output
  
- **If `status: "running"`** → Command still executing (backend continues polling):
  - Do NOT call `read_pty_session` again
  - Do NOT keep polling manually
  - Inform user: "Cargo check is running... (still processing)"
  - Move on to next task

### Step 3: Trust the Backend
The backend automatically:
- Waits up to 5 seconds for command completion
- Polls every 50ms for exit status
- Returns partial output if timeout reached
- Continues running the command (doesn't kill it)
- Handles cleanup

## Why NOT Manual Polling?

❌ **Old approach (manual polling loop):**
```
1. Call run_terminal_cmd
2. Get response with session_id (status: "running")
3. Agent loops: "Should I read again? Do I keep calling read_pty_session?"
4. Confusion → infinite attempts to manually poll
```

✓ **New approach (trust the backend):**
```
1. Call run_terminal_cmd
2. Get response with status field
3. If running → inform user and continue
4. User gets feedback about progress without agent confusion
5. Command completes in background (transparent to agent)
```

## Implementation Details (For Backend Developers)

The automatic polling happens in `vtcode-core/src/tools/registry/executors.rs`:

**Function**: `collect_ephemeral_session_output()`
- Polls every 50ms for exit status
- Waits up to 5 seconds for completion
- Drains PTY buffer after each read (prevents duplicates)
- Returns early after 2 seconds if output available (allows progress updates)
- Returns both partial and final output depending on completion

**Response building**: `build_ephemeral_pty_response()`
- Adds `status` field: `"completed"` or `"running"`
- Sets `code` field only if `completed`
- Sets `session_id` only if `running`

## Common Pitfalls (For Agents)

❌ **Don't:** See `status: "running"` and keep calling read_pty_session
```
Agent gets: { "status": "running", "session_id": "run-xxx" }
Agent thinks: "I should read from this session again"
Result: Confused agent polling loop
```

✓ **Do:** Trust the response and move on
```
Agent gets: { "status": "running", "session_id": "run-xxx" }
Agent thinks: "Command is running; backend handles it. Show user progress."
Result: Smooth experience, no agent confusion
```

## When to Use PTY Sessions Directly

Only use `create_pty_session → send_pty_input → read_pty_session → close_pty_session` when:
- You need **interactive** input/output (e.g., responding to prompts)
- You need **fine-grained control** over when to read/send
- You're building a persistent shell session

For running commands that just need their full output, **always use** `run_terminal_cmd` first.
