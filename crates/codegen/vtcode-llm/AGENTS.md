<!-- markdownlint-disable MD013 -->
# vtcode-llm

[Root AGENTS.md](../../../AGENTS.md) | Canonical home for the LLM provider trait, shared types, and every provider implementation; core's `llm/` is only a re-export and factory layer.

## Modules

- `provider/` trait and shared types (`decisions.rs`, `responses_continuation.rs`); `provider.rs` re-exports.
- `providers/<name>/` per-provider impls; `providers/common.rs` and `providers/shared/mod.rs` facades over shared request, streaming, SSE, usage, and tool-history helpers; `providers/openai_compat.rs` OpenAI-compatible shell; `providers/custom_provider.rs` custom profile router.
- `open_responses/` bridge; `copilot/` (feature-gated); `client.rs` and `capabilities.rs` capability detection; `model_resolver.rs`; `usage_cost.rs`; `system_prompt.rs`; `process_env.rs` child-process environment policy.

## Rules

- New OpenAI-compatible providers implement an `OpenAiCompatSpec` rather than a full `LLMProvider`, and keep the type name and 7-arg `from_config` consumed by `impl_standard_provider_constructor!` in vtcode-core.
- Reasoning controls are route-specific: send `reasoning_effort` or `thinking.budget_tokens` only on routes advertising them, never a generic provider-wide field.
- Provider tool formatters return `Result<Option<Value>, LLMError>` and must never hide serialization failures or let extension maps collide with reserved wire fields.
- Provider error bodies are read with a 16 KiB cap and passed through the secret-redacting sanitizer before metadata, display, or logs.
- Provider-owned child processes use the shared filtered environment, and WebSocket TLS uses an explicit per-connection Rustls provider while preserving trust roots and certificate verification.
- `ResponsesContinuationStore` owns continuation-chain semantics for both runloop and headless runner, so never re-implement the map in a caller.
- `ModelResolver::resolve_with_mode` and `availability_with_mode` must receive the loaded config's credential storage mode, and availability must not be inferred from provider alone.
- `ToolChoice::None` must disable callable tools on the wire, and Decisions uses a separate no-redirect client that fails closed.
- Anthropic Messages routes never send `temperature`/`top_p`/`top_k` (standalone auxiliary requests included), and Claude 4.6+/5.x profiles never send assistant prefill or forced `tool_choice`, fallbacks included.
- After output has been exposed, streaming failures must fail without replay, and only explicit endpoint unavailability may disable Responses.

## Gotchas

- The runloop's stream-timeout fallback keys on `supports_non_streaming`, so providers that support non-streaming must override it explicitly.
- Model normalization happens in `core.prepare()`, not `convert_request()`, so compat payload tests must call `prepare` first.
- Gemini rejects `systemInstruction`/`tools`/`toolConfig` beside `cachedContent`, so requests with a non-default tool config must skip the explicit cache.

Details: [maintainer notes](../../../docs/development/maintainer-notes/vtcode-llm.md) — read the section for the subsystem you change before editing it.
