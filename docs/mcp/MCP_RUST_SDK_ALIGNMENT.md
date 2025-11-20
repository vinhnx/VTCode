# VTCode MCP Implementation Alignment with Official Rust SDK

Review of vtcode MCP implementation against official `rmcp` (Rust SDK) best practices from:
- https://github.com/modelcontextprotocol/rust-sdk (rmcp v0.9.0+)
- https://modelcontextprotocol.io specification and llms.txt

## Executive Summary

VTCode's MCP client implementation is well-structured and follows most MCP specification requirements. This document identifies alignment gaps and optimization opportunities to match official Rust SDK patterns.

**Current Status:**
- ✅ Core client implementation functional
- ✅ Multiple transport support (stdio, HTTP)
- ✅ Tool discovery with progressive disclosure
- ✅ Configuration management via TOML
- ⚠️ Minor alignment opportunities with RMCP patterns
- ⚠️ Async lifecycle management could use RMCP patterns

---

## 1. Architecture Alignment

### VTCode Current Pattern
```
McpClient (high-level)
  ├── McpProvider (per-provider wrapper)
  ├── RmcpClient (RMCP adapter)
  └── ToolDiscovery (tool management)
```

### RMCP Official Pattern
```
ServiceExt trait (protocol-agnostic)
  ├── Transport (stdin/stdout, HTTP, WebSocket)
  ├── Handler (ServerHandler, ClientHandler)
  └── Schema (JsonSchema for tools)
```

### Recommendation: Adopt RMCP's ServiceExt Trait

**Current code** (`vtcode-core/src/mcp/mod.rs`):
```rust
pub struct McpClient {
    providers: HashMap<String, McpProvider>,
    config: McpClientConfig,
}

impl McpClient {
    pub async fn initialize(&mut self) -> Result<()> { ... }
    pub async fn list_tools(&self, provider: &str) -> Result<Vec<Tool>> { ... }
}
```

**Better pattern** (aligned with RMCP):
```rust
// Use ServiceExt for both client and server
use rmcp::ServiceExt;

pub struct VTCodeMcpClient {
    transport: TokioChildProcess,
    config: McpClientConfig,
}

impl VTCodeMcpClient {
    // Leverage ServiceExt methods instead of custom
    async fn serve(self) -> Result<ManagedClient> {
        self.transport.serve().await
    }
}
```

**Action Items:**
1. Review `rmcp` crate features: `["server"]` vs `["client"]`
2. Consider using `ServiceExt` trait methods directly for initialization
3. Simplify custom `initialize()` logic by delegating to RMCP

---

## 2. Transport Configuration

### Current VTCode Transport
**File:** `vtcode-config/src/mcp.rs`

```rust
pub enum McpTransport {
    Stdio(StdioConfig),
    Http(HttpConfig),
}

pub struct StdioConfig {
    command: String,
    args: Vec<String>,
}
```

### RMCP Transport Pattern

```rust
// RMCP uses:
TokioChildProcess::new(Command::new("npx").arg("..."))
// or
HttpTransport::connect("http://...")
```

### Alignment Issues

| Issue | Current | RMCP | Action |
|-------|---------|------|--------|
| **Process Spawning** | Manual `Command` construction | `TokioChildProcess` wrapper | Wrap Command in `TokioChildProcess` |
| **Environment Variables** | Manual handling | Implicit in tokio | Review env var injection |
| **Timeout Handling** | Custom timeout logic | RMCP handles via `ServiceExt` | Leverage RMCP timeout support |
| **Error Handling** | Custom error types | RMCP uses `anyhow::Result` | Migrate to `anyhow` for consistency |

### Recommendation: Use RMCP Transport Wrappers

**Better code:**
```rust
use rmcp::transport::{TokioChildProcess, ConfigureCommandExt};
use tokio::process::Command;

async fn create_stdio_transport(config: &StdioConfig) -> Result<TokioChildProcess> {
    TokioChildProcess::new(
        Command::new(&config.command)
            .args(&config.args)
            .configure(|cmd| {
                // Set environment variables here
                for (k, v) in &config.env {
                    cmd.env(k, v);
                }
            })
    ).context("Failed to create child process")
}
```

**Current code replacement location:** `vtcode-core/src/mcp/mod.rs` constructor

---

## 3. Schema & Tool Definition

### Current VTCode Pattern
**File:** `vtcode-core/src/mcp/tool_discovery.rs`

```rust
pub struct ToolDescription {
    name: String,
    description: Option<String>,
    input_schema: Option<JsonValue>,
}
```

### RMCP Pattern

```rust
// Uses JSON Schema 2020-12 with schemars crate
#[derive(JsonSchema, Serialize)]
pub struct ToolInput {
    // Automatically generates schema
}
```

### Alignment Issues

| Aspect | Current | RMCP | Gap |
|--------|---------|------|-----|
| **Schema Version** | Unspecified | JSON Schema 2020-12 | Need explicit version in validation |
| **Schema Generation** | Manual JSON | `schemars` proc macro | Not using type-safe generation |
| **Schema Validation** | Basic checks | Full JSON Schema validation | Missing advanced features |
| **Tool Metadata** | Basic fields | Full `Tool` struct from spec | Missing optional fields |

### Recommendation: Integrate schemars for Schema Generation

**Add to Cargo.toml:**
```toml
[dependencies]
schemars = "0.8"
```

**Better tool definition:**
```rust
use schemars::{JsonSchema, json_schema};
use serde::{Deserialize, Serialize};

#[derive(JsonSchema, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    #[serde(flatten)]
    pub input_schema: JsonSchema,  // Auto-validated
}

impl McpTool {
    pub fn generate_schema() -> Value {
        json_schema!(Self)  // Type-safe schema generation
    }
}
```

---

## 4. Async Initialization Lifecycle

### Current VTCode Pattern
**File:** `src/agent/runloop/unified/async_mcp_manager.rs`

```rust
pub enum McpInitStatus {
    Disabled,
    Initializing { progress: String },
    Ready { client: McpClient },
    Error { message: String },
}

impl AsyncMcpManager {
    pub async fn start_initialization(&mut self) {
        // Custom async task spawning
    }
}
```

### RMCP Pattern

```rust
// Simplified lifecycle via ServiceExt
let client = transport.serve().await?;

// Lifecycle is handled by the runtime
match client.get_status() {
    ConnectionStatus::Connected => { ... }
    ConnectionStatus::Connecting => { ... }
}
```

### Alignment Issues

1. **Initialization Model**: VTCode uses manual state machine; RMCP uses trait-based lifecycle
2. **Progress Reporting**: VTCode custom; RMCP delegates to handler
3. **Error Recovery**: VTCode stores error state; RMCP propagates errors

### Recommendation: Simplify with RMCP Lifecycle

**Better approach:**
```rust
use rmcp::ServiceExt;

pub struct AsyncMcpManager {
    transport_config: TransportConfig,
    client: Option<ManagedClient>,
    init_task: Option<JoinHandle<Result<ManagedClient>>>,
}

impl AsyncMcpManager {
    pub async fn start_initialization(mut self) -> Result<()> {
        let transport = self.create_transport().await?;
        
        // Use RMCP's ServiceExt directly
        let client = transport.serve().await?;
        self.client = Some(client);
        Ok(())
    }
    
    pub async fn get_status(&self) -> ConnectionStatus {
        self.client
            .as_ref()
            .map(|c| c.get_status())
            .unwrap_or(ConnectionStatus::Disconnected)
    }
}
```

---

## 5. Tool Invocation & Execution

### Current VTCode Pattern
**File:** `vtcode-core/src/tools/registry/mod.rs`

```rust
pub async fn execute_mcp_tool(
    provider: &str,
    tool_name: &str,
    params: JsonValue,
) -> Result<ToolResult> {
    // Custom execution pipeline
}
```

### RMCP Pattern

```rust
// Handled via request/response cycle
let result = client.call_tool(ToolCall {
    name: tool_name.to_string(),
    arguments: params,
}).await?;
```

### Alignment Issues

1. **Request Format**: VTCode uses raw JSON; RMCP uses typed structs
2. **Error Handling**: Different error models
3. **Streaming Support**: RMCP supports streaming; VTCode needs review

### Recommendation: Use RMCP's Call Types

**Better code:**
```rust
use rmcp::client::calls::ToolCall;

pub async fn execute_mcp_tool(
    client: &ManagedClient,
    provider: &str,
    tool_name: &str,
    arguments: JsonValue,
) -> Result<ToolResult> {
    // Build typed request
    let call = ToolCall {
        name: tool_name.to_string(),
        arguments,
    };
    
    // Use RMCP client's typed methods
    let result = client.call_tool(call).await?;
    
    Ok(result)
}
```

**Update location:** `vtcode-core/src/tools/registry/executors.rs`

---

## 6. Error Handling & Result Types

### Current VTCode Pattern
**File:** `vtcode-core/src/mcp/mod.rs`

```rust
pub enum McpError {
    InitializationFailed(String),
    ToolNotFound(String),
    InvalidSchema(String),
    TransportError(String),
}

impl From<McpError> for anyhow::Error { ... }
```

### RMCP Pattern

```rust
// Uses anyhow::Result directly
pub async fn initialize() -> anyhow::Result<Client> {
    // Errors propagate with .context()
}
```

### Alignment Issues

1. **Error Types**: VTCode has custom enum; RMCP uses `anyhow`
2. **Context**: VTCode loses context; RMCP preserves with `.context()`
3. **Consistency**: Mixing error models across codebase

### Recommendation: Unified anyhow Error Handling

**Current:**
```rust
match result {
    Err(McpError::ToolNotFound(name)) => { ... }
}
```

**Better:**
```rust
use anyhow::{anyhow, Context};

// Simpler error propagation
let tool = client.find_tool(name)
    .ok_or_else(|| anyhow!("Tool not found: {}", name))
    .context("Failed to lookup tool")?;
```

**Action:** Update `vtcode-core/src/mcp/mod.rs` error handling

---

## 7. Tool Discovery Progressive Disclosure

### Current VTCode Pattern ✅
**File:** `vtcode-core/src/mcp/tool_discovery.rs`

```rust
pub enum DetailLevel {
    NameOnly,           // Just names
    NameAndDescription, // + descriptions
    Full,               // + input schemas
}

pub fn search_tools(detail_level: DetailLevel) -> Vec<ToolDescription> { ... }
```

### RMCP Alignment
This is **well-designed** and matches RMCP principles of progressive context disclosure.

### Recommendation: Keep as-is
No changes needed. This pattern is optimal for agent context management.

---

## 8. Configuration Management

### Current VTCode Pattern ✅
**File:** `vtcode-config/src/mcp.rs`

```rust
pub struct McpClientConfig {
    pub enabled: bool,
    pub providers: HashMap<String, McpProviderConfig>,
    pub startup_timeout_seconds: u64,
}
```

### RMCP Alignment
RMCP doesn't dictate config format; stdio-based servers expect JSON on stdio.

### Recommendation: Add TOML Config Examples

**Add to `vtcode.example.toml`:**
```toml
[mcp]
enabled = true
startup_timeout_seconds = 15
log_level = "info"

[[mcp.providers]]
name = "time"
command = "uvx"
args = ["mcp-server-time"]
env = { TZ = "UTC" }

[[mcp.providers]]
name = "filesystem"
command = "uvx"
args = ["mcp-server-filesystem", "/Users/john/work"]
```

---

## 9. Provider Health & Connection Management

### Current VTCode Gap ⚠️
**Missing Feature**: Health checks for provider connections

### RMCP Pattern
Supports `ping` requests to verify connection health

### Recommendation: Add Health Check Support

**New file:** `vtcode-core/src/mcp/health.rs`

```rust
pub async fn check_provider_health(client: &ManagedClient) -> Result<()> {
    use rmcp::client::calls::Ping;
    
    client.send(Ping {}).await?;
    Ok(())
}

pub async fn reconnect_if_unhealthy(
    manager: &AsyncMcpManager,
    provider: &str,
) -> Result<()> {
    if let Err(_) = check_provider_health(&manager.get_client()?).await {
        manager.reconnect(provider).await?;
    }
    Ok(())
}
```

**Integration point:** `src/agent/runloop/unified/async_mcp_manager.rs`

---

## 10. OAuth 2.1 Authorization Support

### Current VTCode Gap ⚠️
**Missing Feature**: OAuth 2.1 support for protected resources

### RMCP Pattern
Official Rust SDK includes OAuth support: `docs/OAUTH_SUPPORT.md`

### Recommendation: Plan OAuth Integration

**Steps:**
1. Review `rmcp` OAuth patterns in official SDK
2. Add `oauth2` crate to dependencies
3. Implement `AuthorizationHandler` trait
4. Add `auth` field to `McpProviderConfig`

**Config example:**
```toml
[[mcp.providers]]
name = "secure-api"
command = "npx"
args = ["@secure/mcp-server"]

[mcp.providers.auth]
type = "oauth2"
client_id = "${OAUTH_CLIENT_ID}"
client_secret = "${OAUTH_CLIENT_SECRET}"
scopes = ["read:data", "write:data"]
```

---

## 11. Streaming & Long-Running Operations

### Current VTCode Gap ⚠️
**Status**: May not fully support MCP streaming

### RMCP Pattern
Protocol supports streaming responses via progress notifications

### Recommendation: Add Streaming Support

**New capability:**
```rust
pub async fn stream_mcp_tool(
    client: &ManagedClient,
    provider: &str,
    tool_name: &str,
    arguments: JsonValue,
) -> Result<impl Stream<Item = Result<JsonValue>>> {
    // Stream intermediate results as they arrive
}
```

**Use cases:**
- Long-running file operations
- Paginated API results
- Real-time data updates

---

## 12. Testing & Integration

### Current VTCode Status ✅
**Files:**
- `vtcode-core/tests/mcp_integration_test.rs`
- `vtcode-core/tests/mcp_integration_e2e.rs`

### RMCP Pattern
Official SDK includes examples in `examples/` directory

### Recommendation: Add Integration Tests

**New test file:** `vtcode-core/tests/mcp_provider_lifecycle.rs`

```rust
#[tokio::test]
async fn test_provider_startup_timeout() {
    // Test timeout behavior
}

#[tokio::test]
async fn test_provider_reconnection() {
    // Test reconnection after failure
}

#[tokio::test]
async fn test_concurrent_tool_invocations() {
    // Test thread-safety
}
```

---

## Implementation Priority

### Phase 1: High Priority (Immediate)
1. **Use RMCP Transport wrappers** — Replaces custom transport logic
2. **Add schemars integration** — Type-safe schema generation
3. **Unified error handling** — Use `anyhow` consistently
4. **Update documentation** — Link to RMCP patterns

**Timeline:** 1-2 weeks
**Impact:** Better alignment, reduced custom code

### Phase 2: Medium Priority (1-2 months)
1. **Add health check support** — Robust connection management
2. **Simplify async lifecycle** — Leverage RMCP ServiceExt
3. **Improve streaming support** — For long-running operations
4. **Add comprehensive tests** — Provider lifecycle tests

**Timeline:** 2-4 weeks
**Impact:** Better reliability, spec compliance

### Phase 3: Future (3+ months)
1. **OAuth 2.1 support** — For protected resources
2. **Advanced auth patterns** — mTLS, JWT, etc.
3. **Custom transport backends** — WebSocket, gRPC
4. **Performance optimization** — Connection pooling, caching

**Timeline:** As needed
**Impact:** Enterprise features

---

## Checklist for Fine-Tuning

- [ ] Review `rmcp` v0.9.0+ crate documentation
- [ ] Update Cargo.toml with `rmcp = "0.9"` feature flags
- [ ] Replace custom transport with `TokioChildProcess`
- [ ] Add `schemars` for schema generation
- [ ] Migrate to `anyhow::Result` throughout MCP module
- [ ] Add health check service
- [ ] Implement streaming support
- [ ] Update integration tests
- [ ] Document OAuth integration plan
- [ ] Update AGENTS.md with MCP patterns

---

## References

- **RMCP GitHub:** https://github.com/modelcontextprotocol/rust-sdk
- **RMCP Crates.io:** https://crates.io/crates/rmcp (v0.9.0+)
- **RMCP Examples:** https://github.com/modelcontextprotocol/rust-sdk/tree/main/examples
- **MCP Specification:** https://spec.modelcontextprotocol.io/2025-06-18/
- **MCP llms.txt:** https://modelcontextprotocol.io/llms.txt
- **VTCode MCP Current:** `vtcode-core/src/mcp/mod.rs`

---

## Next Steps

1. **Create RMCP upgrade branch**
   ```bash
   git checkout -b feat/rmcp-alignment
   ```

2. **Start with Phase 1: Transport layer**
   - Update `vtcode-core/src/mcp/transport.rs` to use RMCP wrappers
   - Update `Cargo.toml` dependencies

3. **Test changes incrementally**
   ```bash
   cargo test --package vtcode-core --lib mcp::
   ```

4. **Update documentation**
   - Update `docs/mcp/MCP_COMPLETE_IMPLEMENTATION_STATUS.md`
   - Add RMCP patterns to `AGENTS.md`

5. **Review with team**
   - Share findings before major refactoring
   - Get approval for breaking changes (if any)
