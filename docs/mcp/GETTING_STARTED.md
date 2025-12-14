# MCP Module - Getting Started Guide

**For:** Developers & Teams Ready to Use MCP  
**Time:** 20 minutes to productive coding  
**Status:**  Production-ready (Phase 1 & 2)

---

## Quick Start (5 Minutes)

### Step 1: Understand What's Available

The MCP module provides:
- **Error handling** - 7 helpers for consistent error management
- **Schema validation** - Full JSON Schema 2020-12 support
- **Transport layer** - Stdio and HTTP transports (HTTP in Phase 3)

### Step 2: See It In Action

```rust
use vtcode_core::mcp::*;

// Error handling
if provider_not_found("my_provider") {
    return Err(provider_not_found("my_provider").into());
}

// Schema validation
validate_tool_input(Some(&schema), &input)?;

// Transport creation
let (transport, _stderr) = create_stdio_transport_with_stderr(
    program, args, working_dir, env)?;
```

### Step 3: Read the Docs

- **5 min:** [MCP_MASTER_GUIDE.md](MCP_MASTER_GUIDE.md) - API overview
- **10 min:** [MCP_PHASE1_USAGE_GUIDE.md](MCP_PHASE1_USAGE_GUIDE.md) - Code patterns
- **5 min:** Copy pattern that matches your use case

**You're ready to build!** 

---

## Implementation Checklist

###  Setup (First Time)

- [ ] Read [README.md](README.md) (2 min)
- [ ] Bookmark [INDEX.md](INDEX.md) for future reference
- [ ] Skim [MCP_MASTER_GUIDE.md](MCP_MASTER_GUIDE.md#api-reference) (5 min)
- [ ] Note the 7 error helpers available
- [ ] Review 1-2 code examples from [MCP_PHASE1_USAGE_GUIDE.md](MCP_PHASE1_USAGE_GUIDE.md)

###  Development (Use in Code)

**For Error Handling:**
- [ ] Import error helpers: `use vtcode_core::mcp::*;`
- [ ] Use appropriate helper for your error case
- [ ] Add context with anyhow: `.context("descriptive message")?`
- [ ] Test error path works

**For Schema Validation:**
- [ ] Get your JSON schema (object with properties, required, etc.)
- [ ] Call `validate_tool_input(Some(&schema), &input)?`
- [ ] Handle validation error appropriately
- [ ] Test with valid and invalid inputs

**For Transport:**
- [ ] Import transport creation: `use vtcode_core::mcp::create_stdio_transport_with_stderr;`
- [ ] Call with program, args, working_dir, environment
- [ ] Handle transport errors
- [ ] Test with actual MCP server

###  Testing (Verify It Works)

- [ ] Test error helpers are imported correctly
- [ ] Test schema validation with valid input
- [ ] Test schema validation with invalid input
- [ ] Test transport creation with real program
- [ ] Verify stderr capturing works
- [ ] Run `cargo test -p vtcode-core mcp` (if applicable)

###  Review (Quality)

- [ ] Code follows patterns from [MCP_PHASE1_USAGE_GUIDE.md](MCP_PHASE1_USAGE_GUIDE.md)
- [ ] Error messages are descriptive
- [ ] Schema validation is comprehensive
- [ ] Transport creation handles errors
- [ ] Tests cover both success and failure paths

---

## Code Pattern Examples

### Pattern 1: Error Handling in Tool Invocation

```rust
use vtcode_core::mcp::*;
use anyhow::Context;

fn invoke_mcp_tool(
    provider: &str,
    tool: &str,
    input: &Value,
) -> anyhow::Result<Value> {
    // Check provider exists
    let prov = get_provider(provider)
        .ok_or_else(|| provider_not_found(provider))?;

    // Check tool exists
    let t = prov.get_tool(tool)
        .ok_or_else(|| tool_not_found(tool))?;

    // Validate input schema
    if let Some(schema) = t.input_schema() {
        validate_tool_input(Some(schema), input)
            .context("Schema validation failed")?;
    }

    // Invoke tool with error handling
    t.call(input)
        .context(
            tool_invocation_failed(provider, tool, "execution failed")
        )
}
```

### Pattern 2: Schema Validation with Error Details

```rust
use vtcode_core::mcp::validate_tool_input;
use serde_json::json;

fn validate_user_input(schema: &Value, input: &Value) -> anyhow::Result<()> {
    match validate_tool_input(Some(schema), input) {
        Ok(_) => {
            println!(" Input is valid");
            Ok(())
        }
        Err(e) => {
            eprintln!(" Validation failed: {}", e);
            Err(e)
        }
    }
}

// Test with valid input
let schema = json!({
    "type": "object",
    "properties": {
        "name": { "type": "string" },
        "age": { "type": "integer", "minimum": 0 }
    },
    "required": ["name"]
});

let valid = json!({"name": "Alice", "age": 30});
validate_user_input(&schema, &valid)?; //  Pass

let invalid = json!({"age": -5}); // Missing "name", negative age
validate_user_input(&schema, &invalid)?; //  Fail (as expected)
```

### Pattern 3: Transport Creation with Stderr

```rust
use vtcode_core::mcp::create_stdio_transport_with_stderr;
use std::collections::HashMap;
use std::ffi::OsString;

async fn create_mcp_transport(
    program: &str,
    args: Vec<&str>,
) -> anyhow::Result<()> {
    let args: Vec<OsString> = args.iter()
        .map(|s| OsString::from(s))
        .collect();

    let (transport, stderr) = create_stdio_transport_with_stderr(
        &OsString::from(program),
        &args,
        None, // working_dir
        &HashMap::new(), // env
    )?;

    // Now you have transport ready to use
    // Optional: handle stderr in background task
    if let Some(stderr_reader) = stderr {
        tokio::spawn(async move {
            // Log stderr as needed
        });
    }

    Ok(())
}
```

### Pattern 4: Testing MCP Integration

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_error_handling() {
        let err = tool_not_found("missing_tool");
        assert!(err.to_string().contains("missing_tool"));
    }

    #[test]
    fn test_schema_validation_success() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            },
            "required": ["name"]
        });

        let input = json!({"name": "test"});
        assert!(validate_tool_input(Some(&schema), &input).is_ok());
    }

    #[test]
    fn test_schema_validation_failure() {
        let schema = json!({
            "type": "object",
            "required": ["name"]
        });

        let input = json!({});
        assert!(validate_tool_input(Some(&schema), &input).is_err());
    }
}
```

---

## Common Issues & Solutions

### Issue: "Import not found"

**Problem:**
```
error: cannot find function `tool_not_found` in this scope
```

**Solution:**
```rust
// Wrong
use vtcode_core::mcp;
let err = tool_not_found("x"); //  Won't work

// Right
use vtcode_core::mcp::*;
let err = tool_not_found("x"); //  Works
```

### Issue: "Schema validation always passes"

**Problem:** Passing `None` instead of schema

**Solution:**
```rust
// Wrong
validate_tool_input(None, &input)?; // Always passes!

// Right
validate_tool_input(Some(&schema), &input)?; // Actually validates
```

### Issue: "Transport not capturing stderr"

**Problem:** Using basic transport instead of stderr-capturing variant

**Solution:**
```rust
// Wrong
let transport = create_stdio_transport(&config, &env)?;
// stderr is lost

// Right
let (transport, stderr) = create_stdio_transport_with_stderr(
    program, args, dir, env)?;
// stderr is captured
```

---

## Next Steps After Getting Started

### If You Need More Details
→ See [MCP_PHASE1_USAGE_GUIDE.md](MCP_PHASE1_USAGE_GUIDE.md) for comprehensive patterns

### If You Need to Know What's Tested
→ Check [phase1/VERIFICATION.md](phase1/VERIFICATION.md) (23 tests documented)

### If You're Planning Phase 3 Implementation
→ Read [MCP_MASTER_GUIDE.md#phase-3-roadmap](MCP_MASTER_GUIDE.md#phase-3-roadmap)

### If You Need to Find Something
→ Use [INDEX.md](INDEX.md) for quick navigation

---

## Success Indicators

You'll know you're using MCP correctly when:

 Your code compiles without warnings (MCP-specific)  
 Error messages are clear and descriptive  
 Schema validation catches invalid input  
 Tests verify both success and failure paths  
 Transport creation handles errors gracefully  
 New team members can use your code as an example  

---

## Quick Reference - All APIs

### Error Helpers (7 total)

```rust
use vtcode_core::mcp::*;

tool_not_found("name")              // Tool doesn't exist
provider_not_found("name")          // Provider doesn't exist
provider_unavailable("name")        // Provider temporarily unavailable
schema_invalid("reason")            // Schema validation failed
tool_invocation_failed(p, t, r)     // Tool execution failed
initialization_timeout(seconds)     // Provider init timed out
configuration_error("reason")       // Configuration problem
```

### Schema Validation

```rust
use vtcode_core::mcp::validate_tool_input;

// Full JSON Schema 2020-12 support
validate_tool_input(Some(&schema), &input)?;

// Supports:
// - Required properties
// - Type validation (string, integer, boolean, array, object, null)
// - Min/max constraints
// - Enum values
// - Nested objects
// - Array items
// - Pattern matching (regex)
// - Complex schemas (oneOf, anyOf, allOf)
```

### Transport Creation

```rust
use vtcode_core::mcp::create_stdio_transport_with_stderr;

let (transport, stderr) = create_stdio_transport_with_stderr(
    program,      // Program path
    args,         // Arguments
    working_dir,  // Optional working directory
    env          // Environment variables
)?;
```

---

## Support

**Question?** Check [INDEX.md](INDEX.md) for topic navigation  
**Example needed?** See [MCP_PHASE1_USAGE_GUIDE.md](MCP_PHASE1_USAGE_GUIDE.md)  
**API reference?** [MCP_MASTER_GUIDE.md#api-reference](MCP_MASTER_GUIDE.md#api-reference)  
**Testing patterns?** [phase1/VERIFICATION.md](phase1/VERIFICATION.md)  

---

**Status:** Ready to use   
**Time to Productivity:** 20 minutes  
**Next Step:** Pick a code pattern above and start building! 

---

**Last Updated:** 2025-11-20  
**Phase Status:** Phase 1  | Phase 2  (40%) | Phase 3 
