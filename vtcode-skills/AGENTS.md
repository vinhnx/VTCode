# vtcode-skills

Skill types, discovery, loading, and validation for VT Code.

## Overview

Provides the core skill system including skill manifests, validation, bundling, template rendering, native plugin support, and versioning. Integration-point modules (executor, loader, discovery, etc.) remain in vtcode-core.

## Key Modules

| Module | Purpose |
|--------|---------|
| `types.rs` | Core types: `Skill`, `SkillManifest`, `SkillContext`, `SkillScope` |
| `manifest.rs` | SKILL.md parsing, `SkillYaml`, template generation |
| `authoring.rs` | Skill authoring, frontmatter parsing, validation |
| `bundle.rs` | Skill bundling, import/export, index management |
| `templates.rs` | Template engine, traditional/CLI-tool templates |
| `container.rs` | Skill container management, versioning |
| `container_validation.rs` | Container skills compatibility validation |
| `context_manager.rs` | Memory-efficient skill loading with LRU eviction |
| `validation.rs` | Skill validation rules and reports |
| `validation_report.rs` | Structured validation output |
| `enhanced_validator.rs` | Comprehensive skill validator |
| `native_plugin.rs` | Native plugin loading via `libloading` |
| `system.rs` | System skills embedding and installation |
| `document_processor.rs` | Skill document parsing |
| `file_references.rs` | File reference validation in skills |
| `locations.rs` | Skill location discovery |
| `model.rs` | Skill metadata and scope model |
| `command_skills.rs` | Built-in command skill definitions |
| `injection.rs` | Skill injection into prompts |
| `instructions.rs` | Skill instruction types |
| `prompt_integration.rs` | Skills prompt rendering modes |
| `render.rs` | Skills section rendering |
| `versioning.rs` | Skill version resolution and lockfiles |

## Architecture Notes

- This is a **partial extraction** from vtcode-core. Integration-point files remain in vtcode-core.
- vtcode-core's `skills/mod.rs` re-exports everything from this crate plus keeps local sub-modules.
- `templates/` directory contains traditional and CLI-tool template files.
- `src/skills/assets/samples/` contains embedded system skill samples.

## Dependencies

- `vtcode-commons` (filesystem, SHA256, paths)
- `vtcode-config` (skill configuration, `PromptFormat`)

## Coding Conventions

- Use `anyhow::Result` for fallible operations
- Use `serde` for serialization/deserialization
- Use `tracing` for logging
- Skill manifest parsing uses `serde-saphyr` for YAML frontmatter
- Template paths are relative to `templates/` directory
