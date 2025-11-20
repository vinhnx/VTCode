# Agent MCP Failure Handling Guidelines

This document guides **LLM agents** on how to diagnose and explain MCP failures to users.

## Core Principle

When an MCP tool fails, agents should:

1. **Identify** the failure type (exit code, error message)
2. **Explain** why it happened (non-technical)
3. **Diagnose** the root cause (specific steps)
4. **Fix** the issue (actionable commands)
5. **Verify** the solution works

## MCP Fundamentals for Agents

### What is MCP?
- **Not a shell command** (can't call `mcp::time::get_current_time`)
- **Requires a running server process** (via `uvx` or installed Python package)
- **Configured in `.mcp.json` and `vtcode.toml`**
- **Invoked through the tool interface** (not bash)

### Common User Mistake
Users often try shell invocation:
```bash
$ mcp::time::get_current_time Asia/Tokyo
# Result: command not found (exit 127)
```

**Agent must explain:** MCP tools aren't shell commands. They require:
- MCP server to be running
- Tool interface invocation
- Proper configuration

## Failure Type → Agent Response Map

### Type 1: Command Not Found (Exit 127)

**What the error looks like:**
```
$ mcp::time::get_current_time Asia/Tokyo
zsh: command not found: mcp::time::get_current_time
```

**Agent response:**
```
MCP tools aren't shell commands. They're accessed via the tool interface, not through the shell.

The syntax 'mcp::time::get_current_time' doesn't work in bash/zsh. 

Instead, MCP tools:
- Are invoked through the VTCode tool system
- Require the MCP server process to be running
- Need proper configuration in .mcp.json

To check if the time MCP is available:
1. Verify it's installed: uvx mcp-server-time --help
2. Check .mcp.json is configured: cat .mcp.json | grep -A 3 "time"
3. Ensure MCP is enabled: grep "enabled = true" vtcode.toml
4. Run diagnostics: vtcode doctor

If needed, install: pip install mcp-server-time
```

### Type 2: Broken Pipe (EPIPE)

**What the error looks like:**
```
Tool execution failed
Error: BrokenPipe
Message: "connection lost to MCP server"
```

**Agent response:**
```
The MCP server for 'time' crashed or lost connection.

This happens when:
- Server process terminates unexpectedly
- Server can't handle the tool request
- Resource exhaustion (memory/CPU)

Fix:
1. Check MCP configuration is valid:
   cat .mcp.json | head -20

2. Verify server installation:
   uvx mcp-server-time --help

3. Test server manually:
   uvx mcp-server-time
   # Should show help or wait for input

4. Check if it's in PATH:
   which uvx

5. Run full diagnostics:
   vtcode doctor

6. If still failing, increase timeout:
   Edit vtcode.toml:
   [mcp]
   startup_timeout_seconds = 30
```

### Type 3: Timeout

**What the error looks like:**
```
Tool execution timeout
Error: Timeout
Message: "MCP server did not respond within 15 seconds"
```

**Agent response:**
```
The MCP server took too long to start or respond.

This typically means:
- Server startup is slower than expected
- System resources are constrained
- Network latency (if remote server)
- Server is hung or stuck

Solutions:

1. Increase startup timeout in vtcode.toml:
   [mcp]
   startup_timeout_seconds = 30  # default is 15

2. Check system resources:
   free -h          # Memory availability
   df -h            # Disk space
   top -l 1 | head  # CPU usage

3. Try starting the server manually:
   uvx mcp-server-time
   # If this hangs, the server itself has an issue

4. Enable debug logging:
   [mcp]
   log_level = "debug"
   
   Then check logs for details.

5. Check if uvx is available:
   which uvx
   uvx --version
```

### Type 4: Provider Not Found / Config Error

**What the error looks like:**
```
Tool execution failed
Error: InitializationError
Message: "No such process" or "Failed to start MCP server"
```

**Agent response:**
```
The MCP server failed to start because it's not properly installed or configured.

Check your setup:

1. Verify .mcp.json exists and is valid JSON:
   cat .mcp.json
   
   Should look like:
   {
     "mcpServers": {
       "time": {
         "command": "uvx",
         "args": ["mcp-server-time"],
         "type": "stdio"
       }
     }
   }

2. Install the MCP server:
   pip install mcp-server-time
   # or
   uv pip install mcp-server-time

3. Verify it's installed:
   which uvx
   uvx mcp-server-time --help

4. Check vtcode.toml enables MCP:
   grep -A 5 "^\\[mcp\\]" vtcode.toml
   # Should show: enabled = true

5. Run diagnostic:
   vtcode doctor
   # Check "MCP: " line for status

If still failing, manually verify:
   uvx mcp-server-time --version
```

### Type 5: Tool Invocation Failed (Runtime Error)

**What the error looks like:**
```
Tool execution FAILURE
Status: FAILURE
Error: "Invalid parameter: timezone"
```

**Agent response:**
```
The MCP server is running, but the tool invocation failed.

This means the server is working, but something about the tool request was wrong.

Debug steps:

1. Verify tool exists and parameters are correct:
   - Check what parameters the tool expects
   - Verify you're providing required fields
   - Check parameter types/formats

2. Check MCP server version:
   pip show mcp-server-time
   
   If outdated, upgrade:
   pip install --upgrade mcp-server-time

3. Test with correct parameters:
   [Try again with valid parameters]

4. If you need to know what parameters the tool accepts:
   Check the MCP server documentation or:
   uvx mcp-server-time --help

5. Enable debug logging to see detailed errors:
   [mcp]
   log_level = "debug"
```

## Agent Implementation Checklist

When receiving an MCP tool failure, agents should:

- [ ] Parse the error message for exit code or error type
- [ ] Classify error (Command not found / EPIPE / Timeout / Config / Runtime)
- [ ] Explain in plain language what went wrong
- [ ] Show at least one diagnostic command
- [ ] Provide specific fix steps with examples
- [ ] Suggest verification ("Run `vtcode doctor` to verify")
- [ ] Reference `docs/mcp/MCP_DIAGNOSTIC_GUIDE.md` for complex issues
- [ ] **Never** suggest shell invocation of MCP tools

## Template for Agents

```markdown
**Problem:** [Describe the failure]

**Why it happened:**
[Plain language explanation - avoid jargon]

**Diagnostic steps:**

1. [First thing to check]
   $ [command]

2. [Second thing to check]
   $ [command]

3. [Third thing to check]
   $ [command]

**Solution:**

1. [Fix step 1]
   $ [command]

2. [Fix step 2]
   $ [command]

3. [Verify it works]
   $ [verification command]

**More help:** See `docs/mcp/MCP_DIAGNOSTIC_GUIDE.md` for detailed diagnostics.
```

## Key Messaging

### DON'T say:
- "MCP error occurred"
- "Try restarting" (without explanation)
- "Check logs" (without guidance)
- "Run this command" (without context)

### DO say:
- "MCP server crashed because [specific reason]"
- "To fix, [specific steps with commands]"
- "Verify with: [command that proves it's fixed]"
- "This happens because [mechanism explanation]"

## For Extension Developers

If implementing error handling for MCP tools:

1. Catch tool execution errors
2. Classify the error (exit code, error type)
3. Generate diagnostic message using templates above
4. Display to user in structured format
5. Reference MCP_DIAGNOSTIC_GUIDE.md

Example (TypeScript):
```typescript
try {
  await invokeMcpTool(toolName, params);
} catch (error) {
  const errorType = classifyMcpError(error);
  const guidance = getMcpDiagnosticGuidance(errorType);
  renderer.displayError(error.message);
  renderer.displayInfo(guidance);
}
```

## References

- **Quick Reference:** `docs/mcp/MCP_AGENT_QUICK_REFERENCE.md`
- **Full Guide:** `docs/mcp/MCP_DIAGNOSTIC_GUIDE.md`
- **Implementation:** `src/agent/runloop/unified/mcp_support.rs`
- **Error Handling:** `src/agent/runloop/unified/async_mcp_manager.rs`
