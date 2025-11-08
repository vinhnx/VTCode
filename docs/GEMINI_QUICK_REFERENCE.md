# Gemini API Quick Reference

## Default Model

```rust
// Stable, production-ready default
"gemini-2.5-flash"
```

## All Supported Models

```rust
// Stable models (recommended for production)
"gemini-2.5-pro"           // Most capable, higher cost
"gemini-2.5-flash"         // Best price/performance ⭐
"gemini-2.5-flash-lite"    // Fastest, lowest cost

// Preview models (for testing/early access)
"gemini-2.5-flash-preview-05-20"
```

## Model Capabilities

All Gemini 2.5 models support:

-   ✅ Thinking/Reasoning
-   ✅ Function Calling
-   ✅ Context Caching (1M tokens)
-   ✅ Code Execution
-   ✅ Batch API
-   ✅ Structured Output
-   ✅ Search Grounding
-   ✅ URL Context

## Token Limits

| Model Series | Input Tokens | Output Tokens |
| ------------ | ------------ | ------------- |
| Gemini 2.5   | 1,048,576    | 65,536        |
| Gemini 2.0   | 1,048,576    | 8,192         |

## Usage

```rust
use vtcode_core::llm::providers::GeminiProvider;
use vtcode_config::constants::models;

// Default (recommended)
let provider = GeminiProvider::new(api_key);

// Specific model
let provider = GeminiProvider::with_model(
    api_key,
    models::google::GEMINI_2_5_PRO.to_string()
);

// Check capabilities
let has_reasoning = GeminiProvider::supports_reasoning(model);
let has_caching = GeminiProvider::supports_caching(model);
let has_code_exec = GeminiProvider::supports_code_execution(model);

// Get limits
let max_in = GeminiProvider::max_input_tokens(model);  // 1,048,576
let max_out = GeminiProvider::max_output_tokens(model); // 65,536
```

## Error Handling

| Error Type      | HTTP Code | Description                 |
| --------------- | --------- | --------------------------- |
| Authentication  | 401, 403  | Invalid API key             |
| Rate Limit      | 429       | Too many requests           |
| Invalid Request | 400       | Malformed request           |
| Quota Exceeded  | 429       | Daily/monthly quota reached |

## Environment Variables

```bash
# Either of these work
export GOOGLE_API_KEY="your-api-key"
export GEMINI_API_KEY="your-api-key"
```

## Constants Reference

```rust
use vtcode_config::constants::models;

// Model IDs
models::google::GEMINI_2_5_PRO
models::google::GEMINI_2_5_FLASH
models::google::GEMINI_2_5_FLASH_LITE
models::google::GEMINI_2_5_FLASH_PREVIEW

// Capability arrays
models::google::SUPPORTED_MODELS
models::google::REASONING_MODELS
models::google::CACHING_MODELS
models::google::CODE_EXECUTION_MODELS
models::google::DEFAULT_MODEL
```

## Best Practices

1. **Use Stable Models** - Prefer `gemini-2.5-flash` over preview versions
2. **Validate Tokens** - Check limits before making requests
3. **Handle Rate Limits** - Implement exponential backoff
4. **Check Capabilities** - Use helper methods before enabling features
5. **Secure API Keys** - Use environment variables, never hardcode

## Quick Model Selection Guide

**Need best quality?** → `gemini-2.5-pro`
**Need best value?** → `gemini-2.5-flash` ⭐
**Need lowest latency?** → `gemini-2.5-flash-lite`
**Need cutting edge?** → `gemini-2.5-flash-preview-05-20`

## API Endpoints

-   **Base URL**: `https://generativelanguage.googleapis.com/v1beta`
-   **Generate**: `/models/{model}:generateContent`
-   **Stream**: `/models/{model}:streamGenerateContent`

## Documentation Links

-   Models: https://ai.google.dev/gemini-api/docs/models
-   Thinking: https://ai.google.dev/gemini-api/docs/thinking
-   Caching: https://ai.google.dev/gemini-api/docs/caching
-   Function Calling: https://ai.google.dev/gemini-api/docs/function-calling

---

_Last Updated: November 8, 2025_
