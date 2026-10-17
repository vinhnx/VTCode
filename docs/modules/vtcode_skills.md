# vtcode-skills

Skill types, discovery, loading, and validation for VT Code.

## Overview

Provides the core skill system including skill manifests, validation, bundling, template rendering, native plugin
support, and versioning. Integration-point modules remain in `vtcode-core`.

## Key Modules

| Module                    | Purpose                                                            |
| ------------------------- | ------------------------------------------------------------------ |
| `types.rs`                | Core types: `Skill`, `SkillManifest`, `SkillContext`, `SkillScope` |
| `manifest.rs`             | SKILL.md parsing, `SkillYaml`, template generation                 |
| `authoring.rs`            | Skill authoring, frontmatter parsing, validation                   |
| `bundle.rs`               | Skill bundling, import/export, index management                    |
| `templates.rs`            | Template engine, traditional/CLI-tool templates                    |
| `container.rs`            | Skill container management, versioning                             |
| `container_validation.rs` | Container skills compatibility validation                          |
| `context_manager.rs`      | Memory-efficient skill loading with LRU eviction                   |
| `validation.rs`           | Skill validation rules and reports                                 |
| `enhanced_validator.rs`   | Comprehensive skill validator                                      |
| `native_plugin.rs`        | Native plugin loading via `libloading`                             |
| `system.rs`               | System skills embedding and installation                           |
| `versioning.rs`           | Skill version resolution and lockfiles                             |
| `injection.rs`            | Skill injection into prompts                                       |
| `prompt_integration.rs`   | Skills prompt rendering modes                                      |

## Skill Lifecycle

1. **Discovery** — Skills are discovered from filesystem locations and embedded system skills
2. **Loading** — Skills are loaded with LRU eviction for memory efficiency
3. **Validation** — Skills are validated against manifest rules and compatibility checks
4. **Injection** — Skills are injected into prompts for agent context
5. **Execution** — Skills are executed by the skill executor in vtcode-core

## Architecture Notes

- Partial extraction from vtcode-core; vtcode-core's `skills/mod.rs` re-exports everything from this crate
- `templates/` directory contains traditional and CLI-tool template files
- `src/skills/assets/samples/` contains embedded system skill samples
- `SkillManifest` uses `serde-saphyr` for YAML frontmatter parsing

## Dependencies

- `vtcode-commons` — filesystem, SHA256, paths
- `vtcode-config` — skill configuration, `PromptFormat`

## Coding Conventions

- Use `anyhow::Result` for fallible operations
- Use `serde` for serialization/deserialization
- Use `tracing` for logging
- Template paths are relative to `templates/` directory

## See Also

- [Skills Documentation](../skills/) — user-facing skill guides
- [Contributing](../CONTRIBUTING.md) — how to create custom skills
