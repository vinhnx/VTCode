# vtcode-skills

[Root AGENTS.md](../AGENTS.md) | Skill types, discovery, loading, validation. Partial extraction from vtcode-core.

## Key Modules

`types.rs` + `manifest.rs` core types | `authoring.rs` | `bundle.rs` | `templates.rs` | `container.rs` + `container_validation.rs` | `context_manager.rs` | `validation.rs` + `validation_report.rs` + `enhanced_validator.rs` | `native_plugin.rs` | `system.rs` | `document_processor.rs` | `file_references.rs` | `locations.rs` | `model.rs` | `command_skills.rs` | `injection.rs` + `instructions.rs` + `prompt_integration.rs` + `render.rs` | `versioning.rs`

## Architecture Notes

- **Partial extraction** — vtcode-core's `skills/mod.rs` re-exports this crate plus keeps local sub-modules.
- `templates/` contains template files; `src/skills/assets/samples/` has embedded samples.

## Dependencies

`vtcode-commons` (fs, SHA256) | `vtcode-config` (skills, `PromptFormat`)

## Coding Conventions

`anyhow::Result`, `serde`, `tracing`. Manifest parsing uses `serde-saphyr` for YAML frontmatter. Template paths relative to `templates/`.
