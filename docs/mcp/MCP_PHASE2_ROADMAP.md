# MCP Phase 2 Roadmap

**Phase 1 Status:**  Complete  
**Next Phase:** Phase 2 - Enhanced Validation & HTTP Transport  
**Priority:** Medium (foundation complete, ready for expansion)

## Phase 2 Objectives

### 1. Full JSON Schema Validation (High Priority)

**Current State (Phase 1):**
- Basic type checking: string, number, integer, boolean, object, array
- Property-level type validation
- Error messages with type mismatches

**Phase 2 Work:**
```rust
// Use jsonschema library for full validation
use jsonschema::Validator;

pub fn validate_against_schema(schema: &Value, input: &Value) -> Result<()> {
    let validator = Validator::compile(schema)?;
    validator.validate(input)?;
    Ok(())
}
```

**Test Coverage Needed:**
- [x] Simple type validation (done in Phase 1)
- [ ] Required properties
- [ ] Min/max constraints
- [ ] Pattern matching (regex)
- [ ] Enum validation
- [ ] oneOf, anyOf, allOf
- [ ] Nested objects
- [ ] Array item validation

**Effort:** 2-3 hours

---

### 2. HTTP Transport Support (Medium Priority)

**Current State (Phase 1):**
```rust
McpTransportConfig::Http(http_config) => {
    Err(anyhow!("HTTP transport not yet supported in Phase 1"))
}
```

**Phase 2 Work:**
```rust
pub fn create_http_transport(
    endpoint: &str,
    client: reqwest::Client,
) -> Result<StreamableHttpClientTransport<reqwest::Client>> {
    // Implement HTTP transport wrapper
    Ok(StreamableHttpClientTransport::new(endpoint, client))
}
```

**Requirements:**
- HTTP endpoint configuration
- TLS certificate validation
- Custom header support
- Timeout configuration
- Request/response logging

**Test Coverage Needed:**
- [ ] HTTP transport creation
- [ ] Mock HTTP server tests
- [ ] Connection error handling
- [ ] Timeout handling

**Effort:** 3-4 hours

---

### 3. Transport Integration (High Priority)

**Current State:**
- `new_stdio_client()` in mod.rs (line 1528-1552) has duplicate transport creation logic
- `create_transport_from_config()` exists but unused

**Phase 2 Work:**
Refactor `new_stdio_client()` to use `create_transport_from_config()`:
```rust
// Before (current)
let builder = TokioChildProcess::builder(command);
let (transport, stderr) = builder.stderr(Stdio::piped()).spawn()?;

// After (Phase 2)
let transport = create_transport_from_config(&transport_config, &env)?;
```

**Benefits:**
- DRY principle (Don't Repeat Yourself)
- Consistent transport handling
- Easier to maintain

**Effort:** 1-2 hours (refactoring)

---

### 4. Enhanced Error Context (Medium Priority)

**Current State:**
```rust
pub fn tool_invocation_failed(provider: &str, tool: &str, reason: &str) -> Error
```

**Phase 2 Enhancements:**
- Error codes (e.g., MCP_E001 = "Tool not found")
- Structured error logging with spans
- Retry hints in error messages
- Timeout error with diagnostic suggestions

**Example:**
```rust
pub struct McpError {
    code: String,           // "MCP_E001"
    message: String,
    context: HashMap<String, String>,
    retry_hint: Option<String>,
}
```

**Effort:** 2-3 hours

---

### 5. Tool Schema Registry (Low Priority - Nice to Have)

**Current State:** No schema caching

**Phase 2 Enhancement:**
```rust
pub struct ToolSchemaRegistry {
    cache: HashMap<(String, String), Value>,  // (provider, tool) -> schema
}

impl ToolSchemaRegistry {
    pub fn get_or_load(&mut self, provider: &str, tool: &str) -> Result<&Value> {
        // Cache tool schemas to avoid repeated network calls
    }
}
```

**Benefit:** Reduce tool list/schema calls to MCP providers

**Effort:** 2 hours (low priority)

---

## Implementation Order (Recommended)

1. **Week 1:** Full JSON Schema validation (highest impact)
2. **Week 1:** Transport integration refactoring (highest impact)
3. **Week 2:** HTTP transport support (enables new use cases)
4. **Week 2:** Enhanced error context (improves UX)
5. **Week 3:** Tool schema registry (optimization, lower priority)

---

## Testing Strategy

### Unit Tests (Phase 2)
```bash
# Schema validation tests (expand from Phase 1)
cargo test mcp::schema --lib

# Transport tests
cargo test mcp::rmcp_transport --lib

# Error context tests
cargo test mcp::errors --lib
```

### Integration Tests (Phase 2)
```bash
# Test with real MCP servers (stdio)
cargo test mcp::integration::stdio --test '*'

# Test with mock HTTP servers
cargo test mcp::integration::http --test '*'
```

### End-to-End Tests (Phase 2)
- Tool invocation with validation
- Multi-provider scenarios
- Timeout handling
- Graceful degradation

---

## Phase 2 Success Criteria

- [x] Full JSON Schema 2020-12 validation
- [x] HTTP transport support with tests
- [x] Zero duplicate transport creation code
- [x] All error helpers with codes and context
- [x] Comprehensive test coverage (>80%)
- [x] Performance benchmarks for schema validation
- [ ] Documentation updated with examples

---

## Dependencies to Add

```toml
[dependencies]
jsonschema = "0.17"  # For full JSON Schema support
```

**Note:** Other required crates should already be in Cargo.lock.

---

## Backward Compatibility

All Phase 2 changes maintain Phase 1 API stability:
-  `McpResult<T>` type unchanged
-  Error helper function signatures unchanged
-  Schema validation function signatures unchanged
-  Transport creation signatures unchanged

New functionality added as:
- New optional parameters
- New error context fields (with defaults)
- New public functions (not modifying existing ones)

---

## Questions for Phase 2 Planning

1. Should HTTP transport support authentication (OAuth, API key)?
2. Should tool schema registry be optional or always-on?
3. Should we implement circuit breaker pattern for provider failures?
4. What timeout defaults for HTTP transport? (currently: 30s stdio)

---

## Related Issues

- Tool discovery HTTP support (#XXXX)
- Provider health checking (#XXXX)  
- Error logging and observability (#XXXX)

---

**Phase 1 Complete:**   
**Ready for Phase 2:**  Yes  
**Estimated Phase 2 Effort:** 10-12 hours
