# Quick Reference: Agent Command Execution Guide

## When Running Commands, Always Use `run_terminal_cmd`

```
run_terminal_cmd(command="cargo check", timeout_secs=300)
```

## Response Format

All responses include a `status` field:

```json
{
  "status": "completed",  // or "running"
  "code": 0,              // Exit code (only if completed)
  "output": "...",        // Output from command
  "session_id": null      // Session ID (only if running)
}
```

## Interpretation Logic

```
if response.status == "completed":
    // Command finished
    if response.code == 0:
        // Success
        use response.output
    else:
        // Error (exit code 1+)
        show error with response.output
else if response.status == "running":
    // Command still executing (long-running command)
    inform user: "Command is running... [partial output]"
    do NOT call read_pty_session
    move on to next task
```

## Key Points

| Scenario | What to Do | What NOT to Do |
|----------|-----------|-----------------|
| `status: "completed"` | Use the output and check `code` | - |
| `status: "running"` | Inform user and move on | ❌ Don't poll with read_pty_session |
| See `session_id` in response | ✓ It's there for UI progress tracking | ❌ Don't use it for manual polling |
| Command output continues in background | ✓ Let backend handle it | ❌ Don't make repeated calls |

## Example: Cargo Check

```
Agent calls:
  run_terminal_cmd(command="cargo check", timeout_secs=300)

Response arrives in ~5 seconds:
  {
    "status": "running",
    "output": "Checking vtcode-core v0.43.4...",
    "session_id": "run-xyz"
  }

Agent should:
  ✓ Print: "Cargo check started. This may take a few minutes..."
  ✓ Move on to next task
  ✓ Trust backend continues polling

After 2 minutes of background polling:
  Backend eventually completes, UI updates user

Agent should NOT:
  ❌ Keep calling read_pty_session
  ❌ Get stuck in a polling loop
  ❌ Ask "Is it done yet?"
```

## The Rule

**Trust the `status` field. Never manually poll.**

If `status: "completed"` → You have the full output.
If `status: "running"` → Backend handles the rest. Move on.

---

**Related**: See `docs/long-running-commands.md` for detailed explanation.
