# Confirm Usage for Destructive Commands

When interacting with the agent using `run_pty_cmd` or PTY-backed tools, certain commands that can change history or delete files require an explicit confirmation. This is to protect users and avoid accidental destructive actions.

How to confirm a destructive operation:

-   CLI example via JSON tool call:

```json
{
    "command": ["git", "reset", "--hard"],
    "working_dir": ".",
    "confirm": true
}
```

-   Programmatically via `EnhancedTerminalInput` (Rust):

```rust
EnhancedTerminalInput {
    command: vec!["git".to_string(), "reset".to_string(), "--hard".to_string()],
    working_dir: None,
    timeout_secs: Some(300),
    confirm: Some(true),
    ..Default::default()
}
```

Agent behavior:

-   If `confirm` is not provided, the agent will refuse to execute destructive commands.
-   If `confirm=true`, the agent logs a `PermissionEvent` in `~/.vtcode/audit/permissions-YYYY-MM-DD.log` that includes the command and the decision (Allowed).
-   The agent will perform a dry-run or pre-flight checks by default (e.g., `git status`, `git diff`) before confirming.
