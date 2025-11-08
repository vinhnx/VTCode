# GLM-4.6 Rate Limit Error Fix

## Problem

Users were experiencing rate limit errors with the GLM-4.6 model from Z.AI with the error message:

```
# GLM-4.6 Rate Limit Error Fix

## Problem

Users were experiencing rate limit errors with the GLM-4.6 model from Z.AI with the error message:
```

provider error: rate limit exceed

````

The error detection in the Z.AI provider was not comprehensive enough to catch all rate limit error patterns.

## Root Cause

According to the [Z.AI API documentation](https://docs.z.ai/api-reference/api-code.md), HTTP 429 errors can be returned for multiple scenarios:

1. Interface request concurrency exceeded
2. File upload frequency too fast
3. Account balance exhausted
4. Account anomaly

The original error detection only checked for:
- HTTP status code 429
- Generic "rate" substring in the error message

This missed specific error messages and error codes defined by Z.AI.

## Solution

Enhanced the rate limit error detection in `vtcode-core/src/llm/providers/zai.rs` with a comprehensive approach:

### 1. Z.AI Error Code Detection

Z.AI returns errors in a structured JSON format:
```json
{
  "error": {
    "code": "1302",
    "message": "High concurrency usage of this API"
  }
}
````

The fix now detects specific rate limit error codes:

-   **1302**: High concurrency usage of this API
-   **1303**: High frequency usage of this API
-   **1304**: Daily call limit for this API reached
-   **1308**: Usage limit reached (includes reset time)
-   **1309**: GLM Coding Plan package has expired

### 2. Improved Error Message Pattern Detection

The fix checks for the following patterns (case-insensitive):

1. `rate limit` / `rate_limit` / `ratelimit` - Standard rate limit messages
2. `concurrency` - High concurrency errors
3. `frequency` - High frequency errors
4. `balance exhausted` - Account balance errors
5. `quota` - Quota-related errors
6. `usage limit` - Usage limit errors
7. `too many requests` - HTTP 429 standard message
8. `daily call limit` - Daily limit errors
9. `package has expired` - Subscription expiration

### 3. Enhanced JSON Error Parsing

The fix properly parses the Z.AI error JSON structure:

```rust
let (error_code, message) = serde_json::from_str::<Value>(&text)
    .ok()
    .and_then(|value| {
        let error_obj = value.get("error")?;
        let code = error_obj.get("code")?.as_str()?;
        let msg = error_obj.get("message")?.as_str()?;
        Some((code.to_string(), msg.to_string()))
    })
    .unwrap_or_else(|| (String::new(), text.clone()));
```

This handles the proper Z.AI error structure and falls back to text parsing for non-JSON responses.

````

The error detection in the Z.AI provider was not comprehensive enough to catch all rate limit error patterns.

## Root Cause

According to the [Z.AI API documentation](https://docs.z.ai/api-reference/api-code.md), HTTP 429 errors can be returned for multiple scenarios:

1. Interface request concurrency exceeded
2. File upload frequency too fast
3. Account balance exhausted
4. Account anomaly

The original error detection only checked for:

-   HTTP status code 429
-   Generic "rate" substring in the error message

This missed specific error messages like "rate limit exceed" (note: "exceed" not "exceeded"), "concurrency exceeded", "balance exhausted", and other rate-limit-related patterns.

## Solution

Enhanced the rate limit error detection in `vtcode-core/src/llm/providers/zai.rs` to check for multiple patterns:

### Improved Error Detection Patterns

The fix now checks for the following patterns (case-insensitive):

1. `rate limit` - Standard rate limit message
2. `rate_limit` - Underscore variant
3. `ratelimit` - No space variant
4. `concurrency exceeded` - Z.AI specific concurrency error
5. `balance exhausted` - Z.AI account balance error
6. `quota` - Quota-related errors
7. `too many requests` - Common HTTP 429 message

### Enhanced JSON Error Parsing

The fix also improves error message extraction from JSON responses by checking multiple possible locations:

```rust
value
    .get("error")
    .and_then(|e| e.get("message"))
    .and_then(|m| m.as_str())
    .or_else(|| value.get("message").and_then(|m| m.as_str()))
    .or_else(|| value.get("error").and_then(|e| e.as_str()))
````

This handles various JSON response structures:

-   `{"error": {"message": "..."}}`
-   `{"message": "..."}`
-   `{"error": "..."}`

## Changes Made

### File Modified

-   `vtcode-core/src/llm/providers/zai.rs`

### Changes

1. Added comprehensive rate limit pattern matching
2. Improved JSON error message extraction
3. Added inline documentation explaining Z.AI's HTTP 429 scenarios
4. Added unit tests for rate limit pattern detection

## Testing

Three comprehensive test cases were added:

### 1. Error Code Detection Test

```rust
#[test]
fn test_rate_limit_error_codes() {
    // Verifies Z.AI error codes 1302, 1303, 1304, 1308, 1309
    // are correctly identified as rate limit errors
}
```

### 2. Error Message Pattern Test

```rust
#[test]
fn test_rate_limit_error_patterns() {
    // Tests various rate limit error message patterns including:
    // - "rate limit exceed"
    // - "High concurrency usage"
    // - "High frequency usage"
    // - "balance exhausted"
    // - "quota exceeded"
    // - "usage limit reached"
    // - "daily call limit reached"
    // - "package has expired"
}
```

### 3. JSON Parsing Test

````rust
#[test]
fn test_error_json_parsing() {
    // Verifies Z.AI error JSON structure is correctly parsed:
    // {"error":{"code":"1302","message":"High concurrency usage"}}
}
```## Reference Documentation

-   Z.AI API Documentation: https://docs.z.ai/llms.txt
-   Z.AI Error Codes: https://docs.z.ai/api-reference/api-code.md
-   GLM-4.6 Guide: https://docs.z.ai/guides/llm/glm-4.6.md

## Related Issues

This fix addresses rate limiting issues that occur when:

-   Making too many concurrent requests to the Z.AI API
-   Exceeding API quota limits
-   Account balance is exhausted
-   Account has anomalies/violations

## Future Improvements

Consider implementing:

1. Exponential backoff retry logic for rate limit errors
2. Request throttling at the client level
3. Better rate limit error messages showing retry-after timing
4. Account balance monitoring and warnings
````
