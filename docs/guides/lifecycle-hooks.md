# Lifecycle Hooks

VT Code supports lifecycle hooks that execute shell commands in response to
agent events. Hooks let you enrich the model's context, enforce policy, surface
notifications, or block risky operations automatically. This guide explains how
hooks are configured in `vtcode.toml`, which events are available, and how the
agent interprets hook output.

Similar to Claude Code Hooks: https://docs.claude.com/en/docs/claude-code/hooks.

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

### TaskCompletion / TaskCompleted

Runs when a task is marked completed or failed (for example, an agent team task).
Configure either `task_completion` or `task_completed` in `vtcode.toml` (both are
supported). Payload:

```json
{
  "session_id": "...",
  "cwd": "/path/to/project",
  "hook_event_name": "TaskCompletion",
  "task_name": "team_task",
  "status": "completed" | "failed",
  "details": { "task_id": 1, "assigned_to": "teammate-1", "summary": "..." } | null,
  "transcript_path": "..." | null
}
```

### TeammateIdle

Runs when a teammate has no pending or in-progress tasks. Configure
`teammate_idle` in `vtcode.toml`. Payload:

```json
{
  "session_id": "...",
  "cwd": "/path/to/project",
  "hook_event_name": "TeammateIdle",
  "teammate": "teammate-1",
  "details": { "team": "team", "teammate": "teammate-1" } | null,
  "transcript_path": "..." | null
}
```

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

Messages emitted via stderr or interpreted fields are captured for diagnostics.
Fatal/error diagnostics are written to tracing logs for debugging and are not
rendered in the TUI transcript.

## Best Practices

* Validate regular expressions and configuration with `vtcode config validate`.
* Keep hook scripts idempotent and side-effect aware—hooks may run multiple
commands in parallel for matching groups.
* Use short timeouts and descriptive error messages so users understand why an
operation was blocked.
* Store reusable hooks alongside your repository and reference them with
project-root environment variables.

## Practical Setup Guide

### Getting Started

To start using lifecycle hooks in your project:

1. **Create a `vtcode.toml` configuration file** in your project root if you don't already have one:

```bash
touch vtcode.toml
```

2. **Add the lifecycle hooks section** to your configuration:

```toml
[hooks.lifecycle]
# Add your hooks here
```

3. **Create a hooks directory** to store your hook scripts:

```bash
mkdir -p .vtcode/hooks
```

### Example Setup: Enhanced Session Context

Here's a practical example that adds project context when a session starts:

1. **Create a script** at `.vtcode/hooks/session-context.sh`:

```bash
#!/bin/bash

# Read the JSON payload from stdin
payload=$(cat)

# Extract project info and return as context
echo "Project: $(basename $VT_PROJECT_DIR)" > /tmp/context.txt
echo "Files in project root:" >> /tmp/context.txt
ls -la $VT_PROJECT_DIR >> /tmp/context.txt

# Output additional context for the model
cat /tmp/context.txt
rm /tmp/context.txt
```

2. **Make the script executable**:

```bash
chmod +x .vtcode/hooks/session-context.sh
```

3. **Configure the hook** in your `vtcode.toml`:

```toml
[hooks.lifecycle]
session_start = [
  { 
    hooks = [ 
      { command = "$VT_PROJECT_DIR/.vtcode/hooks/session-context.sh" } 
    ] 
  }
]
```

### Example Setup: Pre-Tool Validation

Here's how to set up validation before running bash commands:

1. **Create a validation script** at `.vtcode/hooks/validate-bash.sh`:

```bash
#!/bin/bash

# Read the JSON payload from stdin
payload=$(cat)

# Extract the command being run
command=$(echo "$payload" | jq -r '.tool_input.command // ""')

# Block dangerous commands
if [[ "$command" == *"rm -rf"* ]] || [[ "$command" == *"/"* ]]; then
    echo "Dangerous command blocked: $command" >&2
    exit 2  # This will block the command
fi

# Allow safe commands
echo "Command approved: $command"
```

2. **Configure the pre-tool hook**:

```toml
[hooks.lifecycle]
pre_tool_use = [
  {
    matcher = "Bash",
    hooks = [
      { 
        command = "$VT_PROJECT_DIR/.vtcode/hooks/validate-bash.sh", 
        timeout_seconds = 5 
      }
    ]
  }
]
```

### Testing Your Hooks

1. **Validate your configuration**:

```bash
vtcode config validate
```

2. **Test hook execution manually** by simulating the JSON payload:

```bash
# Create a test payload file
echo '{"session_id": "test", "cwd": "/tmp", "hook_event_name": "SessionStart", "source": "startup", "transcript_path": null}' > test_payload.json

# Run your hook manually to test
cat test_payload.json | .vtcode/hooks/session-context.sh
```

### Common Use Cases

Here are some common lifecycle hook use cases you might want to implement:

**Security Validation**: Validate dangerous commands before execution
**Context Enrichment**: Add project-specific information when sessions start
**Policy Enforcement**: Block prompts containing sensitive keywords
**Logging**: Track agent activity and tool usage
**Environment Setup**: Configure project-specific environment variables or settings

### Detailed Example Configurations

Here are complete example configurations for common scenarios:

#### Example 1: Security Policy Enforcement

Block potentially dangerous operations and log security events:

```toml
[hooks.lifecycle]
# Block prompts containing sensitive data
user_prompt_submit = [
  {
    matcher = ".*password.*|.*secret.*|.*token.*|.*api.*key.*",
    hooks = [
      { 
        command = '''
          python3 -c "
import sys, json
payload = json.load(sys.stdin)
print(f'Prompt blocked for security reasons: {payload[\"prompt\"][:50]}...', file=sys.stderr)
"
        ''',
        timeout_seconds = 5
      }
    ]
  }
]

# Validate Bash commands for dangerous patterns
pre_tool_use = [
  {
    matcher = "Bash",
    hooks = [
      { 
        command = "$VT_PROJECT_DIR/.vtcode/hooks/security-check.sh", 
        timeout_seconds = 10 
      }
    ]
  }
]

# Log completed Bash commands
post_tool_use = [
  {
    matcher = "Bash",
    hooks = [
      { 
        command = "$VT_PROJECT_DIR/.vtcode/hooks/log-command.sh" 
      }
    ]
  }
]
```

#### Example 2: Development Environment Setup

Set up project-specific context and tools:

```toml
[hooks.lifecycle]
# Set up project environment at session start
session_start = [
  { 
    hooks = [ 
      { 
        command = "$VT_PROJECT_DIR/.vtcode/hooks/setup-env.sh",
        timeout_seconds = 30
      }
    ] 
  }
]

# Add project-specific context for code modifications
pre_tool_use = [
  {
    matcher = "Write|Edit",
    hooks = [
      { 
        command = "$VT_PROJECT_DIR/.vtcode/hooks/check-style.sh" 
      }
    ]
  }
]

# Validate code after write operations
post_tool_use = [
  {
    matcher = "Write|Edit",
    hooks = [
      { 
        command = "$VT_PROJECT_DIR/.vtcode/hooks/run-linter.sh" 
      }
    ]
  }
]
```

#### Example 3: CI/CD Integration

Integrate with your development workflow:

```toml
[hooks.lifecycle]
# Run tests when code is modified
post_tool_use = [
  {
    matcher = "Write|Edit",
    hooks = [
      { 
        command = "$VT_PROJECT_DIR/.vtcode/hooks/run-tests.sh",
        timeout_seconds = 120
      }
    ]
  }
]

# Validate commit messages
pre_tool_use = [
  {
    matcher = "Bash",
    hooks = [
      { 
        command = "$VT_PROJECT_DIR/.vtcode/hooks/validate-commit.sh" 
      }
    ]
  }
]

# Update documentation on file changes
post_tool_use = [
  {
    matcher = ".*\\.md$",
    hooks = [
      { 
        command = "$VT_PROJECT_DIR/.vtcode/hooks/update-docs-index.sh" 
      }
    ]
  }
]
```

#### Example 4: Monitoring and Analytics

Track agent usage and performance:

```toml
[hooks.lifecycle]
# Log session start
session_start = [
  { 
    hooks = [ 
      { 
        command = "$VT_PROJECT_DIR/.vtcode/hooks/log-session-start.sh" 
      }
    ] 
  }
]

# Log tool usage
post_tool_use = [
  {
    matcher = ".*",
    hooks = [
      { 
        command = "$VT_PROJECT_DIR/.vtcode/hooks/log-tool-usage.sh" 
      }
    ]
  }
]

# Log session end
session_end = [
  { 
    hooks = [ 
      { 
        command = "$VT_PROJECT_DIR/.vtcode/hooks/log-session-end.sh" 
      }
    ] 
  }
]
```

### Validation and Testing

Before using lifecycle hooks in production, validate your configuration:

1. **Validate configuration syntax**:

```bash
vtcode config validate
```

This checks that your `vtcode.toml` file has valid syntax and that all regular expressions in matchers are properly formatted.

2. **Test hooks manually** by simulating the JSON payload:

```bash
# Create a test payload file that matches the expected format
cat > test-payload.json << 'EOF'
{
  "session_id": "test-session-123",
  "cwd": "/path/to/project",
  "hook_event_name": "SessionStart",
  "source": "startup",
  "transcript_path": null
}
EOF

# Run your hook manually to test it
cat test-payload.json | .vtcode/hooks/session-context.sh
```

3. **Check script permissions** - make sure your hook scripts are executable:

```bash
chmod +x .vtcode/hooks/*.sh
```

### Debugging Tips

1. **Check hook execution** by looking at stderr output in the VT Code UI
2. **Use `jq`** to parse JSON payloads in your scripts for easier handling
3. **Set shorter timeouts** during development to avoid hanging processes
4. **Log to files** for debugging complex hook logic:

```bash
echo "$(date): Processing hook for $VT_HOOK_EVENT" >> /tmp/vtcode-hooks.log
```

5. **Test exit codes** - remember that exit code 2 blocks execution, so test carefully during development:

```bash
# Test with a script that won't block
echo 'echo "Test output"' > temp_hook.sh
chmod +x temp_hook.sh
cat payload.json | ./temp_hook.sh
rm temp_hook.sh
```

### Security Considerations

1. **Sandbox your scripts** - avoid running potentially malicious code from hook outputs
2. **Validate all inputs** - never trust user input or tool parameters without validation
3. **Use relative paths** - prefer `$VT_PROJECT_DIR` over hardcoded paths
4. **Minimize permissions** - run hooks with minimal required privileges
5. **Audit script content** - regularly review hook scripts for security issues

### Performance Optimization

1. **Optimize timeout values** - set appropriate timeouts for different operations:
   - Fast validations: 1-5 seconds
   - Code analysis: 10-30 seconds
   - Full project scans: 30-60 seconds
   - Long-running processes: 120+ seconds (use sparingly)

2. **Cache expensive operations** to avoid repeating the same work:

```bash
# Example: cache git status results
cache_file="/tmp/vtcode_git_status_$VT_SESSION_ID"
if [[ ! -f "$cache_file" ]] || [[ $(find "$cache_file" -mmin +5) ]]; then
  git status --porcelain > "$cache_file"
fi
cat "$cache_file"
```

3. **Parallel execution considerations** - hooks in the same group run sequentially, but multiple matching groups might run in parallel, so design your hooks to be thread-safe if needed.
