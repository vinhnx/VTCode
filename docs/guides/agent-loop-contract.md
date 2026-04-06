# Agent Loop Contract

VT Code keeps its existing harness-first runtime, but its external loop contract
now lines up more closely with SDK-style agent runtimes.

This guide describes the public lifecycle semantics shared by interactive runs,
`vtcode exec`, harness logs, and Open Responses extension events.

## Message and Event Mapping

VT Code does not expose Claude-specific SDK structs. The canonical stream stays
`vtcode_exec_events::ThreadEvent`.

The closest concept mapping is:

| Agent SDK concept | VT Code event |
| --- | --- |
| `SystemMessage(init)` | `thread.started` |
| `AssistantMessage` | `item.*` with `agent_message`, `reasoning`, `tool_invocation` |
| Tool-result `UserMessage` | `item.*` with `tool_output` or `command_execution` |
| `StreamEvent` | `item.updated` plus Open Responses stream events |
| `ResultMessage` | `thread.completed` |
| `compact_boundary` | `thread.compact_boundary` |

`turn.started`, `turn.completed`, and `turn.failed` remain VT Code turn
wrappers around the inner item lifecycle.

## Terminal Thread Result

VT Code now emits `thread.completed` at the end of a session or exec run.

Fields:

- `thread_id`: stable event-stream thread identifier
- `session_id`: stable VT Code session identifier
- `subtype`: `success`, `error_max_turns`, `error_max_budget_usd`, `error_during_execution`, or `cancelled`
- `outcome_code`: VT Code-specific terminal code
- `result`: final assistant summary text on successful completion only
- `stop_reason`: provider stop reason when available
- `usage`: aggregate token usage for the full thread
- `total_cost_usd`: aggregate estimated cost when pricing metadata exists
- `num_turns`: total turn count

For `vtcode exec`, `outcome_code` comes from `TaskOutcome::code()`. Interactive
sessions preserve the corresponding VT Code session end semantics.

## Compaction Boundary

Whenever VT Code compacts history itself or via a provider-native compaction
path, it emits `thread.compact_boundary`.

Fields:

- `thread_id`
- `trigger`: `manual` or `auto`
- `mode`: `local` or `provider`
- `original_message_count`
- `compacted_message_count`
- `history_artifact_path`: optional archived history path

This is emitted for manual `/compact` flows and for automatic local fallback
compaction. When Open Responses is enabled, VT Code surfaces these as VT Code
custom extension events without changing the core Open Responses response model.

## Budget and Limits

`agent.harness.max_budget_usd` is the shared budget setting for interactive and
exec sessions.

- VT Code estimates cost from aggregate usage via `ModelResolver::estimate_cost`.
- If pricing metadata is unavailable for the active model, VT Code does not
  enforce the budget.
- In that case `total_cost_usd` stays `null` and VT Code emits one warning.

Turn limits still surface through `thread.completed.subtype = "error_max_turns"`.

## Hooks

VT Code now supports `hooks.lifecycle.pre_compact`.

`pre_compact` runs before VT Code records a compaction boundary. Its payload
includes:

- `session_id`
- `cwd`
- `hook_event_name = "PreCompact"`
- `trigger`
- `mode`
- `original_message_count`
- `compacted_message_count`
- `history_artifact_path`
- `transcript_path`

`session_start` with source `compact` remains supported for compatibility, but
`pre_compact` is the first-class hook for compaction-aware automation.

## Related Controls

These VT Code settings line up with common agent-loop controls:

- Tool allow and deny rules: `[permissions].allow`, `[permissions].deny`, tool policy config
- Permission mode: workspace trust, human-in-the-loop settings, full-auto allowlists
- Effort: provider/model reasoning settings
- Tool discovery: MCP and tool catalog flows
- Resume and fork continuity: session archives, thread bootstrap, and compaction envelopes

Subagents are unchanged in this pass.
