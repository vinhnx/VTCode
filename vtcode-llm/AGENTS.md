# vtcode-llm

[Root AGENTS.md](../AGENTS.md) | LLM provider abstraction, client implementations, streaming.

## Key Modules

`provider/` core trait | `providers/` per-provider impls | `factory_types.rs` + `provider_config_types.rs` config | `system_prompt.rs` injection | `http_client.rs` | `lightweight_routing.rs` | `model_resolver.rs` | `tool_bridge.rs` | `rig_adapter.rs` | `capabilities.rs`

## Architecture Notes

- **Partial extraction** from vtcode-core. `ProviderConfig` duplicates vtcode-core's `factory.rs` — will merge when CGP decouples.
- `system_prompt.rs` provides stub getters with `OnceLock` setters; vtcode-core overrides at init.

## Dependencies

`vtcode-commons` (HTTP, CGP, types) | `vtcode-config` (provider config, timeouts) | `vtcode-utility-tool-specs` (schemas)

## Coding Conventions

Providers in `providers/<name>/mod.rs`. Use `anyhow::Result`, `tracing`, not `println!`. Provider-specific types stay local; shared go in `types.rs` or `provider/`.
