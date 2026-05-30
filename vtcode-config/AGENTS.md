# vtcode-config

[Root AGENTS.md](../AGENTS.md) | Config loading, schema, constants. `vtcode.toml` is the source of truth.

## Modules

`loader/` ConfigManager + ConfigBuilder + layers | `constants/` models, env vars, URLs, tools | `core/` AgentConfig + all nested config structs | `models/` ModelId + Provider enums | `types/` EditingMode, ReasoningEffortLevel, etc. | `schema/` JSON Schema export (feature-gated) | `defaults/` ConfigDefaultsProvider | `auth/` auth config re-exports | `mcp/` MCP config | `acp/` ACP config | `hooks/` lifecycle hooks | `subagents/` subagent discovery

## Rules

- `ModelId` enum is the canonical model identifier — all model matching must go through it.
- `constants/` is organized by domain: `models/`, `urls.rs`, `env_vars.rs`, `tools.rs`.
- `ConfigLayerStack` handles layered config (defaults → file → env → CLI) — do not bypass.
- `bootstrap` feature (default) scaffolds config dirs. Disable for parse-only consumers.
- `schema` feature gates `vtcode_config_schema_json()` — used by `build.rs`.

## Adding a Model

1. Add constant in `constants/models/<provider>.rs`.
2. Add `ModelId` variant + all match arms in `models/model_id/`.
3. Update `models/model_id/defaults.rs` for orchestrator/single defaults.
4. See `adding-llm-providers` skill for full checklist.

## Gotchas

- `build.rs` generates capability tables from `docs/models.json` — update that file too.
- `VTCodeConfig::load()` resolves layers — do not use `ConfigManager::load_from_workspace()` directly in production code.
