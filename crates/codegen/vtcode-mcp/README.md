# vtcode-mcp

Model Context Protocol (MCP) client, connection pooling, and tool discovery
for VT Code. Extracted from `vtcode-core` to isolate the MCP subsystem into an
independently compilable crate.

<!-- cargo-rdme start -->

Model Context Protocol (MCP) client management built on top of the Codex MCP building blocks.

This crate adapts the reference MCP client, server and type
definitions from <https://github.com/openai/codex> to integrate them
with VT Code's multi-provider configuration model. The original
implementation inside this project had grown organically and mixed a
large amount of bookkeeping logic with the lower level rmcp client
transport. The rewritten version keeps the VT Code specific surface
(allow lists, tool indexing, status reporting) but delegates the
actual protocol interaction to a lightweight `RmcpClient` adapter
that mirrors Codex' `mcp-client` crate. This dramatically reduces
the amount of bespoke glue we have to maintain while aligning the
behaviour with the upstream MCP implementations.

<!-- cargo-rdme end -->

## Modules

| Module | Purpose |
|---|---|
| `client` | MCP client lifecycle and provider management |
| `connection_pool` | Connection pooling for MCP providers |
| `enhanced_config` | Enhanced MCP configuration with validation |
| `errors` | MCP-specific error types |
| `provider` | MCP provider connection and interaction |
| `rmcp_client` | Low-level rmcp protocol adapter |
| `rmcp_transport` | HTTP and stdio transport layers |
| `schema` | JSON Schema validation for tool inputs |
| `tool_discovery` | Dynamic tool discovery from MCP providers |
| `tool_discovery_cache` | Caching for discovered tools |
| `traits` | `McpToolExecutor` and `McpElicitationHandler` traits |
| `types` | MCP protocol types (tools, prompts, resources) |
| `utils` | Timezone injection, header building, schema helpers |

## Public entrypoints

- `McpClient` -- manage MCP providers and invoke tools
- `McpProvider` -- single provider connection
- `McpConnectionPool` / `PooledMcpManager` -- connection pooling
- `ToolDiscovery` / `ToolDiscoveryResult` -- dynamic tool discovery
- `McpToolExecutor` / `McpElicitationHandler` -- trait interfaces
- `validate_mcp_config` -- configuration validation

## Features

| Feature | Description |
|---|---|
| `schema` | JSON Schema validation via `schemars` |

## Usage

```rust
use vtcode_mcp::{McpClient, McpToolInfo};

let client = McpClient::new(config);
let tools = client.list_tools().await?;
```

## API reference

<https://docs.rs/vtcode-mcp>
