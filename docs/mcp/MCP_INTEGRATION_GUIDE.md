# MCP Integration Guide for VT Code

This document outlines how VT Code implements Model Context Protocol (MCP) based on Claude's official MCP specifications and best practices.

## Overview

VT Code integrates MCP to connect with external tools, databases, and APIs through the `vtcode-core/src/mcp/` module. MCP provides a standardized interface for AI agents to access tools beyond their native capabilities.

## Architecture

### Core Components

**Location**: `vtcode-core/src/mcp/`

```
mcp/
├── mod.rs                    # Main MCP client and provider management
├── cli.rs                    # MCP CLI commands and user interaction
├── connection_pool.rs        # Connection management (disabled - needs updates)
├── enhanced_config.rs        # Enhanced configuration handling
├── errors.rs                 # Error types and handling
├── rmcp_transport.rs         # Transport layer (stdio, HTTP, child process)
├── schema.rs                 # JSON schema validation
├── tool_discovery.rs         # Tool discovery and caching
└── tool_discovery_cache.rs   # Advanced caching (disabled - needs updates)
```

### Key Abstractions

**McpClient** (`mod.rs:176`)
- High-level client managing multiple MCP providers
- Enforces VT Code policies (tool allowlists, security validation)
- Handles provider lifecycle (initialize, shutdown, tool execution)

**McpProvider**
- Individual provider connection
- Handles MCP handshake and protocol communication
- Manages tool, resource, and prompt discovery

**McpToolExecutor** (trait, `mod.rs:167`)
- Interface for the tool registry to execute MCP tools
- Enables integration with VT Code's native tool system

## Configuration

### Configuration Files

**Project-Scoped** (checked into source control):
```json
// .mcp.json
{
  "mcpServers": {
    "server_name": {
      "command": "npx",
      "args": ["mcp-server-name"],
      "type": "stdio",
      "env": {
        "API_KEY": "value"
      }
    }
  }
}
```

**User-Scoped** (personal, cross-project):
```
~/.claude.json
// Under "mcpServers" field
```

**Runtime Configuration**:
```toml
# vtcode.toml
[mcp]
enabled = true
startup_timeout_seconds = 10
tool_timeout_seconds = 30
experimental_use_rmcp_client = true

[[mcp.providers]]
name = "fetch"
enabled = true
transport = { type = "stdio", command = "uvx", args = ["mcp-server-fetch"] }
max_concurrent_requests = 1
```

### Configuration Precedence

1. **Environment Variables** (highest priority)
   - `MCP_TIMEOUT`: Startup timeout in milliseconds (default: 10000)
   - `MAX_MCP_OUTPUT_TOKENS`: Maximum output token limit (default: 25000)
   - API keys and authentication tokens
   - Custom timezone via `VT_LOCAL_TIMEZONE`

2. **vtcode.toml** (runtime configuration)
   - MCP enablement
   - Timeout settings
   - Provider definitions
   - Security policies

3. **Code Constants** (`vtcode-core/src/config/constants.rs`)
   - Default values
   - Built-in limits

## Transport Types

VT Code supports three transport mechanisms for MCP servers:

### 1. Stdio Transport

```toml
[[mcp.providers]]
name = "example"
transport = { 
  type = "stdio", 
  command = "npx",
  args = ["mcp-server-example"],
  working_directory = "/path/to/dir"
}
```

**Implementation**: `rmcp_transport.rs:create_stdio_transport()`
- Direct process communication via stdin/stdout
- Best for local tools and development

### 2. HTTP Transport

```toml
[[mcp.providers]]
name = "remote-api"
transport = { 
  type = "http",
  url = "https://mcp.example.com/api"
}
```

**Implementation**: `rmcp_transport.rs:create_http_transport()`
- Requires `experimental_use_rmcp_client = true`
- Remote server integration
- OAuth 2.0 authentication support (via `/mcp` CLI command)

### 3. Child Process Transport (Advanced)

Used internally for managed stdio connections with enhanced lifecycle control.

## Security

### Validation Framework

**Argument Validation** (`mod.rs:312`):
```rust
pub fn validate_tool_arguments(&self, _tool_name: &str, args: &Value) -> Result<()>
```

Checks:
- **Size Limits**: `max_argument_size` enforcement
- **Path Traversal Protection**: Blocks `../` and `..\\` patterns
- **Schema Validation**: Enforces JSON schema compliance

### Allow Lists

**Configuration**:
```toml
[mcp.allowlist]
tools = ["allowed_tool_1", "allowed_tool_2"]
resources = ["resource:uri:*"]
prompts = ["prompt_name"]
```

**Enforcement**:
- Tools not in allowlist are rejected
- Allowlists can use glob patterns
- Per-provider granularity supported

### Access Control

**Token Management**:
- Stored securely on disk
- Automatically refreshed
- Revocable via `/mcp` CLI

**API Key Handling**:
- Read from environment variables (recommended)
- Never hardcoded
- Per-provider configuration

## Tool Discovery & Execution

### Discovery Process

**Phase 1: List Tools**
```rust
// Called during provider initialization
pub async fn list_tools(&self) -> Result<Vec<McpToolInfo>>
```

Returns:
- Tool name
- Description
- Input schema (JSON Schema)
- Provider source

**Phase 2: Cache Tools**
```rust
pub async fn refresh_tools(&self, allowlist: &McpAllowListConfig, timeout: Duration) -> Result<()>
```

- Applies allowlist filtering
- Caches tool metadata
- Validates input schemas

### Tool Execution

**Execution Flow** (`mod.rs:344`):
1. **Validation**: Arguments checked against security policies
2. **Resolution**: Determine which provider owns the tool
3. **Execution**: Call tool on the appropriate provider
4. **Formatting**: Standard result format applied

**Timeout Handling**:
```rust
async fn run_with_timeout<F, T>(
    fut: F,
    timeout: Option<Duration>,
    label: &str
) -> Result<T>
```

- Per-provider startup timeout
- Per-tool execution timeout
- Configurable via `vtcode.toml` or environment

## Resources & Prompts

### Resource Management

**API**:
```rust
pub async fn list_resources(&self) -> Result<Vec<McpResourceInfo>>
pub async fn read_resource(&self, uri: &str) -> Result<McpResourceData>
```

**Features**:
- Lazy loading of resource contents
- URI-based resource identification
- MIME type support
- Size metadata

**Usage in VT Code**:
- File system resources (`file://` URIs)
- Database resources
- API responses
- Referenced via `@` mentions

### Prompt Templates

**API**:
```rust
pub async fn list_prompts(&self) -> Result<Vec<McpPromptInfo>>
pub async fn get_prompt(
    &self, 
    prompt_name: &str, 
    arguments: Option<HashMap<String, String>>
) -> Result<McpPromptDetail>
```

**Features**:
- Parameterized prompts
- Multiple message formats
- Argument validation
- Template rendering

## Event Handling & Notifications

### Notification Types

**From MCP Provider to VT Code**:

1. **Logging** (`on_logging_message`)
   - Standard logging integration
   - Severity levels: debug, info, warning, error

2. **Progress** (`on_progress`)
   - Long-running operation feedback
   - Token-based tracking
   - Message and percentage updates

3. **Resource Updates** (`on_resource_updated`, `on_resource_list_changed`)
   - File/data modifications
   - Cache invalidation signals

4. **Tool List Changes** (`on_tool_list_changed`)
   - Dynamic tool availability
   - Discovery cache refresh triggers

5. **Prompt List Changes** (`on_prompt_list_changed`)
   - Template updates
   - Cache invalidation

### Elicitation (User Interaction)

**Purpose**: MCP providers can request user input during tool execution

**Flow**:
```rust
pub struct McpElicitationRequest {
    pub message: String,
    pub requested_schema: Value,  // JSON Schema for expected input
}

pub struct McpElicitationResponse {
    pub action: ElicitationAction,  // Accept, Reject, or Cancel
    pub content: Option<Value>,     // User response
}
```

**Implementation**:
- Custom `McpElicitationHandler` trait
- Schema-based validation
- Examples: interactive authentication, file selection

## Integration with VT Code Tools

### Tool Registry Integration

MCP tools are exposed through VT Code's `McpToolExecutor` trait:

```rust
pub async fn execute_mcp_tool(&self, tool_name: &str, args: &Value) -> Result<Value>
pub async fn list_mcp_tools(&self) -> Result<Vec<McpToolInfo>>
pub async fn has_mcp_tool(&self, tool_name: &str) -> Result<bool>
fn get_status(&self) -> McpClientStatus
```

### Tool Naming Convention

- Format: `{provider_name}:{tool_name}`
- Example: `fetch:get_url`, `postgres:query`
- Enables multi-provider tool deduplication

## CLI Commands

**Available Commands** (`cli.rs`):

```bash
# List configured MCP servers
/mcp list

# Add a new server
/mcp add <name> <command> [args...]

# Configure server
/mcp config <name> --env KEY=value

# Authenticate remote server
/mcp authenticate <name>

# Remove server
/mcp remove <name>

# Status and diagnostics
/mcp status
```

## Performance & Optimization

### Connection Pooling

**Design Pattern** (`connection_pool.rs` - disabled):
- Reuse connections across tool calls
- Semaphore-based concurrency control (per provider)
- Automatic cleanup and timeout handling

**Current Implementation**:
```rust
pub struct McpProvider {
    // ...
    semaphore: Arc<Semaphore>,  // max_concurrent_requests
}
```

### Caching Strategy

**Tool Metadata Caching** (`tool_discovery.rs`):
- Cache tools during initialization
- Invalidate on `tool_list_changed` notification
- Manual refresh via `refresh_tools()`

**Resource Caching**:
- Lazy loading (fetch only when accessed)
- No persistent caching by default
- Per-request caching available

### Output Token Management

**Limits**:
- Default max: 25,000 tokens per tool call
- Configurable via `MAX_MCP_OUTPUT_TOKENS` environment variable
- Warning threshold: 10,000 tokens
- Truncation or pagination recommended for large outputs

## Enterprise Configuration

### Managed MCP Configuration

**For IT Administrators**:

```json
// /Library/Application Support/ClaudeCode/managed-mcp.json (macOS)
// /etc/claude-code/managed-mcp.json (Linux/WSL)
// C:\Program Files\ClaudeCode\managed-mcp.json (Windows)
{
  "mcpServers": {
    "approved_server": { /* config */ }
  }
}
```

**Effects**:
- Takes exclusive control over MCP servers
- Users cannot add or modify servers
- Requires administrator privileges to deploy

### Policy-Based Control

**Allowlist/Denylist** (in managed settings):

```json
{
  "allowedMcpServers": [
    { "name": "fetch" },
    { "command": "uvx mcp-server-*" },
    { "serverUrl": "https://mcp.company.com/*" }
  ],
  "deniedMcpServers": [
    { "name": "dangerous_server" }
  ]
}
```

**Matching Rules**:
- Name-based: Exact match against server name
- Command-based: Pattern match against command
- URL-based: Wildcard patterns for remote servers
- Denylist has absolute precedence

## Troubleshooting

### Common Issues

**1. Connection Timeout**
- Check `MCP_TIMEOUT` environment variable
- Verify server startup time
- Review provider `startup_timeout_ms` configuration

**2. Tool Not Found**
- Ensure tool is in allowlist
- Verify provider initialization succeeded
- Check provider logs for discovery errors

**3. Large Output Warnings**
- Increase `MAX_MCP_OUTPUT_TOKENS` if needed
- Configure server pagination/filtering
- Review output size in tool design

**4. Authentication Failures**
- Use `/mcp authenticate <name>` for OAuth
- Verify API keys in environment variables
- Check token refresh status

### Diagnostic Commands

```bash
# Check MCP status
cargo run -- /mcp status

# View active providers
cargo run -- /mcp list

# Test specific tool
cargo run -- ask "Use fetch tool to get https://example.com"

# View debug logs
RUST_LOG=debug cargo run
```

## Extension Points

### Custom Elicitation Handler

```rust
struct MyElicitationHandler;

#[async_trait]
impl McpElicitationHandler for MyElicitationHandler {
    async fn handle_elicitation(
        &self,
        provider: &str,
        request: McpElicitationRequest,
    ) -> Result<McpElicitationResponse> {
        // Implement custom user interaction logic
        Ok(McpElicitationResponse {
            action: ElicitationAction::Accept,
            content: Some(json!({})),
        })
    }
}

// Register with client
mcp_client.set_elicitation_handler(Arc::new(MyElicitationHandler));
```

### Custom Transport

Implement additional transport types by extending `rmcp_transport.rs`:
- WebSocket connections
- gRPC endpoints
- Custom protocols

## Best Practices

1. **Security First**
   - Always use allowlists in production
   - Validate all external tool inputs
   - Never hardcode API keys
   - Use environment variables for secrets

2. **Error Handling**
   - Always use `.with_context()` for error messages
   - Never use `.unwrap()` in production code
   - Provide detailed error messages to users

3. **Performance**
   - Set appropriate timeout values based on tool behavior
   - Use resource pagination for large datasets
   - Monitor token usage with `MAX_MCP_OUTPUT_TOKENS`

4. **Configuration Management**
   - Use `.mcp.json` for project-specific servers
   - Use `~/.claude.json` for personal utilities
   - Use `vtcode.toml` for runtime configuration
   - Document custom servers in project README

5. **Testing**
   - Mock MCP providers in unit tests
   - Integration tests with real providers
   - Test timeout and error scenarios
   - Validate allowlist enforcement

## Related Documentation

- **MCP Official**: https://modelcontextprotocol.io/
- **Claude MCP Docs**: https://code.claude.com/docs/en/mcp
- **VT Code Architecture**: `docs/ARCHITECTURE.md`
- **Configuration Guide**: `docs/config/CONFIGURATION_PRECEDENCE.md`
- **Security Model**: `docs/SECURITY_MODEL.md`

## References

- `vtcode-core/src/mcp/mod.rs`: Main MCP client implementation (2200+ lines)
- `vtcode-core/src/mcp/rmcp_transport.rs`: Transport abstractions
- `vtcode-core/src/mcp/schema.rs`: JSON schema validation
- `vtcode-core/src/mcp/tool_discovery.rs`: Tool discovery and caching
- `vtcode-core/src/config/mcp.rs`: Configuration types
