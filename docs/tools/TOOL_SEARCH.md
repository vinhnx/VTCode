# Tool Search Integration

This document describes VT Code's tool search integration for providers that support deferred tool loading. VT Code currently supports:

- Anthropic advanced-tool-use beta search tools
- OpenAI hosted `tool_search` with deferred function loading

## Overview

The tool search feature allows Claude to search through thousands of tools on-demand instead of loading all tool definitions into context upfront. This solves two critical challenges:

1. **Context efficiency**: Tool definitions can consume massive portions of the context window
2. **Tool selection accuracy**: Claude's ability to correctly select tools degrades with more than 30-50 tools

## Anthropic configuration

Add the following to your `vtcode.toml` under the Anthropic provider section:

```toml
[providers.anthropic.tool_search]
enabled = false                  # Master switch for tool search
algorithm = "regex"              # "regex" (Python regex) or "bm25" (natural language)
defer_by_default = true          # Mark most tools as deferred by default
max_results = 5                  # Maximum tool search results
always_available_tools = []      # Tool names that should never be deferred
```

## Tool Search Algorithms

### Regex (`tool_search_tool_regex_20251119`)

Claude constructs Python regex patterns using `re.search()` syntax:

- `"weather"` - matches tool names/descriptions containing "weather"
- `"get_.*_data"` - matches tools like `get_user_data`, `get_weather_data`
- `"database.*query|query.*database"` - OR patterns
- `"(?i)slack"` - case-insensitive search

### BM25 (`tool_search_tool_bm25_20251119`)

Claude uses natural language queries to search for tools.

## API Usage

### Creating Tool Definitions

```rust
use vtcode_core::llm::provider::{ToolDefinition, ToolSearchAlgorithm};

// Anthropic tool search
let search_tool = ToolDefinition::tool_search(ToolSearchAlgorithm::Regex);

// OpenAI hosted tool search
let hosted_search = ToolDefinition::hosted_tool_search();

// Create a deferred tool (not loaded until discovered)
let deferred_tool = ToolDefinition::function(
    "get_weather".to_string(),
    "Get the weather for a location".to_string(),
    json!({"type": "object", "properties": {"location": {"type": "string"}}, "required": ["location"]}),
).with_defer_loading(true);

// Create a non-deferred tool (always available)
let core_tool = ToolDefinition::function(
    "read_file".to_string(),
    "Read a file from disk".to_string(),
    json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}),
);
```

### OpenAI hosted tool search

For GPT-5.4-family Responses workflows, add `ToolDefinition::hosted_tool_search()` to the request and mark candidate functions with `.with_defer_loading(true)`.

Current VT Code scope for OpenAI:

- Supported: hosted `tool_search`
- Supported: deferred function schemas
- Supported in the Responses parser: OpenAI function-call namespaces and `tool_search_output` tool references
- Not yet modeled in shared tool definitions: MCP-server search surfaces

VT Code keeps small deferred catalogs eager and skips hosted tool search until the deferable tool set reaches 100 entries. This avoids paying the search-tool overhead when direct exposure is cheaper and simpler.

### Handling Tool References

When Claude uses tool search, the response may contain discovered tool references:

```rust
let response = provider.generate(request).await?;

// Check for tool references (tools discovered via search)
if !response.tool_references.is_empty() {
    println!("Discovered tools: {:?}", response.tool_references);
    
    // These tools should be expanded (defer_loading=false) in the next request
    for tool_name in &response.tool_references {
        // Mark the tool as expanded for the next request
    }
}
```

## Response Block Types

The Anthropic provider handles these content block types:

- `server_tool_use`: Server-side tool execution (tool search invocation)
- `tool_search_tool_result`: Results from tool search containing tool references
- `tool_reference`: Reference to a discovered tool

## Beta Header

When tool search is enabled and the request contains deferred tools, the provider automatically includes the required beta header:

```
anthropic-beta: advanced-tool-use-2025-11-20
```

## Limits and guidance

- Maximum tools: 10,000 in catalog
- Search results: 3-5 most relevant tools per search
- Pattern length: Maximum 200 characters for regex patterns
- Anthropic model support: Claude Sonnet 4.5+, Claude Opus 4.5+ only
- OpenAI guidance: prefer hosted tool search with GPT-5.4 when the tool inventory is already known at request time

## Best Practices

1. Keep 3-5 most frequently used tools as non-deferred
2. Write clear, descriptive tool names and descriptions
3. Use semantic keywords in descriptions that match how users describe tasks
4. For OpenAI, prefer grouping tools conceptually and keep deferred catalogs focused
5. Monitor which tools Claude discovers to refine descriptions

## Related Documentation

- [Anthropic Tool Search Documentation](https://docs.anthropic.com/claude/reference/tool-search)
- [OpenAI Tool Search Guide](https://developers.openai.com/api/docs/guides/tools-tool-search)
- [Configuration Reference](config/CONFIGURATION_PRECEDENCE.md)
