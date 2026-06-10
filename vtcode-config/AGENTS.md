# vtcode-config

[Root AGENTS.md](../AGENTS.md) | Config loading, schema, constants. `vtcode.toml` is the source of truth.

## Modules

`loader/` ConfigManager + ConfigBuilder + layers | `constants/` models, env vars, URLs, tools | `core/` AgentConfig + all nested config structs | `models/` ModelId + Provider enums | `types/` ReasoningEffortLevel and related enums | `schema/` JSON Schema export (feature-gated) | `defaults/` ConfigDefaultsProvider | `auth/` auth config re-exports | `mcp/` MCP config | `acp/` ACP config | `hooks/` lifecycle hooks | `subagents/` subagent discovery

## Rules

- `ModelId` enum is the canonical model identifier — all model matching must go through it.
- `constants/` is organized by domain: `models/`, `urls.rs`, `env_vars.rs`, `tools.rs`.
- `ConfigLayerStack` handles layered config (defaults → file → env → CLI) — do not bypass.
- `bootstrap` feature (default) scaffolds config dirs. Disable for parse-only consumers.
- `schema` feature gates `vtcode_config_schema_json()` — used by `build.rs`.

## Adding a Model

Two pathways depending on provider:

### OpenRouter (code-generated)

`build.rs` reads `docs/models.json` `openrouter` section and generates constants, parsing, display, description, and metadata. Only 3 files need manual edits:

1. Add `ModelId` variant in `models/model_id.rs`.
2. Add to `Provider::OpenRouter` match arm in `models/model_id/provider.rs`.
3. Add entry with `vtcode` metadata block in `docs/models.json`.

Do NOT edit `as_str.rs`, `display.rs`, `description.rs`, `parse.rs`, `collection.rs`, or `constants/models/openrouter.rs` — the build script handles those.

### Non-OpenRouter (manual)

1. Add constant in `constants/models/<provider>.rs` (+ `SUPPORTED_MODELS`, `REASONING_MODELS` if applicable).
2. Add `ModelId` variant + all match arms in `models/model_id/` (enum, collection, provider, as_str, display, description, parse, capabilities).
3. Update `models/model_id/defaults.rs` only if changing the provider's default model.
4. Add preset in `vtcode-core/src/models_manager/model_presets.rs` if the model should appear in the `/model` picker.
5. If the model uses a new org prefix, update `heuristic_provider_from_model` in `vtcode-core/src/llm/model_resolver.rs`.
6. Add entry in `docs/models.json` — `build.rs` generates capability tables from it.
7. See `adding-llm-providers` skill for full checklist.

## Gotchas

- `VTCodeConfig::load()` resolves layers — do not use `ConfigManager::load_from_workspace()` directly in production code.
