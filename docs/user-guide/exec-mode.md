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

## Continuation and verification

Exec mode now uses a harness-managed continuation loop instead of accepting the first completion-sounding assistant message.

- By default, `agent.harness.continuation_policy = "exec_only"` enables continuation only for exec/full-auto runs.
- Review mode and `vtcode exec --dry-run` stay single-pass and read-only.
- Interactive TUI sessions keep manual control unless you explicitly set `agent.harness.continuation_policy = "all"`.

When a run starts, VT Code uses `task_tracker` state as the completion contract. If the model has not created a tracker yet, exec mode
creates an internal three-step scaffold (`analyze`, `change`, `verify`) and persists it through the existing task-tracker files under
`.vtcode/`.

Completion is accepted only when:

- all tracker steps are completed
- every step-level `verify` command has passed

Verification commands run sequentially from the workspace root through the existing exec/sandbox stack. The first non-zero exit stops
verification, records a harness event, and forces a fresh continuation turn with the failure summary.

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
- `harness` – continuation and verification lifecycle items such as `continuation_started`, `verification_started`, and `verification_failed`.

These events align with the published Codex non-interactive schema so downstream automation, dashboards, or log shippers can reuse
existing parsers without modification.

If `--events` is not provided, exec mode also respects `agent.harness.event_log_path`. Relative or absolute file paths write JSONL
events directly; directory paths get a timestamped `harness-<session>-<timestamp>.jsonl` file.

## Resuming sessions

Exec runs are resumable via `vtcode exec resume <SESSION_ID> <PROMPT>` or `vtcode exec resume --last <PROMPT>`.

```bash
vtcode exec resume --last "continue from the prior investigation and summarize the root cause"
```

Resume uses the archived workspace recorded in the original exec session. The follow-up prompt starts a new autonomous turn on the
same archived session identifier, so structured history and the saved session archive stay in sync.
