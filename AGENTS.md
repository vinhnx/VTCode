# AGENTS.md

## Use This File For

- Repo-wide workflow and placement decisions.
- Open `docs/ARCHITECTURE.md` only when the task spans crates, touches runtime boundaries, or you need repo orientation.
- Prefer module-local docs over broad repo exploration when working in one area.

## Core Rules

- Keep changes surgical and behavior-preserving.
- Use Conventional Commits (`type(scope): subject`).
- Match CI expectations in `.github/workflows/`.
- Rust uses 4-space indentation, snake_case functions, PascalCase types, and `anyhow::Result<T>` with `.with_context()` on fallible paths.
- Measure before optimizing.
- Consult [`docs/development/rust-performance-principles.md`](docs/development/rust-performance-principles.md) for Rust-specific performance guidance (aliasing, destructive moves, iterator elision, overflow checking near-zero cost, `#[cold]` strategy, and safety-enables-aggressive-optimization patterns).
- In hot Rust paths, treat `Cow` as conditional ownership, not a free borrow: if values are always borrowed or stored in dense token/AST-style enums, prefer `&str`/slices and compact variants unless measurement shows `Cow` pays for itself.
- Prefer ownership and borrowing by default; introduce `Rc<T>`/`Arc<T>` only for genuine shared ownership.
- When ownership or lifetimes get tangled, first prefer explicit handles/IDs plus an owning context.
- Use `Rc<T>` only for single-threaded sharing and `Arc<T>` only for cross-thread/task sharing; prefer immutable sharing and narrowly scoped interior mutability.
- Break back-references or task-parent links with `Weak<T>`/`Arc::downgrade()` so cycles do not leak memory or keep state alive unexpectedly.
- Do not reach for raw pointers, custom `Send`/`Sync`, or lifetime-branding patterns unless simpler handle-based designs are insufficient; document the invariant if you do.
- If this repo includes or adds C/C++ surfaces, follow [`docs/development/CPP_CORE_GUIDELINES_ADOPTION.md`](docs/development/CPP_CORE_GUIDELINES_ADOPTION.md).

## Verification

Use `./scripts/check-dev.sh` during development. Do not use `./scripts/check.sh` for routine iteration.

| If you changed...                                                        | Run                                    |
| ------------------------------------------------------------------------ | -------------------------------------- |
| A focused code path and you want the default fast gate                   | `./scripts/check-dev.sh`               |
| Logic covered by tests or you added tests                                | `./scripts/check-dev.sh --test`        |
| Multiple crates or shared code                                           | `./scripts/check-dev.sh --workspace`   |
| Extra lint-sensitive code paths                                          | `./scripts/check-dev.sh --lints`       |
| GitHub workflows or workflow-security-sensitive scripts                  | `./scripts/check.sh workflow-security` |
| Ast-grep rules or scan scaffolding (`sgconfig.yml`, `rules/`)            | `vtcode check ast-grep`                |
| PTY/TUI harness paths called out in `docs/harness/QUALITY_SCORE.md`      | `./scripts/check.sh harness`           |
| Release validation, final PR validation, or reviewer/CI explicitly asked | `./scripts/check.sh`                   |

Use `cargo check`, `cargo nextest run`, `cargo fmt`, and `cargo clippy` when you need a narrower command for a specific crate or faster debugging loop.

## Code Placement

Repository shape:

- Main code lives in `src/`, `vtcode-core/`, `vtcode-tui/`, and `tests/`.

Choose placement before adding code:

| Situation                                                               | Preferred location                              |
| ----------------------------------------------------------------------- | ----------------------------------------------- |
| Reusable logic that does not need to live in the core runtime           | An existing smaller crate, or a new small crate |
| Code tightly coupled to existing `vtcode-core` runtime responsibilities | `vtcode-core`                                   |
| Unsure whether new reusable logic belongs in `vtcode-core`              | Keep it out of `vtcode-core` by default         |

## Implementation Notes

- Prefer simple algorithms and control flow until the workload justifies extra complexity.
- Keep new abstractions proportional to current use; do not generalize single-use code.
- Preserve existing APIs and behavior unless the task requires a change.

## Adding a New LLM Provider

When adding a new LLM provider + models, the following files must be touched in order:

| Step | File | What to do |
|------|------|------------|
| 1 | `vtcode-config/src/constants/models/<provider>.rs` | Model ID constants + `SUPPORTED_MODELS` + `DEFAULT_MODEL` |
| 2 | `vtcode-config/src/constants/models/mod.rs` | Add `pub mod <provider>;` |
| 3 | `vtcode-config/src/constants/urls.rs` | Add `*_API_BASE` constant |
| 4 | `vtcode-config/src/constants/env_vars.rs` | Add `*_BASE_URL` env var constant |
| 5 | `vtcode-config/src/constants/model_helpers.rs` | Add `"<key>" => Some(models::<provider>::...)` in both `supported_for` and `default_for` |
| 6 | `vtcode-config/src/models/provider.rs` | Add `Provider::<Name>` variant, all match arms (`label`, `display`, `from_str`, `as_ref`, `all_providers`, env key, `supports_reasoning_effort`) |
| 7 | `vtcode-core/src/llm/providers/<provider>.rs` | Provider implementation — struct, `LLMProvider` impl, `LLMClient` impl |
| 8 | `vtcode-core/src/llm/providers/mod.rs` | Add `pub mod <provider>;` + re-export |
| 9 | `vtcode-core/src/llm/provider_config.rs` | Add `define_provider_config!(<Name>ProviderConfig, ...)` |
| 10 | `vtcode-core/src/llm/cgp.rs` | Add import + `crate::delegate_components!(...)` + `register_builtin_cgp_providers` entry |
| 11 | `vtcode-core/src/llm/factory.rs` | Add provider key to `BUILTIN_PROVIDER_KEYS` if needed |
| 12 | `vtcode-core/src/llm/client.rs` | Add backend kind if new variant needed |
| 13 | `vtcode-commons/src/llm.rs` | Add `BackendKind::<Name>` variant |
| 14 | `vtcode-config/src/models/model_id.rs` | Add `ModelId` enum variants (CRITICAL: model picker uses `ModelId::all_models()`, not presets) |
| 15 | `vtcode-config/src/models/model_id/collection.rs` | Add to `all_models()` |
| 16 | `vtcode-config/src/models/model_id/provider.rs` | Map `ModelId` → `Provider` |
| 17 | `vtcode-config/src/models/model_id/as_str.rs` | Map `ModelId` → model string constant |
| 18 | `vtcode-config/src/models/model_id/display.rs` | Human-readable display names |
| 19 | `vtcode-config/src/models/model_id/description.rs` | Model descriptions |
| 20 | `vtcode-config/src/models/model_id/parse.rs` | Parse string → `ModelId` |
| 21 | `vtcode-config/src/models/model_id/defaults.rs` | `default_orchestrator_for_provider` / `default_single_for_provider` |
| 22 | `vtcode-core/src/models_manager/model_presets.rs` | Add preset function + match arm in `presets_for_provider` |
| 23 | `vtcode-core/src/models_manager/manager.rs` | Add default model in `get_default_model_for_provider` |
| 24 | `vtcode-core/src/llm/model_resolver.rs` | Add to `provider_precedence` + `heuristic_provider_from_model` |
| 25 | `vtcode-core/src/llm/rig_adapter.rs` | Add `Provider::<Name>` arm |
| 26 | `src/startup/first_run_prompts/model.rs` | Add to `default_model_for_provider` |
| 27 | `vtcode-config/src/models/model_id/capabilities.rs` | Add to `capability_provider_key` |
| 28 | `docs/models.json` | Add provider entry with model list (auto-generates capability tables via build.rs) |

**Key insight**: The `/model` splash picker uses `ModelId::all_models()` — `builtin_model_presets()` is a separate system used by `ModelsManager`. Both must be updated for a provider/models to appear in the picker.

## Command Output

Protect context usage. **Any command with unknown or potentially large output must be byte-capped.**

Default pattern:

```bash
COMMAND 2>&1 | head -c 4000
```

<!-- codemod-skill-discovery:begin -->

## Codemod Skill Discovery

This section is managed by `codemod` CLI.

- Core skill: `.agents/skills/codemod/SKILL.md`
- Package skills: `.agents/skills/<package-skill>/SKILL.md`
- Codemod MCP: use it for JSSG authoring guidance, CLI/workflow guidance, import-helper guidance, and semantic-analysis-aware codemod work.
- List installed Codemod skills: `npx codemod ai list --harness codex --format json`

<!-- codemod-skill-discovery:end -->
