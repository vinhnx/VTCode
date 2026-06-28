# vtcode-config

[Root AGENTS.md](../AGENTS.md) | Config loading, schema, constants. `vtcode.toml` is the source of truth.

## Modules

`loader/` ConfigManager + ConfigBuilder + layers | `constants/` models, env vars, URLs, tools | `core/` AgentConfig + all nested config structs | `models/` ModelId + Provider enums | `types/` ReasoningEffortLevel and related enums | `schema/` JSON Schema export (feature-gated) | `defaults/` ConfigDefaultsProvider | `auth/` auth config re-exports | `mcp/` MCP config | `acp/` ACP config | `hooks/` lifecycle hooks | `subagents/` subagent discovery | `core/network_allowlist.rs` | `core/provider_override.rs`

## Rules

- `ModelId` enum is the canonical model identifier — all model matching must go through it.
- `constants/` is organized by domain: `models/`, `urls.rs`, `env_vars.rs`, `tools.rs`.
- `ConfigLayerStack` handles layered config (defaults → file → env → CLI) — do not bypass.
- `bootstrap` feature (default) scaffolds config dirs. Disable for parse-only consumers.
- `schema` feature gates `vtcode_config_schema_json()` — used by `build.rs`.

## Adding a Model

Two pathways: **OpenRouter** (code-generated) — edit `ModelId`, `Provider::OpenRouter` match, `docs/models.json`. Build script handles the rest. **Non-OpenRouter** (manual) — add constant, `ModelId` variant + all match arms, defaults if needed, preset, optional resolver update, `docs/models.json`. See `adding-llm-providers` skill for checklist.

## Gotchas

- `VTCodeConfig::load()` resolves layers — do not use `ConfigManager::load_from_workspace()` directly in production code.
