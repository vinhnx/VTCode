# Lifecycle Hooks

VT Code supports lifecycle hooks that execute shell commands in response to
agent events. Hooks let you enrich the model's context, enforce policy, surface
notifications, or block risky operations automatically. This guide explains how
hooks are configured in `vtcode.toml`, which events are available, and how the
agent interprets hook output.

## Configuration Overview

Hooks live under the `[hooks.lifecycle]` section in your project configuration
and are organized by event-specific arrays. Each entry defines an optional
`matcher` and one or more `hooks` to run when the matcher matches the incoming
event.

```toml
[hooks.lifecycle]
# Session start hook that applies to all triggers (startup, resume, clear, compact)
session_start = [
  { hooks = [ { command = "./scripts/setup-session.sh" } ] }
]

# Pre-tool hook scoped to all Bash commands
pre_tool_use = [
  {
    matcher = "Bash",
    hooks = [
      { command = "./scripts/validate-bash.py", timeout_seconds = 10 }
    ]
  }
]
```

Each hook entry maps to the following structures:

* **`matcher`** – optional string or regular expression that is applied to the
event-specific value (see [Matchers](#matchers)). Use `"*"` or leave empty to
match everything.
* **`hooks`** – array of commands. Every command must specify a `command`
string and may set an explicit `timeout_seconds` (defaults to 60 seconds).

> Tip: Put reusable scripts under `.vtcode/hooks/` (or similar) and reference
them with `$CLAUDE_PROJECT_DIR`/`$VT_PROJECT_DIR` so they work from any working
directory.

## Matchers

Matchers let you scope hooks to specific triggers:

* **Session events** (`session_start`, `session_end`) – compared to the trigger
string (`startup`, `resume`, `clear`, `compact`) or end reason (`clear`,
`logout`, `prompt_input_exit`, `other`).
* **UserPromptSubmit** – compared against the entire prompt text. Use regular
expressions to detect policies or keywords.
* **PreToolUse / PostToolUse** – compared against the tool name. Match builtin
names like `Write`, `Edit`, `Task`, `Bash`, or Model Context Protocol tools such
as `mcp__filesystem__read_file`.

The matcher syntax accepts:

* Empty string or `"*"` – matches all values.
* Plain string – exact match.
* Regular expression – interpreted as `^(?:PATTERN)$` to enforce a full match.

Invalid regular expressions will cause configuration validation to fail at load
time.

## Hook Execution Model

When a lifecycle hook triggers, VT Code spawns `sh -c <command>` in the project
root with the serialized JSON payload on stdin. The process inherits a
60-second timeout unless you provide `timeout_seconds` on the command entry.

The following environment variables are set for every hook command:

* `VT_PROJECT_DIR` / `CLAUDE_PROJECT_DIR` – absolute project root.
* `VT_SESSION_ID` / `CLAUDE_SESSION_ID` – unique session identifier.
* `VT_HOOK_EVENT` – current lifecycle event name (e.g., `PreToolUse`).
* `VT_TRANSCRIPT_PATH` / `CLAUDE_TRANSCRIPT_PATH` – current transcript path when
available.

Use these variables to locate scripts, persist artifacts, or provide additional
context to other tooling.

## Event Reference

### SessionStart

Runs when a session begins. The payload contains:

```json
{
  "session_id": "...",
  "cwd": "/path/to/project",
  "hook_event_name": "SessionStart",
  "source": "startup" | "resume" | "clear" | "compact",
  "transcript_path": "/path/to/transcript.jsonl" | null
}
```

Return additional context snippets via stdout or JSON to prime the model, or
run setup scripts that prepare your development environment.

### SessionEnd

Invoked when a session ends. The payload mirrors `SessionStart` but replaces
`source` with `reason` indicating `clear`, `logout`, `prompt_input_exit`, or
`other`. Use this hook to perform cleanup or logging.

### UserPromptSubmit

Runs before the agent processes the user's prompt with payload:

```json
{
  "session_id": "...",
  "cwd": "/path/to/project",
  "hook_event_name": "UserPromptSubmit",
  "prompt": "user text",
  "transcript_path": "..." | null
}
```

Hooks can inject extra context for the model or block prompt handling entirely.

### PreToolUse

Triggered after the agent prepares tool parameters but before the tool executes.
Payload fields include `tool_name`, serialized `tool_input`, and
`transcript_path`. Use this hook to approve, deny, or ask for confirmation
before running a tool.

### PostToolUse

Runs immediately after a tool completes successfully. The payload includes the
original tool input and the tool response (`tool_response`). Use this hook to
inspect outputs, enforce policy, or append extra context for the model.

## Interpreting Hook Results

Hook commands can influence control flow through exit codes, stdout/stderr, and
optional JSON output.

### Exit Codes

* `0` – success. Stdout becomes user-visible for most events (and is injected as
context for `UserPromptSubmit`).
* `2` – blocking error. The event-specific behavior matches Claude Code's
lifecycle semantics: for example, `PreToolUse` blocks tool execution and
provides stderr back to the agent.
* Any other code – non-blocking failure. Stderr is surfaced to the user, but the
agent continues processing.

Timed-out commands are treated as blocking errors and reported with an error
message.

### JSON Output

If stdout parses as JSON, VT Code interprets fields compatible with Claude Code
hooks:

| Field | Purpose |
| --- | --- |
| `continue` / `stopReason` | Control whether the agent proceeds after the hook. |
| `suppressOutput` | Hide stdout from the transcript. |
| `systemMessage` | Display an informational message. |
| `decision` / `reason` | Event-specific decisions (block prompt, block stop, etc.). |
| `hookSpecificOutput` | Structured data keyed by `hookEventName` with additional context. |

Pre- and post-tool hooks also support `permissionDecision` /
`permissionDecisionReason` inside `hookSpecificOutput` to allow, deny, or prompt
for confirmation. User prompt hooks can block prompt processing and include a
custom reason, while post-tool hooks can block agent continuation and provide
remedial guidance.

### Additional Context

Hooks can append strings to the model context in two ways:

1. Print plain text to stdout with exit code `0` (SessionStart and
   UserPromptSubmit automatically inject stdout as context).
2. Provide `hookSpecificOutput.additionalContext` as a JSON array or string in
the JSON response.

Messages emitted via stderr or interpreted fields are surfaced inside VT Code's
UI so you can monitor hook activity.

## Best Practices

* Validate regular expressions and configuration with `vtcode config validate`.
* Keep hook scripts idempotent and side-effect aware—hooks may run multiple
commands in parallel for matching groups.
* Use short timeouts and descriptive error messages so users understand why an
operation was blocked.
* Store reusable hooks alongside your repository and reference them with
project-root environment variables.
