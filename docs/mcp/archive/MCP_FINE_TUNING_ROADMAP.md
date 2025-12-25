# VT Code MCP Fine-Tuning Implementation Roadmap

Detailed implementation roadmap to align vtcode MCP client with official `rmcp` (Rust SDK) best practices.

## Overview

This document provides concrete implementation steps for the 12 alignment gaps identified in `MCP_RUST_SDK_ALIGNMENT.md`.

---

## Phase 1: Foundation (Weeks 1-2)

### 1.1 Update Dependencies

**File:** `Cargo.toml` (root)

**Current:**

```toml
[workspace]
members = [
    "vtcode-core",
    "vtcode-config",
    # ...
]
```

**Add rmcp dependency to vtcode-core:**

```toml
[package]
name = "vtcode-core"
version = "0.1.0"

[dependencies]
rmcp = { version = "0.9.0", features = ["client"] }
schemars = { version = "0.8", features = ["chrono"] }
anyhow = "1.0"
tokio = { version = "1", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
```

**Verification:**

```bash
cargo check -p vtcode-core
```

---

### 1.2 Create RMCP Transport Layer Wrapper

**New file:** `vtcode-core/src/mcp/rmcp_transport.rs`

```rust
use rmcp::transport::{TokioChildProcess, HttpTransport, ConfigureCommandExt};
use tokio::process::Command;
use anyhow::{Result, Context};
use crate::config::mcp::McpTransport;

/// Create transport from vtcode config
pub async fn create_transport(config: &McpTransport) -> Result<TransportType> {
    match config {
        McpTransport::Stdio(stdio_config) => {
            let mut cmd = Command::new(&stdio_config.command);
            cmd.args(&stdio_config.args);

            // Configure environment variables
            for (key, value) in &stdio_config.env {
                cmd.env(key, value);
            }

            let transport = TokioChildProcess::new(cmd)
                .context("Failed to create child process")?;

            Ok(TransportType::Stdio(transport))
        }
        McpTransport::Http(http_config) => {
            let transport = HttpTransport::connect(&http_config.url)
                .await
                .context("Failed to connect to HTTP server")?;

            Ok(TransportType::Http(transport))
        }
    }
}

pub enum TransportType {
    Stdio(TokioChildProcess),
    Http(HttpTransport),
}
```

**Update:** `vtcode-core/src/mcp/mod.rs` to use new transport layer

---

### 1.3 Migrate to Unified Error Handling

**New file:** `vtcode-core/src/mcp/errors.rs`

```rust
use anyhow::{anyhow, Context, Result};

// Remove custom error enum; use anyhow::Result<T> everywhere

pub type McpResult<T> = anyhow::Result<T>;

// Helper functions for context
pub fn tool_not_found(name: &str) -> anyhow::Error {
    anyhow!("Tool '{}' not found", name)
}

pub fn provider_unavailable(name: &str) -> anyhow::Error {
    anyhow!("Provider '{}' is unavailable", name)
}

pub fn schema_invalid(reason: &str) -> anyhow::Error {
    anyhow!("Invalid schema: {}", reason)
}
```

**Update all files using `McpError`:**

```bash
# Find all McpError references
grep -r "McpError" vtcode-core/src/mcp/

# Replace with anyhow pattern
```

**Before:**

```rust
pub fn list_tools(&self) -> Result<Vec<Tool>, McpError> {
    self.client.as_ref()
        .ok_or(McpError::NotInitialized)?
        .list_tools()
        .map_err(|e| McpError::ToolError(e.to_string()))
}
```

**After:**

```rust
pub fn list_tools(&self) -> anyhow::Result<Vec<Tool>> {
    let client = self.client.as_ref()
        .context("MCP client not initialized")?;

    client.list_tools()
        .context("Failed to list tools")
}
```

---

### 1.4 Add schemars Integration

**New file:** `vtcode-core/src/mcp/schema.rs`

```rust
use schemars::{JsonSchema, json_schema};
use serde::{Serialize, Deserialize};
use serde_json::{json, Value};

/// Tool definition with auto-generated JSON schema
#[derive(JsonSchema, Serialize, Deserialize, Clone, Debug)]
pub struct ManagedTool {
    pub name: String,
    pub description: String,

    #[serde(skip)]
    pub input_schema: Value,
}

impl ManagedTool {
    /// Generate or validate JSON schema for tool input
    pub fn schema_json(&self) -> &Value {
        &self.input_schema
    }

    /// Validate input against schema
    pub fn validate_input(&self, input: &Value) -> anyhow::Result<()> {
        // Use jsonschema crate for validation
        use jsonschema::validator::Draft202012;

        let schema = Draft202012::compile(&self.input_schema)
            .context("Failed to compile JSON schema")?;

        schema.validate(input)
            .context("Input does not match schema")?;

        Ok(())
    }
}

/// Generate schema from Rust type
pub fn generate_schema<T: JsonSchema>() -> Value {
    serde_json::to_value(schemars::schema_for!(T))
        .expect("Failed to serialize schema")
}
```

**Add to Cargo.toml:**

```toml
[dependencies]
jsonschema = "0.18"  # For schema validation
```

---

## Phase 2: Async Lifecycle Simplification (Weeks 2-3)

### 2.1 Refactor AsyncMcpManager with RMCP Patterns

**File:** `src/agent/runloop/unified/async_mcp_manager.rs`

**Current state tracking:**

```rust
pub enum McpInitStatus {
    Disabled,
    Initializing { progress: String },
    Ready { client: McpClient },
    Error { message: String },
}
```

**Simplified version:**

```rust
use rmcp::ServiceExt;

pub enum McpInitStatus {
    Disabled,
    Initializing,
    Ready {
        client: Box<dyn ManagedMcpClient>,  // Trait object
    },
    Error {
        message: String,
        retry_count: u32,
    },
}

pub struct AsyncMcpManager {
    config: McpClientConfig,
    status: McpInitStatus,
    init_task: Option<JoinHandle<anyhow::Result<Box<dyn ManagedMcpClient>>>>,
}

impl AsyncMcpManager {
    pub async fn start_initialization(&mut self) -> anyhow::Result<()> {
        if matches!(self.status, McpInitStatus::Initializing) {
            return Ok(()); // Already initializing
        }

        self.status = McpInitStatus::Initializing;

        let config = self.config.clone();
        let task = tokio::spawn(async move {
            Self::initialize_providers(&config).await
        });

        self.init_task = Some(task);
        Ok(())
    }

    private async fn initialize_providers(
        config: &McpClientConfig,
    ) -> anyhow::Result<Box<dyn ManagedMcpClient>> {
        // Initialize all providers in parallel
        let mut handles = vec![];

        for (name, provider_config) in &config.providers {
            let name = name.clone();
            let config = provider_config.clone();

            let handle = tokio::spawn(async move {
                Self::initialize_provider(&name, &config).await
            });

            handles.push(handle);
        }

        // Wait for all with timeout
        let timeout = Duration::from_secs(config.startup_timeout_seconds);
        let results = tokio::time::timeout(timeout, futures::future::join_all(handles))
            .await
            .context("MCP initialization timeout")?;

        // Collect results
        let mut clients = HashMap::new();
        for result in results {
            let (name, client) = result??;
            clients.insert(name, client);
        }

        Ok(Box::new(MultiProviderClient { clients }))
    }

    pub async fn get_status(&self) -> McpInitStatus {
        // Check if task completed
        if let Some(task) = &self.init_task {
            if task.is_finished() {
                // Handle completion
            }
        }

        self.status.clone()
    }
}
```

**Key improvements:**

1. Cleaner state machine
2. Parallel provider initialization
3. Proper timeout handling
4. Better error context

---

### 2.2 Create MultiProviderClient Wrapper

**New file:** `vtcode-core/src/mcp/multi_provider.rs`

```rust
use async_trait::async_trait;
use std::collections::HashMap;

pub trait ManagedMcpClient: Send + Sync {
    async fn list_tools(&self, provider: &str) -> anyhow::Result<Vec<Tool>>;
    async fn execute_tool(
        &self,
        provider: &str,
        tool: &str,
        args: serde_json::Value,
    ) -> anyhow::Result<ToolResult>;
    async fn health_check(&self) -> anyhow::Result<()>;
}

pub struct MultiProviderClient {
    clients: HashMap<String, Box<dyn ProviderClient>>,
}

#[async_trait]
impl ManagedMcpClient for MultiProviderClient {
    async fn list_tools(&self, provider: &str) -> anyhow::Result<Vec<Tool>> {
        self.get_client(provider)?
            .list_tools()
            .await
    }

    async fn execute_tool(
        &self,
        provider: &str,
        tool: &str,
        args: serde_json::Value,
    ) -> anyhow::Result<ToolResult> {
        let client = self.get_client(provider)?;
        client.execute_tool(tool, args).await
    }

    async fn health_check(&self) -> anyhow::Result<()> {
        // Check all providers
        for (name, client) in &self.clients {
            client.health_check()
                .await
                .context(format!("Provider '{}' health check failed", name))?;
        }
        Ok(())
    }
}

impl MultiProviderClient {
    fn get_client(&self, provider: &str) -> anyhow::Result<&dyn ProviderClient> {
        self.clients.get(provider)
            .map(|c| c.as_ref())
            .ok_or_else(|| anyhow!("Provider '{}' not found", provider))
    }
}
```

---

## Phase 3: Tool Execution & Streaming (Weeks 3-4)

### 3.1 Update Tool Invocation with RMCP Patterns

**File:** `vtcode-core/src/tools/registry/executors.rs`

```rust
use rmcp::client::calls::ToolCall;

pub async fn execute_mcp_tool(
    client: &dyn ManagedMcpClient,
    provider: &str,
    tool_name: &str,
    arguments: serde_json::Value,
) -> anyhow::Result<ToolResult> {
    // Validate schema before execution
    let tools = client.list_tools(provider).await?;
    let tool = tools.iter()
        .find(|t| t.name == tool_name)
        .ok_or_else(|| anyhow!("Tool not found: {}", tool_name))?;

    // Validate input
    if let Some(schema) = &tool.schema {
        schema.validate(&arguments)
            .context("Tool arguments do not match schema")?;
    }

    // Execute with RMCP pattern
    let call = ToolCall {
        name: tool_name.to_string(),
        arguments,
    };

    client.execute_tool(provider, tool_name, call.arguments)
        .await
        .context("Tool execution failed")
}

pub async fn stream_mcp_tool(
    client: &dyn ManagedMcpClient,
    provider: &str,
    tool_name: &str,
    arguments: serde_json::Value,
) -> anyhow::Result<impl Stream<Item = anyhow::Result<serde_json::Value>>> {
    // For streaming operations
    // Implementation depends on RMCP streaming support
    todo!("Implement streaming support")
}
```

---

### 3.2 Add Health Check Service

**New file:** `vtcode-core/src/mcp/health.rs`

```rust
use std::time::Duration;

pub struct HealthChecker {
    interval: Duration,
    timeout: Duration,
}

impl HealthChecker {
    pub fn new() -> Self {
        Self {
            interval: Duration::from_secs(30),
            timeout: Duration::from_secs(5),
        }
    }

    pub async fn check(
        &self,
        client: &dyn ManagedMcpClient,
    ) -> anyhow::Result<HealthStatus> {
        let result = tokio::time::timeout(
            self.timeout,
            client.health_check(),
        ).await;

        match result {
            Ok(Ok(())) => Ok(HealthStatus::Healthy),
            Ok(Err(e)) => Ok(HealthStatus::Unhealthy(e.to_string())),
            Err(_) => Ok(HealthStatus::Timeout),
        }
    }
}

pub enum HealthStatus {
    Healthy,
    Unhealthy(String),
    Timeout,
}
```

---

## Phase 4: Testing & Documentation (Week 4)

### 4.1 Add Integration Tests

**New file:** `vtcode-core/tests/mcp_provider_lifecycle.rs`

```rust
#[tokio::test]
async fn test_provider_initialization() {
    // Test successful provider startup
}

#[tokio::test]
async fn test_provider_initialization_timeout() {
    // Test timeout handling
}

#[tokio::test]
async fn test_provider_reconnection() {
    // Test reconnection after failure
}

#[tokio::test]
async fn test_concurrent_tool_invocations() {
    // Test thread-safety
}

#[tokio::test]
async fn test_schema_validation() {
    // Test input validation
}
```

---

### 4.2 Update Documentation

**Update files:**

1. `docs/mcp/MCP_COMPLETE_IMPLEMENTATION_STATUS.md` — Add Phase 1 completion
2. `AGENTS.md` — Add RMCP integration patterns
3. `docs/mcp/README.md` — Create overview

---

## Validation Checklist

-   [ ] All dependencies updated
-   [ ] `cargo check` passes
-   [ ] `cargo clippy` passes
-   [ ] Transport layer refactored
-   [ ] Error handling unified to `anyhow`
-   [ ] schemars integrated for schemas
-   [ ] AsyncMcpManager refactored
-   [ ] Health checks implemented
-   [ ] Tool invocation updated
-   [ ] All integration tests pass
-   [ ] Documentation updated

---

## Rollback Plan

If issues arise, maintain git history:

```bash
# Create feature branch
git checkout -b feat/rmcp-alignment

# Tag before major changes
git tag phase-1-checkpoint

# If rollback needed
git reset --hard phase-1-checkpoint
```

---

## Performance Impact

**Expected improvements:**

-   Reduced code complexity: 30-40% fewer custom implementations
-   Better error context: Easier debugging with `anyhow`
-   Type-safe schemas: Compile-time schema validation
-   Parallel initialization: Faster MCP startup

**No performance degradation expected** — RMCP patterns are production-tested.

---

## Timeline Summary

| Phase     | Duration    | Key Deliverables                         |
| --------- | ----------- | ---------------------------------------- |
| Phase 1   | 2 weeks     | Dependencies, transport, error handling  |
| Phase 2   | 1 week      | Async lifecycle, multi-provider client   |
| Phase 3   | 1 week      | Tool execution, health checks, streaming |
| Phase 4   | 1 week      | Tests, documentation, validation         |
| **Total** | **5 weeks** | **Full RMCP alignment**                  |

---

## Next Steps

1. Create feature branch: `git checkout -b feat/rmcp-alignment`
2. Start Phase 1 immediately
3. Weekly sync to review progress
4. Tag milestones for easy rollback
5. Merge to main after full validation

---

## References

-   RMCP GitHub: https://github.com/modelcontextprotocol/rust-sdk
-   RMCP Examples: https://github.com/modelcontextprotocol/rust-sdk/tree/main/examples
-   Alignment Doc: `docs/mcp/MCP_RUST_SDK_ALIGNMENT.md`
-   Current Status: `docs/mcp/MCP_COMPLETE_IMPLEMENTATION_STATUS.md`
