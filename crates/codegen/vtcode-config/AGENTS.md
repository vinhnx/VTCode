<!-- markdownlint-disable MD013 -->
# vtcode-config

[Root AGENTS.md](../../../AGENTS.md) | Config loading, schema, and constants, with `vtcode.toml` as the source of truth.

## Modules

- `loader/` ConfigManager, ConfigBuilder, layers (`manager/` loading/security/repair, `manager/persistence.rs` saves, `config.rs`, `bootstrap.rs`) | `core/` AgentConfig and nested configs (`agent/`, `provider_override.rs`, `anthropic_speed.rs`).
- `constants/` models, env vars, URLs, tools, limits, `ui.rs` | `models/` ModelId + Provider | `types/` | `defaults/` | `schema.rs` (feature-gated JSON Schema).
- `api_keys.rs` + `api_keys/credential_resolution.rs` | `mcp/`, `webmcp.rs`, `acp.rs`, `hooks.rs`, `subagents/`, `network_allowlist.rs`, `accessibility.rs`.

## Rules

- Repository-controlled Workspace/Project layers cannot define non-empty `custom_providers`, override `provider_overrides.*.base_url`/`api_key_env`, or set `auth.copilot.command`; origin checks run before provider registration, malformed or symlinked files fail closed, and explicit config files are never repaired.
- Workspace lifecycle hooks are built only by `ConfigManager::collect_workspace_lifecycle_hooks` at load and gate behind per-workspace approval, so never construct them outside load.
- `ConfigLayerStack` handles layering (defaults, file, env, CLI) and must not be bypassed.
- `ModelId` is the canonical model identifier, and all model matching must go through it.
- Resolve credentials through `api_keys::resolve_credential_with_mode` so environment, `.env`, OAuth, secure storage, and legacy migration share one precedence path.
- `webmcp` is separate from `mcp`, disabled by default, loopback-only, requires HTTPS remote MCP URLs, and stores only an environment-variable name for its proxy token.
- `constants/ui.rs` holds only values read outside `vtcode-ui`, and TUI-only knobs stay in `vtcode-ui` so a shadowed copy never silently duplicates a value.

## Adding a Model

OpenRouter: edit `ModelId`, the `Provider::OpenRouter` match, and `docs/models.json` (build script does the rest); others are manual, see the `adding-llm-providers` skill.

## Adding a Setting

New `UiConfig` field: serde default fn + `Default` impl entry, then `python3 scripts/generate_config_field_reference.py`.
Enum values `/config` cycles come from quoted `"..."` strings after `Options:` in the field doc; add the path to its curated group in `src/agent/runloop/unified/settings_interactive/items.rs`.

## Gotchas

- Changing token-efficiency or timeout defaults requires updating `docs/config/CONFIG_FIELD_REFERENCE.md` and the guard-rail tests.
- `accessibility.rs` Windows FFI is never compile-checked on other hosts, so keep its `#[expect(unsafe_code, ...)]` and `// SAFETY:` attached.

Details: [maintainer notes](../../../docs/development/maintainer-notes/vtcode-config.md) — read the section for the subsystem you change before editing it.
