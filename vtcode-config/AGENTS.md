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
- `models/model_id/table.rs` (`model_id_table!`) is the single source for as_str/parse/display/description/provider per variant — add new models as one table row, never a new match arm in the wrapper files.
- `parse.rs` keeps an order-sensitive hand-written preamble (opencode/evolink prefix routing, ZAI shadow guards, dated-haiku remap) before the table lookup — never move prefix rules into the table.
- `core/automation.rs` holds the loop-engineering config surface: `LoopEngineConfig` (gated by `loop_engine_enabled()`, override with `VTCODE_DISABLE_LOOP_ENGINE`), and `verify_mutations` on `FullAutoConfig` — **default off** because the verifier sub-agent doubles mutating-call cost.
- `AgentHarnessConfig` now has `context_reset_mode` (`off`/`on_stall`/`on_compaction`) and `context_reset_stall_threshold` — distinct from compaction config. Default: `off`.
