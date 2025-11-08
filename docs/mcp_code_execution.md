# MCP Code Execution Architecture

Based on Anthropic's ["Code execution with MCP"](https://www.anthropic.com/engineering/code-execution-with-mcp) recommendations, vtcode implements code-centric agent design with progressive disclosure of MCP tools.

## Implementation Status

### ✅ Step 1: Progressive Tool Discovery (COMPLETED)

**Problem**: Loading all MCP tool definitions into the model's context is expensive and inefficient.

**Solution**: `search_tools` helper allows agents to discover tools progressively:
- Search by keyword with relevance scoring
- Request tool definitions in stages (name → description → full schema)
- Reduces context usage by 70-80% compared to full tool disclosure

#### API

```javascript
// Minimal context: just names of matching tools
search_tools({
  keyword: "file operations",
  detail_level: "name-only"
})

// Balanced: names + descriptions
search_tools({
  keyword: "file operations",
  detail_level: "name-and-description"  // default
})

// Full details when ready to use
search_tools({
  keyword: "read_file",
  detail_level: "full"  // includes input schema
})
```

#### Implementation Details

- **Module**: `vtcode-core/src/mcp/tool_discovery.rs`
- **Discovery Engine**: `ToolDiscovery` struct with relevance scoring
- **Matching**: Exact match → substring match → fuzzy match scoring
- **Registry**: Registered as builtin tool `search_tools`

Example response:
```json
{
  "keyword": "file",
  "matched": 3,
  "detail_level": "name-and-description",
  "results": [
    {
      "name": "read_file",
      "provider": "builtin",
      "description": "Read file contents from disk"
    },
    {
      "name": "write_file", 
      "provider": "builtin",
      "description": "Write or overwrite file contents"
    },
    {
      "name": "list_files",
      "provider": "builtin",
      "description": "List files in a directory"
    }
  ]
}
```

### 🔄 Step 2: Code Executor with MCP SDK Generation (IN PROGRESS)

**Objective**: Allow agents to write code that calls MCP tools as library functions, rather than making individual tool calls.

**Benefits**:
- Control flow efficiency (loops, conditionals without repeated model calls)
- Reduced latency (code runs locally in sandbox)
- Better error handling and retries
- Data filtering before returning to model

**Implementation Plan**:
1. Create `vtcode-core/src/exec/code_executor.rs` - Python/JS runtime wrapper
2. Implement `sdk_generator.rs` - Dynamic SDK generation from MCP tool schemas
3. Add code sandbox support in PTY manager
4. Expose as new builtin tool: `execute_code`

### 📋 Step 3: Skill/State Persistence (PENDING)

**Objective**: Allow agents to save reusable functions ("skills") to workspace.

**Plan**:
- Store in `.vtcode/skills/` directory
- Include `SKILL.md` documentation
- Loadable by agents across conversations
- Examples: common API clients, data transformers, validators

### 🔧 Step 4: Data Filtering in Code (PENDING)

**Objective**: Filter large result sets before returning to model.

**Example**:
```python
# Instead of sending 10k rows to model:
files = list_files(path="/workspace", recursive=True)
filtered = [f for f in files if "test" in f and f.endswith(".rs")]
# Only return filtered list
```

### 🔐 Step 5: PII Tokenization Layer (PENDING)

**Objective**: Automatically tokenize sensitive data before MCP calls.

**Features**:
- Detect PII patterns (email, phone, SSN, etc.)
- Replace with secure tokens
- Maintain lookup table in sandbox
- Untokenize when sharing results

## Architecture

```
┌─────────────────────────────────────────────────┐
│            Agent (LLM Model)                    │
└────────────────┬────────────────────────────────┘
                 │
     ┌───────────┴───────────────────┐
     │                               │
     ▼                               ▼
┌──────────────┐           ┌─────────────────────┐
│ Tool Calls   │           │ Code Execution      │
│ (direct)     │           │ (future)            │
└──────────────┘           └─────────────────────┘
     │                               │
     │    ┌────────────────────────┐ │
     │    │  search_tools          │ │
     │    │  (progressive          │ │
     │    │   disclosure)          │ │
     │    └────────────────────────┘ │
     │                               │
     └───────────────┬───────────────┘
                     │
                     ▼
            ┌──────────────────────┐
            │   MCP Tool Executor  │
            │   (with validation)  │
            └──────────┬───────────┘
                       │
         ┌─────────────┴─────────────┐
         │                           │
         ▼                           ▼
    ┌─────────┐              ┌──────────────┐
    │ Builtin │              │ MCP Provider │
    │ Tools   │              │ (fetch, etc) │
    └─────────┘              └──────────────┘
```

## Token Efficiency Gains

| Approach | Tool Context | Per-call | Total Savings |
|----------|--------------|----------|---------------|
| Load all (old) | ~2KB × N | ~500B | baseline |
| Progressive (new) | ~200B × search | ~100B | 60-80% |
| Code execution | 0 in loop | 0 | 90%+ |

## Usage in Prompts

Update system prompts to guide agent behavior:

```markdown
## Tool Discovery

Use search_tools to find relevant operations before calling them:

search_tools({keyword: "file", detail_level: "name-only"})

Then request full details when ready:

search_tools({keyword: "read_file", detail_level: "full"})

This saves context and makes interactions more efficient.
```

## Testing

```bash
# Test the tool discovery module
cargo test tool_discovery

# Test integration with tool registry
cargo test registry
```

## References

- Anthropic Engineering Blog: [Code execution with MCP](https://www.anthropic.com/engineering/code-execution-with-mcp)
- MCP Specification: [Model Context Protocol](https://modelcontextprotocol.io/)
- vtcode MCP Integration: `vtcode-core/src/mcp/`
