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
    "apply_patch".to_string(),
    "Apply a workspace-bound patch".to_string(),
    json!({"type": "object", "properties": {"input": {"type": "string"}}, "required": ["input"]}),
);
```

### OpenAI hosted tool search

For GPT-5.4-family Responses workflows, add `ToolDefinition::hosted_tool_search()` to the request and mark candidate functions with `.with_defer_loading(true)`.

Current VT Code scope for OpenAI:

- Supported: hosted `tool_search`
- Supported: deferred function schemas
- Supported in the Responses parser: OpenAI function-call namespaces and `tool_search_output` tool references
- Not yet modeled in shared tool definitions: MCP-server search surfaces

VT Code defers any MCP catalogue. For non-MCP catalogues, hosted search starts when at least 15 tools are deferable or their combined schema estimate exceeds about 4,000 tokens.

### When are tools deferred?

Deferral is decided per-catalog by `SessionToolCatalog::model_tools`. A tool is flagged `defer_loading = true` when **all** of the following hold:

1. The tool is not a core builtin (e.g. `exec_command`, `write_stdin`, or the active discovery tools) and is not listed in `always_available_tools`.
2. The session is not running under the always-eager TUI surface.
3. A deferral policy is active for the runtime (see below).

The deferral policy is active when **any** of these is true:

- **Anthropic** with `defer_by_default = true` (default): every non-core tool is deferred, including MCP tools.
- **OpenAI** Responses (`model_supports_responses_compaction`): hosted `tool_search` is injected and non-core tools are deferred.
- **Any provider** when `tools.client_tool_search = true` (default): client-local MCP deferral is enabled. Deferred MCP schemas are omitted from the request payload and replaced by a compact discoverability summary in the system prompt; `mcp_search_tools` expands matched schemas into the next request.

Key changes from earlier behavior:

- **MCP presence is the trigger.** Any MCP tool in the catalog is deferred regardless of tool count. MCP schemas are the dominant source of token inflation, so eager exposure is no longer attempted even for a single server.
- **Token-budget backstop.** A catalog is also deferred when its combined schema size exceeds ~4k tokens (≈16k chars), even if the tool count is below the numeric threshold. This catches single large servers whose schema dwarfs the whole builtin set.
- **Client-local is the default.** Providers without a hosted tool search (e.g. Gemini) now default to client-local deferral, so MCP schemas are not sent eagerly.

### Client-local deferred loading

When `tools.client_tool_search` is enabled and no provider-hosted search is available, VT Code:

1. Omits `defer_loading: true` tools from the wire payload.
2. Appends a compact, cache-stable summary of discoverable tools to the system prompt (names + one-line purpose).
3. Keeps `mcp_search_tools` available so a search match expands the tool's full schema into the next request.

Set `tools.client_tool_search = false` to restore the eager catalog for unsupported providers.

```toml
[tools]
client_tool_search = true
```

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

## Auditing first-request token cost

VT Code emits per-request telemetry so you can measure the token overhead the deferral mechanisms above remove. There is no separate "instruction file" field — instruction-file content is merged into the system prompt during assembly, so it is included in `system_prompt_tokens`.

### Per-request token breakdown

Each turn emits a `token_budget_breakdown` metric to the `vtcode.turn.metrics` tracing target (and the trajectory log), measured from the real assembled wire request:

| Field | Meaning |
|---|---|
| `system_prompt_tokens` | Composed system prompt (~4 chars/token estimate), including instruction-file content. |
| `tool_schema_tokens` | On-wire tool schema tokens. Under deferral, deferred MCP schemas are omitted, so this stays near the builtin baseline. |
| `message_history_tokens` | Text portion of the message history (lower-bound; non-text content not counted). |
| `on_wire_tools` | Number of tool definitions actually sent. |
| `client_local_deferral` | Whether client-local deferral omitted deferred schemas this request. |
| `tool_free_recovery` | Whether tools were stripped for a tool-free recovery pass. |

Cache read/write/miss counts are not duplicated here — they are surfaced via `SessionStats` prompt-cache diagnostics.

### Advisory warnings

Two one-time warnings flag token-overhead misconfiguration:

- **System prompt over budget** — when the composed prompt exceeds `agent.max_system_prompt_tokens` (default `8000`) and `agent.system_prompt_budget_warning` is on (default). Advisory unless `agent.trim_system_prompt` is enabled, in which case sections are dropped to fit.
- **Deferred loading disabled but beneficial** — when `tools.client_tool_search = false` and the catalog is large enough that deferral would engage (any MCP tool, ≥15 deferable tools, or combined schema > ~4k tokens). This warns that the full tool-schema tax is paid on every request and that re-enabling `client_tool_search` would omit the large/MCP schemas from the wire payload. Emitted once per process.

The count/schema-token thresholds *triggering* deferral when enabled are correct behavior, not warning conditions — they only warn when deferral is off and would have helped.

## Related Documentation

- [Anthropic Tool Search Documentation](https://docs.anthropic.com/claude/reference/tool-search)
- [OpenAI Tool Search Guide](https://developers.openai.com/api/docs/guides/tools-tool-search)
- [Configuration Reference](../config/CONFIGURATION_PRECEDENCE.md)
