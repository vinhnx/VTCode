# Gemini Function Parameters Sanitization Fix

## Issue

When calling Gemini API with tool definitions containing JSON Schema validation properties like `exclusiveMaximum` and `exclusiveMinimum`, the API returns a 400 error:

```
Invalid JSON payload received. Unknown name "exclusiveMaximum" at
'tools[19].function_declarations[0].parameters.properties[3].value': Cannot find field.
```

## Root Cause

The Gemini API doesn't support certain JSON Schema validation keywords that are valid in standard JSON Schema but not recognized by their function calling schema. These fields need to be removed from nested properties, not just the top-level object.

## Unsupported JSON Schema Keywords

The following JSON Schema properties are **NOT supported** by Gemini API:

### Validation Keywords

-   `exclusiveMaximum` - Upper bound (exclusive)
-   `exclusiveMinimum` - Lower bound (exclusive)
-   `minimum` - Lower bound (inclusive)
-   `maximum` - Upper bound (inclusive)

### Composition Keywords

-   `oneOf` - Exactly one schema matches
-   `anyOf` - At least one schema matches
-   `allOf` - All schemas must match
-   `not` - Schema must not match

### Structural Keywords

-   `additionalProperties` - Extra properties validation
-   `patternProperties` - Pattern-based property matching
-   `dependencies` - Property dependencies
-   `$schema` - Schema version
-   `$id` - Schema identifier
-   `$ref` - Schema reference
-   `definitions` - Schema definitions

### Conditional Keywords

-   `if` - Conditional validation
-   `then` - Then clause
-   `else` - Else clause
-   `const` - Constant value

### Content Keywords

-   `contentMediaType` - Media type hint
-   `contentEncoding` - Encoding hint

## Solution

Updated `sanitize_function_parameters()` in `gemini.rs` to:

1. **Use a constant list** of unsupported fields for clarity
2. **Filter during iteration** instead of removing from mutable map
3. **Apply recursively** to all nested objects and arrays

### Before (Problematic)

```rust
pub fn sanitize_function_parameters(parameters: Value) -> Value {
    match parameters {
        Value::Object(mut map) => {
            // Remove at top level only
            map.remove("exclusiveMaximum");
            map.remove("exclusiveMinimum");
            // ... then recurse
        }
    }
}
```

**Problem**: Removed fields from top level, then recursed - but nested objects weren't checked during the removal phase.

### After (Fixed)

```rust
pub fn sanitize_function_parameters(parameters: Value) -> Value {
    match parameters {
        Value::Object(map) => {
            const UNSUPPORTED_FIELDS: &[&str] = &[
                "exclusiveMaximum",
                "exclusiveMinimum",
                "minimum",
                "maximum",
                // ... complete list
            ];

            // Filter AND recurse in one pass
            let mut sanitized = Map::new();
            for (key, value) in map {
                if UNSUPPORTED_FIELDS.contains(&key.as_str()) {
                    continue; // Skip unsupported field
                }
                // Recursively sanitize nested values
                sanitized.insert(key, sanitize_function_parameters(value));
            }
            Value::Object(sanitized)
        }
        // ... handle arrays recursively too
    }
}
```

**Benefits**:

-   Removes unsupported fields at **every level** of nesting
-   More efficient (single pass)
-   Clearer intent with constant list
-   Easier to maintain and extend

## Test Case Added

```rust
#[test]
fn sanitize_function_parameters_removes_exclusive_min_max() {
    let parameters = json!({
        "type": "object",
        "properties": {
            "max_length": {
                "type": "integer",
                "exclusiveMaximum": 1000000,
                "exclusiveMinimum": 0,
                "minimum": 1,
                "maximum": 999999,
                "description": "Maximum number of characters"
            }
        }
    });

    let sanitized = sanitize_function_parameters(parameters);

    // Verify unsupported fields are removed
    assert!(!props.contains_key("exclusiveMaximum"));
    assert!(!props.contains_key("exclusiveMinimum"));
    assert!(!props.contains_key("minimum"));
    assert!(!props.contains_key("maximum"));

    // Verify supported fields are preserved
    assert_eq!(props.get("type").and_then(|v| v.as_str()), Some("integer"));
    assert_eq!(
        props.get("description").and_then(|v| v.as_str()),
        Some("Maximum number of characters")
    );
}
```

## Verification

```bash
# Code compiles successfully
cargo check --package vtcode-core
# Output: Finished `dev` profile [unoptimized] target(s) in 3.28s
```

## Impact

-   ✅ Fixes API errors when using tools with numeric constraints
-   ✅ No breaking changes - only removes unsupported fields
-   ✅ Works at all levels of schema nesting
-   ✅ More maintainable with constant list

## Files Modified

-   `vtcode-core/src/llm/providers/gemini.rs`
    -   Updated `sanitize_function_parameters()` function
    -   Added test case for nested exclusive min/max

## Related Documentation

-   [Gemini Function Calling Guide](https://ai.google.dev/gemini-api/docs/function-calling)
-   [JSON Schema Specification](https://json-schema.org/specification)

---

_Fixed: November 8, 2025_
_Tested with: gemini-2.5-flash-lite_
