# vtcode-skills

Skill types, discovery, loading, and validation for VT Code.

Provides the core skill system including skill manifests, validation, bundling,
template rendering, and native plugin support.

<!-- cargo-rdme start -->

### vtcode-skills - Skill Types, Discovery, and Validation

Provides the core skill system for VT Code including skill manifests,
validation, bundling, template rendering, and native plugin support.

<!-- cargo-rdme end -->

## Modules

| Module | Purpose |
|---|---|
| `authoring` | Skill authoring and creation tools |
| `bundle` | Skill packaging and bundling |
| `command_skills` | Built-in command skill definitions |
| `container` | Skill container management |
| `container_validation` | Container validation rules |
| `context_manager` | Skill context and state management |
| `document_processor` | Document processing for skill content |
| `enhanced_validator` | Enhanced validation with custom rules |
| `file_references` | File reference resolution |
| `injection` | Skill injection into prompts |
| `instructions` | Instruction generation from skills |
| `locations` | Skill location and path management |
| `manifest` | Skill manifest parsing and types |
| `model` | Skill model types |
| `native_plugin` | Native plugin loading via dynamic libraries |
| `prompt_integration` | Prompt integration for skills |
| `render` | Skill content rendering |
| `system` | System-level skill management |
| `templates` | Template rendering engine |
| `types` | Core skill types |
| `validation_report` | Validation report generation |
| `versioning` | Skill versioning support |

## Features

- **Manifest-based**: YAML/JSON skill manifests with validation
- **Bundling**: Package skills for distribution
- **Native plugins**: Load skills from compiled dynamic libraries
- **Template rendering**: Render skill content with variable substitution
- **Validation**: Comprehensive skill validation with detailed reports

## Dependencies

- `vtcode-commons` (shared primitives, error types)
- `vtcode-config` (configuration loading)
