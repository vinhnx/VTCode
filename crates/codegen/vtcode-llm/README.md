# vtcode-llm

LLM provider abstraction layer for VT Code, providing a unified interface for
multiple LLM providers including Gemini, OpenAI, Anthropic, DeepSeek, Ollama,
and Copilot.

<!-- cargo-rdme start -->

### vtcode-llm - LLM Provider Abstraction

Provides a unified interface for multiple LLM providers including
Gemini, OpenAI, Anthropic, DeepSeek, and Ollama.

<!-- cargo-rdme end -->

## Modules

| Module | Purpose |
|---|---|
| `capabilities` | Provider capability detection and feature flags |
| `client` | Client trait and adapter implementations |
| `config_adapter` | Configuration adapter for provider settings |
| `copilot` | GitHub Copilot integration (feature-gated) |
| `error_display` | User-friendly error message formatting |
| `factory_types` | Factory pattern types for client creation |
| `http_client` | HTTP client implementations |
| `model_resolver` | Model ID resolution and capability matching |
| `open_responses` | OpenAI Responses API support |
| `optimized_client` | Optimized client for high-throughput scenarios |
| `provider` | Provider trait and core types |
| `providers` | Provider-specific implementations |
| `rig_adapter` | Rig framework integration |
| `system_prompt` | System prompt construction |
| `tool_bridge` | Tool call bridging between LLM and local tools |
| `types` | Core LLM types (messages, responses, streaming) |
| `utils` | Shared utilities |

## Features

- **Multi-provider support**: Unified interface for 6+ LLM providers
- **Streaming**: Real-time response streaming
- **Tool calling**: Structured tool call support
- **Copilot integration**: GitHub Copilot OAuth flow (feature-gated)
- **Model resolution**: Automatic model capability detection

## Dependencies

- `vtcode-commons` (HTTP, error types, model families, CompactStr)
- `vtcode-config` (provider config, timeouts, auth)
- `vtcode-utility-tool-specs` (apply_patch schemas)
