<!-- Compact maintainer rules retain the repository instruction line budget. -->
<!-- markdownlint-disable MD013 -->
# vtcode-skills

[Root AGENTS.md](../../../AGENTS.md) | Skill types, discovery, loading, validation. Partial extraction from vtcode-core.

## Key Modules

`types.rs` + `manifest.rs` core types | `authoring.rs` | `bundle.rs` | `templates.rs` | `container.rs` + `container_validation.rs` | `context_manager.rs` | `validation_report.rs` + `enhanced_validator.rs` | `trust.rs` prompt-boundary helpers | `native_plugin.rs` | `system.rs` | `document_processor.rs` | `file_references.rs` | `locations.rs` | `model.rs` | `command_skills.rs` (including `/webmcp`) | `injection.rs` + `instructions.rs` + `prompt_integration.rs` + `render.rs` | `versioning.rs`

## Architecture Notes

- **Partial extraction** — vtcode-core's `skills/mod.rs` re-exports this crate plus keeps local sub-modules. `templates/` contains template files; `src/skills/assets/samples/` has embedded samples.

## Dependencies

`vtcode-commons` (fs, SHA256) | `vtcode-config` (skills, `PromptFormat`)

## Coding Conventions

`anyhow::Result`, `serde`, `tracing`. Manifest parsing uses `serde-saphyr` for YAML frontmatter. Reference extraction excludes Markdown delimiters and external URLs, deduplicates normalized paths, and sorts diagnostics. Template paths relative to `templates/`. Static regex literals live in `LazyLock`/`OnceLock` statics (CI-enforced by the `no-per-call-regex-literal` ast-grep rule); `clippy::expect_used` is crate-allowed for those infallible compile-time-constant initializers, and runtime patterns must be memoized, not recompiled per call.

## Gotchas

- `native_plugin.rs` loads process-level code through `dlopen`; callers must establish provenance and explicit approval, and must never trust repository plugin roots. Skill instructions are bounded untrusted resource content, not policy. Registry capability metadata remains authoritative, and out-of-workspace permissions require an explicit human approval token.
- `trust.rs` `render_untrusted_skill_instructions_with_context` adds `Skill directory:` + capped `<skill_resources>`; use it for activation so relative refs resolve without bulk-loading. `prompt_integration.rs` `Full` adds `[compat:]`/`[tools:]`; `Lean` stays default catalog.
