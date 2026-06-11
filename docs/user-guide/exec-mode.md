# Exec Mode Automation

VT Code's exec mode lets you run autonomous tasks from the shell without starting the interactive TUI. The workflow mirrors the
`codex exec` guidance: commands run non-interactively, stream structured events, and can be resumed later. By default, `vtcode
exec` prints progress, summaries, and outcomes to **stderr**. `stdout` remains silent unless you request structured output (via `--json`), allowing you to safely pipe the output to other tools without pollution.

For the complete runtime lifecycle mapping, see [Agent Loop Contract](../guides/agent-loop-contract.md).

`vtcode schedule` uses this same exec runtime for durable prompt tasks. When a durable scheduled prompt fires, VT Code launches a fresh `vtcode exec` run in the configured workspace and records the resulting status and artifacts. For session-scoped `/loop` polling and reminders, see [Scheduled Tasks](./scheduled-tasks.md).

## Launching exec mode

```bash
vtcode exec "count the total number of lines of code in this project"
```

Exec mode enforces the same workspace trust checks as the interactive UI. The workspace must be marked as `full_auto`, and the
configured `automation.full_auto` section must enable autonomous execution. When the command starts it activates full-auto as an
execution and permission layer on top of the active primary agent; full-auto is not a primary agent itself. Explicit primary-agent
choices, including `duck`, are honoured. If no primary agent is explicitly selected or configured, VT Code selects the effective
`auto` primary agent, and the run fails fast if no effective `auto` exists.

Full-auto treats `[automation.full_auto].allowed_tools` as a hard gate: tools outside the allow-list are denied, and tools inside
the allow-list still pass explicit deny and policy checks. Promptable outcomes that remain inside the allow-list are routed through
automatic permission review instead of asking. `--dangerously-skip-permissions` similarly auto-approves promptable actions while
still respecting explicit denies and policy blocks. You can capture the final summary separately with `--last-message-file` or
persist the raw JSON stream with `--events`. Use `--json` when you want to see the live JSONL feed on stdout.

### Granting workspace trust

If the workspace has not been trusted yet, exec mode resolves trust in this order:

1. **Already trusted.** Persisted `full_auto` trust from a previous run is reused.
2. **`VTCODE_TRUST_WORKSPACE=full-auto`.** Grants and persists full-auto trust for the current workspace before
   running. Set `VTCODE_TRUST_WORKSPACE=deny` to explicitly refuse trust (the run aborts with an error instead of
   prompting). Aliases `1`, `yes`, `on`, `trust`, `trusted` map to grant; `0`, `no`, `off`, `deny`, `denied` map to
   refuse. Add `VTCODE_TRUST_WORKSPACE_QUIET=1` to suppress the "trusted via env" confirmation line.
3. **Interactive TTY prompt.** When stdin and stdout are both connected to a terminal, exec mode prompts you to trust
   the workspace inline — no separate interactive session required.
4. **Hard error.** In every other case (non-TTY, no env override, untrusted) the run aborts with an error listing the
   options above so CI logs are self-explanatory.

`--auto/--full-auto` and `vtcode benchmark` honour the same precedence, so the same env-var works across every
autonomous entry point.

## Dry-run mode

Use `--dry-run` to force read-only execution while still allowing the agent to analyze and plan:

```bash
vtcode exec --dry-run "identify required code changes for adding OAuth refresh token support"
```

In dry-run mode, VT Code enables plan/read-only enforcement for tool calls. Mutating operations are blocked, and the run reports what
would be changed rather than applying edits.

## Continuation and verification

Exec mode now uses a harness-managed continuation loop instead of accepting the first completion-sounding assistant message.

- By default, `agent.harness.continuation_policy = "all"` enables harness continuation in both interactive and exec/full-auto runs.
- Review mode and `vtcode exec --dry-run` stay single-pass and read-only.
- Set `agent.harness.continuation_policy = "exec_only"` if you want interactive TUI sessions to keep manual turn control.

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
- `thread.completed` – emitted once per session with terminal subtype, usage, stop reason, and optional `total_cost_usd`.
- `thread.compact_boundary` – emitted when VT Code compacts history locally or through a provider compaction path.
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

`agent.harness.max_budget_usd` applies to exec mode as well as interactive runs.
If model pricing metadata is unavailable, VT Code leaves `total_cost_usd` unset
and skips budget enforcement for that session.

## Resuming sessions

Exec runs are resumable via `vtcode exec resume <SESSION_ID> <PROMPT>` or `vtcode exec resume --last <PROMPT>`.

```bash
vtcode exec resume --last "continue from the prior investigation and summarize the root cause"
```

Resume uses the archived workspace recorded in the original exec session. The follow-up prompt starts a new autonomous turn on the
same archived session identifier, so structured history and the saved session archive stay in sync.
