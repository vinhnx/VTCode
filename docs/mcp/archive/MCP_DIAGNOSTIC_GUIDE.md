# MCP Diagnostic Guide for LLM Agents

This guide helps LLM agents understand and diagnose MCP (Model Context Protocol) failures and provide helpful guidance to users.

## Overview

MCP tools are **not shell commands**—they require:
1. MCP server process to be running
2. Proper tool interface invocation (not shell `mcp::*` syntax)
3. Configured MCP provider in `.mcp.json` or `vtcode.toml`
4. Active connection between client and MCP server

## Common MCP Failures and Diagnostics

### 1. **Command Not Found (Exit Code 127)**

**What happened:**
User attempted to invoke MCP tool as shell command (e.g., `mcp::time::get_current_time`).

**Diagnosis:**
```bash
# Shell executed this as a command:
$ mcp::time::get_current_time Asia/Tokyo
# Result: command not found (127)
```

**LLM Agent Response:**
```
MCP tools aren't shell commands. They're accessed via the tool interface, not through the shell.

To use the time MCP tool, you need to call it through the proper tool interface.
For example, don't use: mcp::time::get_current_time
Instead, the tool must be invoked through the MCP system when available.

For verification, check if the MCP server is installed:
  uvx mcp-server-time --help

If not installed:
  pip install mcp-server-time
  # or
  uv pip install mcp-server-time
```

### 2. **MCP Server Not Running (EPIPE - Broken Pipe)**

**What happened:**
Tool invocation attempted but MCP server process exited or crashed.

**Error indicators:**
- "Broken pipe" error
- "EPIPE" in error message
- MCP status shows "Error" or "Not connected"

**Diagnostics to run:**
```bash
# Check MCP status
cargo run -- mcp list

# Check if configured in vtcode.toml:
grep -A 5 "time" vtcode.toml

# Try starting MCP server manually:
uvx mcp-server-time
```

**LLM Agent Response:**
```
The MCP server process for 'time' exited unexpectedly (Broken pipe error).

This error often occurs when:
1. The MCP server process crashes on startup
2. The server process was killed or terminated
3. The server fails to handle tool requests

Diagnostic steps:
1. Check MCP configuration in .mcp.json:
   cat .mcp.json | grep -A 3 "time"

2. Verify the MCP server is installed:
   uvx mcp-server-time --help

3. Try running it manually to see startup errors:
   uvx mcp-server-time

4. Check process status:
   ps aux | grep mcp-server-time

5. Review MCP initialization settings in vtcode.toml:
   - startup_timeout_seconds (increase if server is slow)
   - log_level (set to "debug" for more details)

6. Run full diagnostics:
   cargo run -- doctor
```

### 3. **MCP Timeout on Initialization**

**What happened:**
MCP server took too long to start or respond.

**Error indicators:**
- "Timeout" in error message
- "Did not respond within X seconds"
- MCP status shows "Timeout"

**LLM Agent Response:**
```
The MCP server failed to initialize within the configured timeout.

This can happen when:
1. Server startup takes longer than configured timeout
2. Network issues or slow disk I/O
3. Resource constraints (CPU, memory)
4. Server is stuck or hanging

Solutions:
1. Increase startup timeout in vtcode.toml:
   [mcp]
   startup_timeout_seconds = 30  # default is usually 10-15

2. Check system resources:
   - Memory usage: free -h
   - CPU load: top -l 1 | head -20
   - Disk usage: df -h

3. Enable debug logging in vtcode.toml:
   [mcp]
   log_level = "debug"

4. Run the MCP server manually to check for issues:
   uvx mcp-server-time

5. Check if the provider is actually installed:
   uv pip list | grep mcp-server
```

### 4. **MCP Not Configured (No such process)**

**What happened:**
Tool attempted but MCP server wasn't properly configured or referenced.

**Error indicators:**
- "No such process"
- MCP status shows "Disabled" or "Not initialized"
- `.mcp.json` is missing or empty

**LLM Agent Response:**
```
The MCP provider is not properly configured or the server process failed to start.

Check your configuration:

1. Verify .mcp.json exists and is valid:
   cat .mcp.json

2. Expected format for time provider:
   {
     "mcpServers": {
       "time": {
         "command": "uvx",
         "args": ["mcp-server-time"],
         "type": "stdio"
       }
     }
   }

3. Verify the MCP server command is correct:
   - "command" should be either a full path or available in PATH
   - "args" should match the server's expected arguments
   - "type" should be "stdio" for standard input/output servers

4. Ensure vtcode.toml enables MCP:
   [mcp]
   enabled = true

5. Install the MCP server:
   pip install mcp-server-time
   # or
   uv pip install mcp-server-time

6. Verify it's accessible:
   which uvx
   uvx mcp-server-time --help
```

### 5. **MCP Tool Invocation Failed (Runtime Tool Error)**

**What happened:**
MCP server is running but specific tool invocation failed.

**Error indicators:**
- Tool status shows "FAILURE"
- Error message from tool execution
- "Tool error" or "Invocation error"

**Diagnostics:**
- Check tool parameters are correct
- Verify MCP server version supports the tool
- Check server logs for detailed error

**LLM Agent Response:**
```
The MCP tool invocation failed at runtime.

This means the MCP server is running, but the specific tool encountered an error.

Debugging steps:

1. Verify tool exists in the server:
   uvx mcp-server-time --list-tools
   # or check server documentation

2. Check tool parameters:
   - Required fields provided?
   - Correct format/type?
   - Valid values?

3. Check MCP server logs (if available):
   - Look for error messages
   - Enable verbose mode if available

4. Verify MCP server version is current:
   pip install --upgrade mcp-server-time

5. Test the tool manually if possible:
   uvx mcp-server-time
   # then try the tool directly

6. Check server compatibility:
   - Some tools may require specific server versions
   - Check .mcp.json for version requirements
```

## Diagnostic Workflow for Agents

When an MCP tool fails, follow this decision tree:

```
 Tool Invocation Failed
 Exit Code 127 ("command not found")
   → Explain MCP ≠ shell command
     → Show correct tool interface usage
     → Suggest manual server verification

 EPIPE / Broken Pipe
   → Server crashed or exited
     → Check .mcp.json configuration
     → Suggest manual server startup
     → Recommend timeout/logging tweaks

 Timeout
   → Server too slow to start
     → Increase startup_timeout_seconds
     → Check system resources
     → Enable debug logging

 "No such process"
   → Server not configured properly
     → Verify .mcp.json format
     → Check command path in PATH
     → Install MCP server package

 Runtime Tool Error (FAILURE status)
    → MCP server running, tool failed
      → Verify tool parameters
      → Check server version
      → Suggest manual tool testing
      → Recommend server documentation check
```

## Implementation Checklist for LLM Agents

When providing diagnostic guidance for MCP failures:

- [ ] **Identify error type** (command not found, timeout, EPIPE, config, runtime)
- [ ] **Explain why it happened** (clear, non-technical language)
- [ ] **Show exact diagnostic command** to verify the issue
- [ ] **Provide specific fix steps** (numbered, actionable)
- [ ] **Include config examples** (JSON/TOML snippets)
- [ ] **Suggest verification** after fix is applied
- [ ] **Link to relevant docs** (if appropriate)
- [ ] **Avoid shell command syntax** (explain tool interface vs shell)

## Common Diagnostic Commands

```bash
# Check MCP status
vtcode doctor

# List MCP providers
vtcode mcp list

# Check process status
ps aux | grep mcp-server-

# Verify server installation
pip list | grep mcp-server-
uv pip list | grep mcp-server-

# Test server startup
uvx mcp-server-time --help
uvx mcp-server-fetch --help

# Check PATH
echo $PATH
which uvx

# View configuration
cat .mcp.json
cat vtcode.toml | grep -A 10 "mcp"

# Check system resources
free -h        # Memory
df -h          # Disk
top -l 1       # CPU (macOS)
```

## For Documentation / Response Examples

### Good Example
```
The time MCP tool isn't available because the MCP server process isn't running.

MCP tools require:
1. A running MCP server process
2. Proper configuration in .mcp.json
3. The tool accessed through the MCP interface (not as a shell command)

To fix this:

1. Verify the server is installed:
   uvx mcp-server-time --help

2. Check your .mcp.json configuration is valid:
   cat .mcp.json | grep -A 3 "time"

3. If missing, run:
   pip install mcp-server-time

4. Verify MCP is enabled in vtcode.toml:
   [mcp]
   enabled = true

5. Try the doctor command:
   vtcode doctor

The time tool will be available once the server starts successfully.
```

### Bad Example (Avoid)
```
MCP error occurred. Try reinstalling. If that doesn't work, restart.
```

## Testing Your Diagnostics

When generating diagnostic guides:

1. **Verify commands work** in the target environment
2. **Test error scenarios** manually
3. **Check output clarity** (no jargon where possible)
4. **Confirm actionability** (user can follow steps)
5. **Validate fixes** address root cause

## References

- MCP Initialization: `docs/mcp/MCP_INITIALIZATION_TIMEOUT.md`
- MCP Status Display: `src/agent/runloop/unified/mcp_support.rs`
- Error Handling: `src/agent/runloop/unified/async_mcp_manager.rs`
- Full Integration: `docs/mcp/MCP_COMPLETE_IMPLEMENTATION_STATUS.md`
