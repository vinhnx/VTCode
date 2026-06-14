# vtcode-llm

LLM provider abstraction, client implementations, and streaming for VT Code.

## Overview

Provides a unified interface for multiple LLM providers (Gemini, OpenAI, Anthropic, DeepSeek, Ollama, etc.) with streaming, tool calling, and provider-specific adaptations.

## Key Modules

| Module | Purpose |
|--------|---------|
| `provider/` | Core trait (`LLMProvider`), message types, request/response |
| `providers/` | Per-provider implementations (gemini, openai, anthropic, etc.) |
| `factory_types.rs` | `ProviderConfig` struct, `infer_provider_from_model` |
| `provider_config_types.rs` | Standalone `ProviderConfig` (no CGP deps) |
| `system_prompt.rs` | System prompt injection via `OnceLock` callbacks |
| `http_client.rs` | `HttpClientFactory` with timeout configuration |
| `lightweight_routing.rs` | Model routing for lightweight/fast inference |
| `model_resolver.rs` | Model name resolution and provider detection |
| `tool_bridge.rs` | Tool execution correlation and intent tracking |
| `rig_adapter.rs` | Rig-core adapter for structured output |
| `capabilities.rs` | Provider capability detection |

## Architecture Notes

- This is a **partial extraction** from vtcode-core. Integration-point files remain in vtcode-core.
- `ProviderConfig` is defined here AND in vtcode-core's `factory.rs` (identical fields). They will merge when CGP integration is decoupled.
- `system_prompt.rs` provides stub getters with `OnceLock` setters. vtcode-core overrides at init.
- Provider trait uses `#[cfg(feature = "copilot")]` and `#[cfg(feature = "anthropic-api")]` gates.

## Dependencies

- `vtcode-commons` (HTTP, CGP, error types, model families, CompactStr)
- `vtcode-config` (provider config, timeouts, auth)
- `vtcode-utility-tool-specs` (apply_patch schemas)

## Coding Conventions

- Provider implementations go in `providers/<name>/mod.rs`
- Use `anyhow::Result` for fallible operations
- Use `tracing` for logging, not `println!`
- Provider-specific types stay in their provider module
- Shared types go in `types.rs` or `provider/`
