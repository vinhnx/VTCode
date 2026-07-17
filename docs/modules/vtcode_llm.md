# vtcode-llm

LLM provider abstraction, client implementations, and streaming for VT Code.

## Overview

Provides a unified interface for multiple LLM providers with streaming, tool calling, and provider-specific adaptations. This is a partial extraction from `vtcode-core`; integration-point files remain there.

## Key Modules

| Module | Purpose |
|--------|---------|
| `providers/` | Per-provider implementations (gemini, openai, anthropic, deepseek, ollama, etc.) |
| `provider/` | Core trait (`LLMProvider`), message types, request/response |
| `factory_types.rs` | `ProviderConfig` struct, `infer_provider_from_model` |
| `system_prompt.rs` | System prompt injection via `OnceLock` callbacks |
| `http_client.rs` | `HttpClientFactory` with timeout configuration |
| `lightweight_routing.rs` | Model routing for lightweight/fast inference |
| `model_resolver.rs` | Model name resolution and provider detection |
| `tool_bridge.rs` | Tool execution correlation and intent tracking |
| `rig_adapter.rs` | Rig-core adapter for structured output |
| `capabilities.rs` | Provider capability detection |

## Supported Providers

| Provider | Module | Key Models |
|----------|--------|------------|
| Google Gemini | `providers/gemini/` | Gemini 3.1 Pro, Gemini 3.5 Flash |
| OpenAI | `providers/openai/` | GPT-5.4, GPT-5.5, GPT-5.3 Codex |
| Anthropic | `providers/anthropic/` | Claude Opus 4.8, Claude Sonnet 4.6 |
| DeepSeek | `providers/deepseek.rs` | DeepSeek V4 Pro, V4 Flash |
| Z.AI | `providers/zai.rs` | GLM-5.2, GLM-5.1, GLM-4.7 |
| Moonshot | `providers/moonshot.rs` | Kimi K2.7 Code, K2.6, K2.5 |
| StepFun | `providers/stepfun.rs` | Step-3.7-Flash |
| MiniMax | `providers/minimax.rs` | MiniMax-M3, M2.7, M2.5 |
| Ollama | `providers/ollama/` | Local and cloud models |
| OpenRouter | `providers/openrouter/` | Marketplace models |
| Evolink | `providers/evolink.rs` | Multi-model gateway |
| HuggingFace | `providers/huggingface.rs` | Router-based models |
| Mistral | `providers/mistral.rs` | Mistral models |
| Qwen | `providers/qwen.rs` | Qwen models |
| MiMo | `providers/mimo.rs` | MiMo V2.5, V2.5 Pro |

## Architecture Notes

- `ProviderConfig` is defined here AND in vtcode-core (identical fields) — will merge when CGP integration is decoupled
- `system_prompt.rs` provides stub getters with `OnceLock` setters; vtcode-core overrides at init
- Provider trait uses feature gates: `#[cfg(feature = "copilot")]`, `#[cfg(feature = "anthropic-api")]`

## Dependencies

- `vtcode-commons` — HTTP, CGP, error types, model families, CompactStr
- `vtcode-config` — provider config, timeouts, auth
- `vtcode-utility-tool-specs` — apply_patch schemas

## Coding Conventions

- Provider implementations go in `providers/<name>/mod.rs`
- Use `anyhow::Result` for fallible operations
- Use `tracing` for logging, not `println!`
- Provider-specific types stay in their provider module
- Shared types go in `types.rs` or `provider/`

## See Also

- [Provider Guides](../providers/PROVIDER_GUIDES.md) — setup and configuration
- [Model Catalog](../models.json) — full model listing with capabilities
- `crates/codegen/vtcode-core/src/config/models.rs` — model constants re-export
