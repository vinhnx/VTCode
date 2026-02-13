# Gemini 3 API Reference & Configuration Guide

## Table of Contents
1. [Model Selection](#model-selection)
2. [Thinking Levels](#thinking-levels)
3. [Configuration Examples](#configuration-examples)
4. [Advanced Features](#advanced-features)
5. [Troubleshooting](#troubleshooting)

## Model Selection

### Gemini 3 Pro (`gemini-3-pro-preview`)
- **Use Case**: Complex reasoning, broad knowledge, agentic workflows, autonomous coding
- **Pricing**: $2/1M tokens input, $12/1M tokens output
- **Context**: 1M input, 64K output
- **Thinking Levels**: `low`, `high` (default)
- **Knowledge Cutoff**: January 2025

```rust
let model = "gemini-3-pro-preview";
let effort = ReasoningEffortLevel::High; // Recommended for complex tasks
```

### Gemini 3 Flash (`gemini-3-flash-preview`)
- **Use Case**: Fast inference, high-throughput applications, budget-conscious
- **Pricing**: $0.50/1M tokens input, $3/1M tokens output
- **Context**: 1M input, 64K output
- **Thinking Levels**: `minimal`, `low`, `medium`, `high` (default)
- **Knowledge Cutoff**: January 2025
- **Special**: Pro-level intelligence at Flash pricing

```rust
let model = "gemini-3-flash-preview";
let effort = ReasoningEffortLevel::Medium; // Balanced reasoning + speed
```

### Gemini 3 Pro Image (`gemini-3-pro-image-preview`)
- **Use Case**: Image generation, creation, and editing
- **Pricing**: $2 input, varies by output resolution
- **Output Resolution**: Supports 2K, 4K (recommended: 4K for detail)
- **Features**: Grounded image generation, conversational editing with thought signatures

```rust
let model = "gemini-3-pro-image-preview";
// Use with imageConfig: { aspectRatio: "16:9", imageSize: "4K" }
```

## Thinking Levels

### Level Mapping & Use Cases

| Level | Flash | Pro | Latency | Use Case |
|-------|-------|-----|---------|----------|
| `minimal` | ✅ | ❌ | Very Low | Chat, high-throughput, no reasoning needed |
| `low` | ✅ | ✅ | Low | Simple instruction following, fast responses |
| `medium` | ✅ | ❌ | Medium | Balanced reasoning + speed (Flash only) |
| `high` | ✅ | ✅ | High | Deep reasoning, complex problems (default) |

### Internal API Mapping

VT Code's `ReasoningEffortLevel` → Gemini `thinking_level`:

```rust
pub enum ReasoningEffortLevel {
    None,     // → "low" (fallback)
    Minimal,  // → "minimal" (Flash only, else "low")
    Low,      // → "low"
    Medium,   // → "medium" (Flash) / "high" (Pro fallback)
    High,     // → "high"
    XHigh,    // → "high" (max available)
}
```

### Example: Configuring Thinking Level

```rust
use vtcode_config::types::ReasoningEffortLevel;
use vtcode_core::llm::provider::LLMRequest;

let request = LLMRequest {
    model: "gemini-3-flash-preview".to_string(),
    messages: vec![/* ... */],
    reasoning_effort: Some(ReasoningEffortLevel::Medium), // Balanced
    // ... other fields
};

// Provider automatically maps to: thinkingLevel: "medium"
```

## Configuration Examples

### Basic Usage

#### High-Reasoning Task (Gemini 3 Pro)
```rust
let request = LLMRequest {
    model: "gemini-3-pro-preview".to_string(),
    messages: vec![
        Message::user("Analyze this complex algorithm for race conditions...".to_string()),
    ],
    reasoning_effort: Some(ReasoningEffortLevel::High), // Max reasoning
    max_tokens: Some(4096),
    temperature: None, // Keep default 1.0
    // ...
};
```

#### Fast Inference (Gemini 3 Flash)
```rust
let request = LLMRequest {
    model: "gemini-3-flash-preview".to_string(),
    messages: vec![
        Message::user("Write a quick summary of...".to_string()),
    ],
    reasoning_effort: Some(ReasoningEffortLevel::Minimal), // Quick response
    max_tokens: Some(512),
    temperature: None, // Keep default 1.0
    // ...
};
```

#### Balanced Reasoning (Gemini 3 Flash Medium)
```rust
let request = LLMRequest {
    model: "gemini-3-flash-preview".to_string(),
    messages: vec![
        Message::user("Debug this issue and suggest fixes...".to_string()),
    ],
    reasoning_effort: Some(ReasoningEffortLevel::Medium), // Balanced
    max_tokens: Some(2048),
    temperature: None, // Keep default 1.0
    // ...
};
```

### Configuration File (vtcode.toml)

```toml
[llm]
provider = "gemini"
model = "gemini-3-flash-preview"  # Default model
temperature = 1.0                  # Keep at default for Gemini 3

[llm.gemini]
enabled = true
models = [
    "gemini-3-pro-preview",
    "gemini-3-flash-preview",
    "gemini-3-pro-image-preview",
    "gemini-3-pro",
    "gemini-3-flash"
]
default_reasoning_effort = "high"  # For Pro models
```

## Advanced Features

### Media Resolution (Image/Video Processing)

Control token allocation for multimodal input:

```json
{
  "generationConfig": {
    "mediaResolution": {
      "level": "media_resolution_high"  // or low, medium, ultra_high
    }
  }
}
```

**Recommended Settings**:
- Images: `media_resolution_high` (1120 tokens, quality saturates)
- PDFs: `media_resolution_medium` (560 tokens, optimal for OCR)
- Video: `media_resolution_low` (70 tokens per frame, for general understanding)
- Video (Text-heavy): `media_resolution_high` (280 tokens, for dense text/OCR)

### Context Caching

Reduce costs for repeated queries with similar context:

```rust
// Requires minimum 2048 cached tokens
// Automatically handled by vtcode-core when enabled
```

Enable in config:
```toml
[caching]
enabled = true

[caching.gemini]
enabled = true
mode = "implicit"  # or "explicit" for manual cache control
```

### Code Execution

Models can write and execute Python code:

```rust
// Available for all Gemini 3 and 2.5 models
// Automatically enabled when model supports it
```

### Thought Signatures (Critical for Multi-Turn)

Gemini 3 uses encrypted thought signatures to maintain reasoning context:

#### Function Calling (Strict Validation)
When the model calls a function, save and return the signature:

```json
{
  "role": "model",
  "parts": [{
    "functionCall": { "name": "tool_name", "args": {...} },
    "thoughtSignature": "<encrypted_signature>"  // MUST RETURN THIS
  }]
}
```

#### Text/Chat (Non-Strict)
Signatures are optional but recommended:

```json
{
  "role": "model",
  "parts": [{
    "text": "Let me think through this step by step...",
    "thoughtSignature": "<signature>"  // RECOMMENDED
  }]
}
```

#### Image Generation/Editing (Strict Validation)
Required for conversational image editing:

```json
{
  "role": "model",
  "parts": [
    { "text": "...", "thoughtSignature": "<sig_1>" },
    { "inlineData": {...}, "thoughtSignature": "<sig_2>" }
  ]
}
```

**VT Code Handling**: Automatically manages thought signatures when using official SDKs.

## Troubleshooting

### Error: `400 Bad Request - Invalid request`

**Cause 1**: Mixing `thinking_level` with deprecated `thinking_budget`
```rust
// ❌ WRONG
{
  "generationConfig": {
    "thinkingConfig": {
      "thinkingLevel": "high",
      "thinkingBudget": 10000  // Don't use both!
    }
  }
}

// ✅ CORRECT
{
  "generationConfig": {
    "thinkingConfig": {
      "thinkingLevel": "high"
    }
  }
}
```

**Cause 2**: Missing thought signature in function calling
```rust
// Ensure you're returning all thought signatures from the model's response
// in the next request, especially for sequential function calls
```

### Looping or Degraded Performance

**Cause**: Non-default temperature setting
```rust
// ❌ DON'T do this:
temperature: Some(0.7)  // Can cause looping on complex tasks

// ✅ DO this:
temperature: None  // Uses Gemini 3 default of 1.0
```

### Low-Quality Reasoning

**Solution**: Increase thinking level
```rust
// If results are poor with Low effort:
reasoning_effort: Some(ReasoningEffortLevel::High)  // Default for Pro

// For Flash, try Medium:
reasoning_effort: Some(ReasoningEffortLevel::Medium)
```

### High Token Usage on PDFs

**Cause**: New OCR resolution defaults
**Solution**: Explicitly set lower media resolution:
```rust
// mediaResolution: "media_resolution_medium"  // 560 tokens instead of default
```

## API Reference Links

- [Gemini 3 Developer Guide](https://ai.google.dev/gemini-api/docs/gemini-3)
- [Models Documentation](https://ai.google.dev/gemini-api/docs/models/gemini)
- [Thinking Levels Guide](https://ai.google.dev/gemini-api/docs/gemini-3#thinking-level)
- [Thought Signatures](https://ai.google.dev/gemini-api/docs/thought-signatures)
- [Media Resolution](https://ai.google.dev/gemini-api/docs/media-resolution)
- [Pricing](https://ai.google.dev/gemini-api/docs/pricing)

## Implementation Details

### Constants Location
- Model IDs: `vtcode-config/src/constants.rs` (module `models::google`)
- Model capabilities: Same file (REASONING_MODELS, EXTENDED_THINKING_MODELS, etc.)

### Provider Implementation
- File: `vtcode-core/src/llm/providers/gemini.rs`
- Thinking level mapping: Lines 633-675
- Helper methods: `supports_extended_thinking()`, `supported_thinking_levels()`

### Type Definitions
- `ReasoningEffortLevel`: `vtcode-config/src/types.rs`
- Model IDs: `vtcode-config/src/models.rs`
