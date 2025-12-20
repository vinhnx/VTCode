# Lifecycle Hooks (Experimental)

**Status:** Documented but not enabled by default  
**Stability:** Experimental (may change without notice)

## Overview

Lifecycle hooks allow you to execute custom scripts at key points in the VT Code agent lifecycle:

- **Session start** - Run setup scripts when a session begins
- **Session end** - Run cleanup or logging scripts when a session ends
- **User prompt submit** - Execute before processing user input
- **Pre-tool execution** - Validate or transform tools before they run
- **Post-tool execution** - Log, verify, or process tool results

## Use Cases

- **Security validation** - Scan bash commands before execution
- **Code linting** - Run formatters/linters after file writes
- **Audit logging** - Log all tool usage for compliance
- **Session setup** - Load environment variables or context on startup
- **Cleanup** - Archive logs or cleanup temp files on exit

## Configuration

To enable hooks, add this to your `vtcode.toml`:

```toml
[hooks.lifecycle]
session_start = []        # Scripts to run at session start
session_end = []          # Scripts to run at session end
user_prompt_submit = []   # Scripts to run before processing user input
pre_tool_use = []         # Scripts to run before tool execution
post_tool_use = []        # Scripts to run after tool execution
```

## Examples

### Example 1: Run linter after file writes

```toml
[hooks.lifecycle]
post_tool_use = [
  { matcher = "Write|Edit", hooks = [{ command = ".vtcode/hooks/run-linter.sh" }] }
]
```

Create `.vtcode/hooks/run-linter.sh`:
```bash
#!/bin/bash
# Run prettier/rustfmt on modified files
echo "Running linters on modified files..."
# Your linting commands here
```

### Example 2: Security validation for bash commands

```toml
[hooks.lifecycle]
pre_tool_use = [
  { matcher = "Bash", hooks = [{ command = ".vtcode/hooks/security-check.sh", timeout_seconds = 10 }] }
]
```

Create `.vtcode/hooks/security-check.sh`:
```bash
#!/bin/bash
# Validate bash command before execution
COMMAND=$1
if [[ "$COMMAND" =~ "rm -rf" ]]; then
    echo "Dangerous command blocked: $COMMAND"
    exit 1
fi
echo "Command validated: $COMMAND"
exit 0
```

### Example 3: Session setup with environment

```toml
[hooks.lifecycle]
session_start = [
  { hooks = [{ command = ".vtcode/hooks/setup-env.sh", timeout_seconds = 30 }] }
]
```

Create `.vtcode/hooks/setup-env.sh`:
```bash
#!/bin/bash
# Setup environment for the session
export PROJECT_ROOT="$(pwd)"
export RUST_BACKTRACE=1
echo "Session setup complete"
```

### Example 4: Comprehensive logging

```toml
[hooks.lifecycle]
session_start = [
  { hooks = [{ command = ".vtcode/hooks/log-session-start.sh" }] }
]
session_end = [
  { hooks = [{ command = ".vtcode/hooks/log-session-end.sh" }] }
]
post_tool_use = [
  { matcher = ".*", hooks = [{ command = ".vtcode/hooks/log-tool-usage.sh" }] }
]
```

## Hook Matchers

The `matcher` field uses regex patterns to match tool names:

| Pattern | Matches |
|---------|---------|
| `Bash` | Bash/shell commands |
| `Write` | write_file tool |
| `Edit` | edit_file tool |
| `Read` | read_file tool |
| `Write\|Edit` | write_file OR edit_file |
| `.*` | All tools |

## Configuration Options

Each hook can have these options:

```toml
{ 
  command = "/path/to/script.sh",  # Required: script to execute
  timeout_seconds = 30              # Optional: max runtime (default: 300)
}
```

## Environment Variables

Hooks have access to:
- Standard environment variables
- `$VT_PROJECT_DIR` - Workspace root directory
- `$VT_SESSION_ID` - Current session identifier
- Tool-specific variables (depending on hook type)

## Notes

- Hooks are matched using regex patterns
- Timeouts prevent hung script execution
- Scripts must be executable (`chmod +x script.sh`)
- Exit code 0 = success, non-zero = failure
- Non-existent scripts are silently skipped
- Hooks are **experimental** and may change behavior between versions

## Troubleshooting

**Hook not running:**
- Ensure script is executable: `chmod +x .vtcode/hooks/script.sh`
- Check script path is correct (relative to project root)
- Verify matcher regex matches your tool name

**Hook timeout:**
- Increase `timeout_seconds` for long-running scripts
- Check script for infinite loops or blocking I/O

**Hook script error:**
- Test script independently: `bash .vtcode/hooks/script.sh`
- Check exit codes (non-zero causes hook to fail)
- Add logging/debugging to script

## Advanced: Hook Integration with AGENTS.md

For projects using AGENTS.md guidelines, you can use hooks to enforce coding standards:

```bash
#!/bin/bash
# Enforce AGENTS.md standards on writes
if grep -q "unwrap()" "$1"; then
    echo "Error: unwrap() not allowed (see AGENTS.md)"
    exit 1
fi
```
