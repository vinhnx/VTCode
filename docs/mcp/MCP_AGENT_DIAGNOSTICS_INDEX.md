# MCP Agent Diagnostics Index

**Central reference** for LLM agents diagnosing and explaining MCP tool failures to users.

## Quick Navigation

### For Fast Lookups
 **Start here:** [`docs/mcp/MCP_AGENT_QUICK_REFERENCE.md`](./mcp/MCP_AGENT_QUICK_REFERENCE.md)
- Error type → solution mapping
- One-line diagnoses
- Command cheat sheet

### For Detailed Guidance
 **Full diagnostics:** [`docs/mcp/MCP_DIAGNOSTIC_GUIDE.md`](./mcp/MCP_DIAGNOSTIC_GUIDE.md)
- 5 common MCP failure types
- Detailed diagnostics per error
- Decision tree for agents
- Implementation checklist

### For Agent Implementation
- Agent response templates
- Type-by-type response map
- Implementation checklist
- Key messaging guidelines

## Common Failure Types

| Exit Code / Error | Cause | Quick Fix |
|------------------|-------|----------|
| **127** | Not a shell command | Explain tool interface |
| **EPIPE** | Server crashed | Check config, restart |
| **Timeout** | Server slow | Increase timeout setting |
| **No such process** | Not installed/configured | Install package, verify config |
| **FAILURE** | Tool runtime error | Check params, verify tool |

## Usage by Role

### LLM Agent (Generating User Guidance)
1. Catch MCP tool failure
2. Identify error type from `MCP_AGENT_QUICK_REFERENCE.md`
4. Reference detailed guide if needed (`MCP_DIAGNOSTIC_GUIDE.md`)
5. Provide step-by-step diagnostics and fixes

### Extension Developer (Implementing Error Handling)
2. Classify errors using type categories
3. Generate diagnostic guidance
4. Display to user in structured format
5. Link to full documentation

### User (Troubleshooting)
1. Follow agent guidance (LLM-generated)
2. Run diagnostic commands
3. Apply fixes step-by-step
4. Verify with provided commands
5. Reference `MCP_DIAGNOSTIC_GUIDE.md` for deeper context

## Example: Time MCP Failure

### User sees:
```
$ mcp::time::get_current_time Asia/Tokyo
zsh: command not found: mcp::time::get_current_time
```

### Agent uses:
1. **Quick Reference:** Identifies "Exit 127" → "Not shell command"
2. **Failure Handling Guide:** Uses "Command Not Found" template
3. **Diagnostic Guide:** Provides detailed steps for time MCP specifically
4. **Generates response:**

```
MCP tools aren't shell commands. They're accessed via the tool interface, not through the shell.

To fix this:

1. Verify MCP server is installed:
   uvx mcp-server-time --help

2. Check .mcp.json configuration:
   cat .mcp.json | grep -A 3 "time"

3. Ensure MCP is enabled in vtcode.toml:
   grep "enabled = true" vtcode.toml

4. If missing, install:
   pip install mcp-server-time

5. Verify setup:
   vtcode doctor

The time tool will be available once the server starts.
```

## Document Map

```
docs/
 MCP_AGENT_DIAGNOSTICS_INDEX.md   ← This file
 mcp/
     MCP_AGENT_QUICK_REFERENCE.md  ← Quick lookup
     MCP_DIAGNOSTIC_GUIDE.md        ← Full diagnostics
     MCP_DIAGNOSTIC_GUIDE.md
     MCP_INITIALIZATION_TIMEOUT.md
     MCP_COMPLETE_IMPLEMENTATION_STATUS.md
     [other MCP docs]
```

## Key Principles for Agents

When diagnosing MCP failures:

 **DO:**
- Identify error type (exit code, error message)
- Explain in non-technical terms
- Provide diagnostic commands
- Show exact fix steps
- Verify after fix is applied
- Reference documentation

 **DON'T:**
- Suggest shell invocation syntax
- Generic "try again" advice
- Unexplained "restart" recommendations
- Reference logs without guidance
- Assume user knows what MCP is

## Integration Points

### Rust Code
- Error handling: `src/agent/runloop/unified/async_mcp_manager.rs`
- Status display: `src/agent/runloop/unified/mcp_support.rs`
- Tool execution: `src/agent/runloop/unified/tool_pipeline.rs`

### TypeScript Code
- Error messages: `vscode-extension/src/error/errorMessages.ts`
- Tool adaptation: `vscode-extension/src/mcpChatAdapter.ts`
- Health checks: `vscode-extension/src/mcp/enhancedMcpToolManager.ts`

## Testing Agent Responses

Before deploying agent diagnostics:

- [ ] Test error classification (maps to correct type)
- [ ] Verify commands work in target environment
- [ ] Check response clarity (non-technical language)
- [ ] Confirm actionability (user can follow steps)
- [ ] Validate fixes address root cause
- [ ] Test with common MCP servers (time, fetch, etc.)

## See Also

- Full MCP status: `docs/mcp/MCP_COMPLETE_IMPLEMENTATION_STATUS.md`
- Performance tuning: `docs/mcp/MCP_PERFORMANCE_BENCHMARKS.md`
- Timeout configuration: `docs/mcp/MCP_INITIALIZATION_TIMEOUT.md`
- Integration testing: `docs/mcp/MCP_INTEGRATION_TESTING.md`
