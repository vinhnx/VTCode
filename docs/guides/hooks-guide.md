# VT Code Hooks System Documentation

This document describes the hooks system in VT Code, which is inspired by and compatible with the Claude Code hooks system. Hooks allow you to execute custom scripts in response to various events in the VT Code agent lifecycle.

## Overview

VT Code hooks enable automation by running shell commands in response to specific events. The system supports various lifecycle events and provides a flexible matching mechanism to target specific tools or events.

## Configuration

Hooks are configured in your `vtcode.toml` file under the `[hooks.lifecycle]` section:

```toml
[hooks.lifecycle]
# Pre-tool use hooks - Run before tools execute
[[hooks.lifecycle.pre_tool_use]]
matcher = "Bash"  # Match specific tools using regex
hooks = [
  { command = "$VT_PROJECT_DIR/.vtcode/hooks/bash-validator.sh", timeout_seconds = 30 }
]

# Post-tool use hooks - Run after tools execute successfully
[[hooks.lifecycle.post_tool_use]]
matcher = "Write|Edit"  # Match Write or Edit tools
hooks = [
  { command = "$VT_PROJECT_DIR/.vtcode/hooks/code-formatter.sh" }
]

# User prompt submit hooks - Run when user submits a prompt
[[hooks.lifecycle.user_prompt_submit]]
hooks = [
  { command = "$VT_PROJECT_DIR/.vtcode/hooks/prompt-validator.sh" }
]

# Session start hooks - Run when a session begins
[[hooks.lifecycle.session_start]]
hooks = [
  { command = "$VT_PROJECT_DIR/.vtcode/hooks/session-setup.sh" }
]

# Session end hooks - Run when a session ends
[[hooks.lifecycle.session_end]]
hooks = [
  { command = "$VT_PROJECT_DIR/.vtcode/hooks/session-cleanup.sh" }
]

# Task completion hooks - Run when a task completes or fails
[[hooks.lifecycle.task_completed]]
hooks = [
  { command = "$VT_PROJECT_DIR/.vtcode/hooks/task-complete.sh" }
]

# Teammate idle hooks - Run when a teammate has no active tasks
[[hooks.lifecycle.teammate_idle]]
hooks = [
  { command = "$VT_PROJECT_DIR/.vtcode/hooks/teammate-idle.sh" }
]
```

## Hook Events

### PreToolUse

-   Runs after VT Code creates tool parameters and before processing the tool call
-   Can block, allow, or modify tool execution
-   Common matchers: `Write`, `Edit`, `Bash`, `Read`, etc.

### PostToolUse

-   Runs immediately after a tool completes successfully
-   Can provide feedback or perform follow-up actions
-   Uses same matchers as PreToolUse

### UserPromptSubmit

-   Runs when the user submits a prompt, before VT Code processes it
-   Can validate prompts, add context, or block certain types of prompts

### SessionStart

-   Runs when VT Code starts a new session
-   Useful for loading development context, installing dependencies, or setting up environment variables

### SessionEnd

-   Runs when a VT Code session ends
-   Useful for cleanup tasks, logging session statistics, or saving session state

### TaskCompletion / TaskCompleted

-   Runs when a task is marked completed or failed (for example, agent team tasks)
-   Configure `task_completion` or `task_completed` (both are supported)
-   Matchers apply to `task_name`

### TeammateIdle

-   Runs when a teammate has no pending or in-progress tasks
-   Matchers apply to `teammate` (the teammate name)

## Hook Matching

The `matcher` field supports:

-   Simple strings that match exactly: `Write` matches only the Write tool
-   Regex patterns: `Edit|Write` or `.*` (match all)
-   Use `.*` to match all tools for a specific event type
-   Empty string or no matcher field matches all events of that type

## Hook Scripts

Hook scripts receive JSON data via stdin containing session information and event-specific data:

```json
{
    "session_id": "vt-12345-67890",
    "transcript_path": "/path/to/transcript.jsonl",
    "cwd": "/current/working/directory",
    "hook_event_name": "PreToolUse",
    "tool_name": "Write",
    "tool_input": {
        "file_path": "/path/to/file.txt",
        "content": "file content"
    }
}
```

## Exit Code Semantics

-   **Exit code 0**: Success - hook completed normally
-   **Exit code 2**: Blocking error - prevents the action from proceeding
-   **Other exit codes**: Non-blocking error - logs error but continues

## Environment Variables

Hook scripts have access to these environment variables:

-   `VT_PROJECT_DIR`: Path to the project root directory
-   `CLAUDE_PROJECT_DIR`: Same as VT_PROJECT_DIR (for compatibility)
-   `VT_SESSION_ID`: Current session ID
-   `CLAUDE_SESSION_ID`: Same as VT_SESSION_ID (for compatibility)
-   `VT_HOOK_EVENT`: Name of the hook event being executed
-   `VT_TRANSCRIPT_PATH`: Path to the current transcript file

## JSON Output Format

Hooks can return structured JSON in stdout for advanced control:

```json
{
    "continue": true,
    "stopReason": "string",
    "suppressOutput": true,
    "systemMessage": "string",
    "hookSpecificOutput": {
        "hookEventName": "PreToolUse",
        "permissionDecision": "allow",
        "permissionDecisionReason": "My reason here",
        "additionalContext": "Additional information for VT Code"
    }
}
```

## Security Considerations

**USE AT YOUR OWN RISK**: VT Code hooks execute arbitrary shell commands on your system automatically. Always:

-   Validate and sanitize inputs
-   Quote shell variables properly: `"$VAR"` not `$VAR`
-   Block path traversal by checking for `..` in file paths
-   Use absolute paths for scripts
-   Review all hook commands before adding them to your configuration

## Example Hook Scripts

The example hook scripts provided demonstrate common use cases:

-   `bash-validator.sh`: Validates bash commands for safety
-   `file-protection.sh`: Protects sensitive files from modification
-   `code-formatter.sh`: Formats code after file operations
-   `prompt-validator.sh`: Validates user prompts for sensitive information
-   `session-setup.sh`: Sets up environment at session start
-   `session-cleanup.sh`: Performs cleanup at session end
