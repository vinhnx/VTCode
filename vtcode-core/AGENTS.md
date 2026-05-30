# vtcode-core

[Root AGENTS.md](../AGENTS.md) | Largest crate (~70 modules). Agent loop, tools, LLM, config, safety, UI.

## Key Modules

`core/agent/` runtime | `llm/` + `models_manager/` providers | `tools/` + `tool_policy.rs` registry | `safety/` + `sandboxing/` + `exec_policy/` + `command_safety/` policies | `config/` + `constants.rs` | `context/` + `memory/` conversation | `prompts/` | `exec/events/` (re-exports `vtcode-exec-events::ThreadEvent`)

## Rules

- Re-export from `lib.rs`. Consumers must not reach into submodules.
- `ThreadEvent` lives in `vtcode-exec-events` — never duplicate.
- `exec_policy` (Codex policy) != `command_safety` (tree-sitter validation) — do not merge.
- Constants in `config::constants`, not inline.
- Feature gates at module level, not scattered `#[cfg]`.

## Adding a Tool

Implement in `tools/` → register in `tools::registry` → name in `tools::names` → classify in `ToolPolicy` → wire in `core/agent/`.

## Adding an LLM Provider

Use `adding-llm-providers` skill. Update `ModelId::all_models()` + `builtin_model_presets()`.

## Gotchas

- `lib.rs` is 500+ lines — append re-exports, don't restructure.
- `#[cfg_attr(not(test), allow(...))]` clippy suppressions — do not remove.
- `anthropic_api/`, `gemini/` are facades; real code in `llm/providers/`.
