# Exec Mode Automation

VT Code's exec mode lets you run autonomous tasks from the shell without starting the interactive TUI. The workflow mirrors the
`codex exec` guidance: commands run non-interactively, stream structured events, and can be resumed later. By default, `vtcode
exec` prints progress to stderr while the final agent message is written to stdout, so you can safely pipe the result into other
scripts without filtering intermediary logs.

## Launching exec mode

```bash
vtcode exec "count the total number of lines of code in this project"
```

Exec mode enforces the same workspace trust checks as the interactive UI. The workspace must be marked as `full_auto`, and the
configured `automation.full_auto` section must enable autonomous execution. When the command starts it automatically activates the
full-auto tool allow list and assumes no human is present to grant additional approvals. The agent will not prompt for user input
allowances or confirmation dialogs, so ensure the allow list covers every tool it may need. You can capture the final summary separately with
`--last-message-file` or persist the raw JSON stream with `--events`. Use `--json` when you want to see the live JSONL feed on
stdout.

## Structured event stream

Use `vtcode exec --json` to emit JSON Lines that match the non-interactive Codex schema:

- `thread.started` – emitted once per session with a stable `thread_id`.
- `turn.started` / `turn.completed` – wrap each autonomous turn and include token usage on completion.
- `turn.failed` – surfaces limit breaches or early exits together with the outcome description.
- `item.started` / `item.updated` / `item.completed` – track the lifecycle of individual operations.

Supported item payloads:

- `agent_message` – final or intermediate assistant responses.
- `reasoning` – streamed model thoughts captured from providers that expose them.
- `command_execution` – shell or tool invocations with `status`, `aggregated_output`, and `exit_code` fields.
- `file_change` – applied patches grouped by path and change type.
- `mcp_tool_call` – Model Context Protocol tool runs with tool name, arguments, and status.
- `web_search` – provider search calls including the query and optional summary.

These events align with the published Codex non-interactive schema so downstream automation, dashboards, or log shippers can reuse
existing parsers without modification.

## Resuming sessions

Exec runs are resumable via `vtcode exec resume <SESSION_ID>` or `vtcode exec resume --last`. Resumed runs keep their structured
history, so follow-up commands continue the same thread and honor prior approvals.
