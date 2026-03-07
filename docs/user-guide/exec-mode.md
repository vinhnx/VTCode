# Exec Mode Automation

VT Code's exec mode lets you run autonomous tasks from the shell without starting the interactive TUI. The workflow mirrors the
`codex exec` guidance: commands run non-interactively, stream structured events, and can be resumed later. By default, `vtcode
exec` prints progress, summaries, and outcomes to **stderr**. `stdout` remains silent unless you request structured output (via `--json`), allowing you to safely pipe the output to other tools without pollution.

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

## Dry-run mode

Use `--dry-run` to force read-only execution while still allowing the agent to analyze and plan:

```bash
vtcode exec --dry-run "identify required code changes for adding OAuth refresh token support"
```

In dry-run mode, VT Code enables plan/read-only enforcement for tool calls. Mutating operations are blocked, and the run reports what
would be changed rather than applying edits.

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

Exec runs are resumable via `vtcode exec resume <SESSION_ID> <PROMPT>` or `vtcode exec resume --last <PROMPT>`.

```bash
vtcode exec resume --last "continue from the prior investigation and summarize the root cause"
```

Resume uses the archived workspace recorded in the original exec session. The follow-up prompt starts a new autonomous turn on the
same archived session identifier, so structured history and the saved session archive stay in sync.
