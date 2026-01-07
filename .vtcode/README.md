# VT Code Dynamic Context Directory

This directory contains dynamic context files for VT Code agent operations.

## Directory Structure

```
.vtcode/
  context/
    tool_outputs/     # Large tool outputs spooled to files
  history/            # Conversation history during summarization
  mcp/
    tools/            # MCP tool descriptions and schemas
    status.json       # MCP provider status
  skills/
    INDEX.md          # Available skills index
    {skill_name}/     # Individual skill directories
  terminals/
    INDEX.md          # Terminal sessions index
    {session_id}.txt  # Terminal session output
```

## Purpose

These files implement **dynamic context discovery** - a pattern where large outputs
are written to files instead of being truncated. This allows the agent to:

1. Retrieve full tool outputs on demand via `read_file`
2. Search through outputs using `grep_file`
3. Recover conversation details lost during summarization
4. Discover available skills and MCP tools efficiently

## Configuration

Configure in `vtcode.toml`:

```toml
[context.dynamic]
enabled = true
tool_output_threshold = 8192  # Bytes before spooling
sync_terminals = true
persist_history = true
sync_mcp_tools = true
sync_skills = true
```

---
*This directory is managed by VT Code. Files may be automatically created, updated, or cleaned up.*
