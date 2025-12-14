# MCP Phase 1 Usage Guide

**Phase:** 1 (Foundation complete)  
**Updated:** 2025-11-20  
**Audience:** VTCode developers integrating MCP

## Quick Start

### Import the MCP module
```rust
use vtcode_core::mcp::{
    McpResult,
    tool_not_found, provider_not_found,
    validate_tool_input, validate_against_schema,
    create_stdio_transport, create_transport_from_config,
};
```

---

## Error Handling Pattern

### Creating Specific Errors

```rust
use vtcode_core::mcp::*;

// Tool errors
let err = tool_not_found("my_tool");
let err = tool_invocation_failed("claude", "my_tool", "invalid input");

// Provider errors
let err = provider_not_found("openai");
let err = provider_unavailable("claude");

// Schema errors
let err = schema_invalid("missing required property 'name'");

// Config errors
let err = configuration_error("invalid provider config");

// Timeout errors
let err = initialization_timeout(30);
```

### Using McpResult<T>

```rust
use vtcode_core::mcp::McpResult;

fn invoke_tool(provider: &str, tool: &str) -> McpResult<serde_json::Value> {
    // Function returns Result<T, anyhow::Error>
    // Automatically compatible with ? operator
    let result = call_tool(provider, tool)?;
    Ok(result)
}

// Caller can handle errors
match invoke_tool("claude", "list_files") {
    Ok(result) => println!("Success: {:?}", result),
    Err(e) => eprintln!("Error: {}", e),
}
```

---

## Schema Validation

### Basic Validation

```rust
use vtcode_core::mcp::validate_tool_input;
use serde_json::json;

let schema = json!({
    "type": "object",
    "properties": {
        "name": { "type": "string" },
        "age": { "type": "integer" }
    }
});

let valid_input = json!({
    "name": "John",
    "age": 30
});

match validate_tool_input(Some(&schema), &valid_input) {
    Ok(()) => println!("Valid input"),
    Err(e) => eprintln!("Validation error: {}", e),
}
```

### Schema Validation Against Errors

```rust
// Type mismatch detected
let invalid_input = json!({
    "name": 123,  // Should be string
    "age": "thirty"  // Should be integer
});

// Returns error with specific field and type info:
// "Property 'name': expected string, got number"
match validate_tool_input(Some(&schema), &invalid_input) {
    Err(e) => println!("Field error: {}", e),  // Detailed message
    _ => {}
}
```

### Null Input Detection

```rust
let null_input = json!(null);

// Always rejects null input
validate_tool_input(Some(&schema), &null_input)
    .expect_err("Null inputs always rejected");
```

### No Schema Validation

```rust
// When schema is None, only checks input is not null
validate_tool_input(None, &json!({"any": "value"}))?;
```

---

## Transport Creation

### From Stdio Config

```rust
use vtcode_core::mcp::create_stdio_transport;
use vtcode_config::mcp::McpStdioServerConfig;

let config = McpStdioServerConfig {
    command: "python".to_string(),
    args: vec!["-m".to_string(), "mcp.server".to_string()],
    working_directory: Some("/path/to/server".into()),
};

let env = std::collections::HashMap::new();

let transport = create_stdio_transport(&config, &env)?;
// Ready to use with RMCP client
```

### From Generic Config

```rust
use vtcode_core::mcp::create_transport_from_config;
use vtcode_config::mcp::McpTransportConfig;

let transport_config = McpTransportConfig::Stdio(stdio_config);

let transport = create_transport_from_config(&transport_config, &env)?;
```

### HTTP Transport (Phase 2)

```rust
// Not yet supported in Phase 1
// Coming in Phase 2:
let transport_config = McpTransportConfig::Http(http_config);
// Err: "HTTP transport not yet supported in Phase 1"
```

---

## Common Patterns

### Error Chaining with Context

```rust
use anyhow::Context;
use vtcode_core::mcp::*;

fn setup_provider(name: &str) -> McpResult<()> {
    let config = load_config()
        .context("Failed to load MCP configuration")?;
    
    let transport = create_transport_from_config(&config.transport, &config.env)
        .context(format!("Failed to create transport for provider '{}'", name))?;
    
    Ok(())
}

// Error message propagates context:
// "Failed to create transport for provider 'claude': Failed to create child process..."
```

### Schema Validation with Tool Input

```rust
fn call_tool(
    provider: &str,
    tool: &str,
    input: serde_json::Value,
) -> McpResult<serde_json::Value> {
    // Get tool schema from provider
    let schema = get_tool_schema(provider, tool)?;
    
    // Validate input against schema
    validate_tool_input(schema.as_ref(), &input)
        .context(format!("Invalid input for tool '{}'", tool))?;
    
    // Safe to invoke tool
    invoke_tool(provider, tool, input)
        .context(format!("Tool invocation failed: {}/{}", provider, tool))
}
```

### Safe Error Fallback

```rust
fn try_get_tools(provider: &str) -> McpResult<Vec<Tool>> {
    match fetch_tool_list(provider) {
        Ok(tools) => Ok(tools),
        Err(_) => {
            // Fallback to empty list or cache
            Ok(Vec::new())
        }
    }
}
```

---

## What's NOT Supported in Phase 1

 Full JSON Schema validation (use Phase 2)
- No support for: `minLength`, `maxLength`, `pattern`, `enum`, `oneOf`, `anyOf`, `allOf`
- No nested schema validation beyond property types
- No array item type validation

 HTTP transport

 Complex error context (coming Phase 2)

 Tool schema caching (coming Phase 2)

---

## Phase 1 Limitations & Workarounds

### Limitation: Can't validate complex schemas
```rust
// This schema validation will only check type, not constraints
let schema = json!({
    "type": "object",
    "properties": {
        "name": {
            "type": "string",
            "minLength": 1,
            "maxLength": 255
        }
    }
});

// Phase 1: Only validates type is string
// Phase 2: Will validate length constraints
```

**Workaround:** Add manual validation after schema check:
```rust
validate_tool_input(Some(&schema), &input)?;

// Manual length check for Phase 1
if let Some(name) = input.get("name").and_then(|v| v.as_str()) {
    anyhow::ensure!(name.len() > 0, "name cannot be empty");
    anyhow::ensure!(name.len() <= 255, "name too long");
}
```

### Limitation: No HTTP transport
```rust
// Fails in Phase 1
let transport = create_transport_from_config(&http_config, &env)?;
// Err: "HTTP transport not yet supported in Phase 1"
```

**Workaround:** Use only stdio transport or wait for Phase 2

---

## Testing Phase 1 Code

### Unit Test Example

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use vtcode_core::mcp::*;

    #[test]
    fn test_schema_validation() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            }
        });

        let valid = json!({"name": "test"});
        assert!(validate_tool_input(Some(&schema), &valid).is_ok());

        let invalid = json!({"name": 123});
        assert!(validate_tool_input(Some(&schema), &invalid).is_err());
    }

    #[test]
    fn test_error_messages() {
        let err = tool_not_found("missing");
        assert_eq!(err.to_string(), "MCP tool 'missing' not found");
    }
}
```

---

## Debugging Tips

### Enable Detailed Error Context
```rust
use anyhow::Result;
use std::backtrace::Backtrace;

fn main() -> Result<()> {
    // Set backtrace to see call stack
    std::env::set_var("RUST_BACKTRACE", "1");
    
    // Your MCP code here
    Ok(())
}
```

### Log Error Chain
```rust
use anyhow::Chain;

if let Err(e) = invoke_tool(...) {
    eprintln!("Error chain:");
    for (i, cause) in std::error::Error::chain(&*e).enumerate() {
        eprintln!("  {}: {}", i, cause);
    }
}
```

---

## See Also

- `docs/mcp/MCP_PHASE1_FINAL_REVIEW.md` - What was fixed
- `docs/mcp/MCP_PHASE2_ROADMAP.md` - What's coming next
- `vtcode-core/src/mcp/` - Source code and tests

