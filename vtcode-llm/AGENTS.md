# vtcode-llm

[Root AGENTS.md](../AGENTS.md) | Public LLM abstraction with decoupled config traits for downstream consumers.

## Modules

`config` ProviderConfig + AdapterHooks + OwnedProviderConfig | `provider` re-exports | `providers` feature-gated per-provider | `telemetry` streaming helpers | `mock` StaticResponseClient

## Rules

- `ProviderConfig` trait is the extension point. `as_factory_config()` converts to core type.
- `AdapterHooks` handles prompt cache resolution — don't add resolution to `ProviderConfig` itself.
- Use `OwnedProviderConfig` in tests, never depend on dot-config.
- Feature flags per provider: `anthropic`, `openai`, `google`, `deepseek`, `ollama`, `openrouter`, `moonshot`, `zai`.

## Adding a Provider

Implementation in `vtcode-core::llm::providers` → feature flag here → re-export in `providers` module → use `adding-llm-providers` skill.

## Gotchas

- `vtcode_core::llm::factory::ProviderConfig` also implements `ProviderConfig` — intentional, not a bug.
