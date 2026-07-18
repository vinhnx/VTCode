//! # Agent Skills Integration
//!
//! Enhanced skills system for VT Code with progressive loading, filesystem discovery,
//! and strict `SKILL.md` validation.
//!
//! The bulk of the skill types, validation, bundling, and template logic lives
//! in the `vtcode-skills` crate. This module re-exports everything from there
//! and keeps the integration-point files that depend on `crate::core`, `crate::llm`,
//! or `crate::tools` as local sub-modules.

// Re-export everything from vtcode-skills for backward compatibility.
pub use vtcode_skills::*;

// Explicit re-exports from vtcode-skills sub-modules (glob doesn't flatten nested items)
pub use vtcode_skills::authoring::{
    SkillAuthor, SkillFrontmatter, ValidationReport as AuthoringValidationReport, render_skills_lean,
};
pub use vtcode_skills::bundle::{
    ImportedSkillInfo, SkillStoreIndex, SkillVersionIndex, export_skill_bundle, import_inline_bundle,
    import_skill_bundle, load_skill_index,
};
pub use vtcode_skills::command_skills::{
    BuiltInCommandExecutor, BuiltInCommandSkill, CommandSkillBackend, CommandSkillSpec, built_in_command_skill,
    built_in_command_skill_contexts, command_skill_specs, find_command_skill_by_skill_name,
    find_command_skill_by_slash_name,
};
pub use vtcode_skills::container::{
    SkillContainer, SkillSource as ContainerSkillSource, SkillSpec, SkillType, SkillVersion,
};
pub use vtcode_skills::container_validation::{
    ContainerSkillsRequirement, ContainerSkillsValidator, ContainerValidationResult, IncompatibleSkillInfo,
};
pub use vtcode_skills::context_manager::{
    ContextConfig, ContextLevel, ContextManager, ContextStats, PersistentContextManager,
};
pub use vtcode_skills::document_processor::{
    DocumentMetadata, DocumentProcessor, DocumentProcessorConfig, DocumentType, ProcessedDocument,
};
pub use vtcode_skills::enhanced_validator::ComprehensiveSkillValidator;
pub use vtcode_skills::file_references::FileReferenceValidator;
pub use vtcode_skills::injection::{SkillInjections, build_skill_injections};
pub use vtcode_skills::instructions::{SKILL_INSTRUCTIONS_PREFIX, SkillInstructions};
pub use vtcode_skills::locations::{
    DiscoveredSkill, DiscoveryStats as LocationDiscoveryStats, SkillLocation, SkillLocationType, SkillLocations,
};
pub use vtcode_skills::manifest::{SkillYaml, generate_skill_template, parse_skill_content, parse_skill_file};
pub use vtcode_skills::model::{SkillErrorInfo, SkillLoadOutcome, SkillMetadata};
pub use vtcode_skills::native_plugin::{
    NativePlugin, NativePluginTrait, PLUGIN_ABI_VERSION, PluginContext, PluginLoader, PluginMetadata, PluginResult,
    validate_plugin_structure,
};
pub use vtcode_skills::prompt_integration::{
    SkillsRenderMode, generate_skills_prompt, generate_skills_prompt_with_mode,
};
pub use vtcode_skills::render::render_skills_section;
pub use vtcode_skills::templates::{
    SkillTemplate, SkillTemplateBuilder, TemplateEngine, TemplateType, TemplateVariable,
};
pub use vtcode_skills::types::{
    Skill, SkillContext, SkillManifest, SkillNetworkPolicy, SkillRegistryEntry, SkillResource, SkillScope,
};
pub use vtcode_skills::validation_report::{SkillValidationReport, ValidationIssue, ValidationLevel};
pub use vtcode_skills::versioning::{
    ResolvedSkillRef, SkillLockfile, SkillSource, resolve_default_version, resolve_version,
};

// ---------------------------------------------------------------------------
// Staying sub-modules (depend on vtcode-core internals)
// ---------------------------------------------------------------------------
pub mod auto_verification;
pub mod cli_bridge;
pub mod discovery;
pub mod enhanced_harness;
pub mod executor;
pub mod loader;
pub mod manager;
pub mod skill_file_tracker;
pub mod streaming;
pub mod validation;

// Re-export stayed-module public API
pub use cli_bridge::{CliToolBridge, CliToolConfig, CliToolResult, discover_cli_tools};
pub use discovery::{DiscoveryConfig, DiscoveryResult, DiscoveryStats, ProgressiveSkillLoader, SkillDiscovery};
pub use executor::{execute_skill_with_sub_llm, filter_tools_for_skill};
pub use loader::{
    EnhancedSkill, EnhancedSkillLoader, SkillLoaderConfig, SkillRoot, detect_skill_mentions,
    discover_skill_metadata_lightweight, load_skill_resources, load_skills,
};
pub use manager::SkillsManager;
pub use streaming::{StreamEvent, StreamingConfig, StreamingExecution, StreamingSkillExecutor};
pub use validation::{SkillValidator, ValidationConfig, ValidationReport, ValidationStatus};
