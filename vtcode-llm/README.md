# vtcode-llm

Prototype extraction of VT Code's unified LLM client layer.

`vtcode-llm` provides a thin façade over `vtcode-core`'s LLM subsystem,
re-exporting provider abstractions, request/response types, and a
[`ProviderConfig`](src/config.rs) trait so downstream consumers can supply
their own configuration without depending on VT Code's internal dot-config
structures.

## Usage

```toml
[dependencies]
vtcode-llm = { path = "../vtcode-llm", default-features = false, features = ["openai"] }
```

```rust
use vtcode_llm::{AnyClient, make_client, BackendKind, LLMRequest, Message, MessageRole};

// Build a client for the desired backend.
let client: AnyClient = make_client(BackendKind::OpenAI)?;

// Compose a request.
let request = LLMRequest {
    messages: vec![Message {
        role: MessageRole::User,
        content: "Hello from vtcode-llm!".into(),
        ..Default::default()
    }],
    ..Default::default()
};
```

## Feature Flags

| Flag | Default | Description |
|------|---------|-------------|
| `anthropic` | yes | `AnthropicProvider` export |
| `deepseek` | yes | `DeepSeekProvider` export |
| `google` | yes | `GeminiProvider` export |
| `moonshot` | yes | `MoonshotProvider` export |
| `ollama` | yes | `OllamaProvider` export |
| `openai` | yes | `OpenAIProvider` export |
| `openrouter` | yes | `OpenRouterProvider` export |
| `zai` | yes | `ZAIProvider` export |
| `functions` | yes | Function/tool-calling types (`ToolCall`, `ToolChoice`, `FunctionDefinition`, …) |
| `telemetry` | | Streaming telemetry helpers (`StreamTelemetry`, `StreamDelta`, …) |
| `mock` | | `StaticResponseClient` for testing (pulls `async-trait`) |

## API Reference

### Core re-exports

- `AnyClient`, `make_client` — build a provider-agnostic LLM client
- `create_provider_with_config`, `get_factory` — factory helpers
- `BackendKind`, `LLMError`, `Usage`, `LLMResponse` — shared types
- `ErrorFormatter`, `ErrorReporter`, `PathResolver`, `TelemetrySink`, `WorkspacePaths` — common utilities

### `provider` module

`LLMProvider`, `LLMRequest`, `LLMResponse`, `LLMStream`, `LLMStreamEvent`, `Message`, `MessageRole`, `ParallelToolConfig`

### `config` module

`ProviderConfig` trait — implement on your own types to supply provider configuration without coupling to VT Code internals.

### `providers` module

Feature-gated, provider-specific exports (one module per backend).

## Related docs

- [LLM environment guide](../docs/modules/vtcode_llm_environment.md)
