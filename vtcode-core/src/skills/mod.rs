//! # Agent Skills Integration
//!
//! Enhanced skills system for VT Code with multi-level loading, CLI tool integration,
//! and progressive context management inspired by pi-mono's minimalist approach.
//!
//! ## Features
//!
//! - **Progressive Disclosure**: Three-level loading (metadata → instructions → resources)
//! - **CLI Tool Bridge**: Integrate any command-line tool as a skill
//! - **Dynamic Discovery**: Auto-discover skills and CLI tools from filesystem
//! - **Context Management**: Memory-efficient loading with LRU eviction
//! - **Anthropic Compatibility**: Full support for Anthropic's skill specification
//! - **Tool Integration**: Seamless integration with VTCode's tool registry
//!
//! ## Architecture
//!
//! ### Three-Level Loading System
//!
//! **Level 1: Metadata** (~50 tokens)
//! - Always loaded in system prompt
//! - Name, description, and basic info
//! - Minimal context overhead
//!
//! **Level 2: Instructions** (variable, <5K tokens typical)
//! - Loaded when skill is first triggered
//! - SKILL.md body with workflows and guidance
//! - Context-managed with automatic eviction
//!
//! **Level 3: Resources** (on-demand)
//! - Scripts, templates, reference materials
//! - Loaded only when specifically requested
//! - No context overhead when unused
//!
//! ### Skill Types
//!
//! **Traditional Skills**: Directories with SKILL.md files following Anthropic spec
//! **CLI Tool Skills**: Executable tools with README.md documentation
//! **Hybrid Skills**: Skills that combine instructions with external tool execution
//!
//! ## Quick Start
//!
//! ```ignore
//! use vtcode_core::skills::discovery::{SkillDiscovery, DiscoveryConfig};
//! use vtcode_core::skills::context_manager::{ContextManager, ContextConfig};
//!
//! // Configure discovery
//! let mut discovery = SkillDiscovery::new();
//! let result = discovery.discover_all(workspace_root).await?;
//!
//! // Setup context management
//! let context_manager = ContextManager::new();
//!
//! // Register discovered skills
//! for skill in result.skills {
//!     context_manager.register_skill_metadata(skill.manifest().clone())?;
//! }
//!
//! // Load skill on demand
//! let skill_context = context_manager.get_skill_context("my-skill");
//! ```
//!
//! ## Directory Structure
//!
//! ### Traditional Skills
//! ```text
//! my-skill/
//! ├── SKILL.md              # Metadata (YAML) + Instructions (Markdown)
//! ├── ADVANCED.md           # Optional: Advanced guide
//! ├── scripts/
//! │   └── helper.py         # Optional: Executable scripts
//! └── templates/
//!     └── example.json      # Optional: Reference materials
//! ```
//!
//! ### CLI Tool Skills
//! ```text
//! my-tool/
//! ├── tool                  # Executable (any language)
//! ├── README.md             # Tool documentation
//! ├── tool.json             # Optional: Configuration
//! └── schema.json           # Optional: Argument validation
//! ```
//!
//! ### SKILL.md Template
//! ```yaml
//! ---
//! name: my-skill
//! description: What this skill does and when to use it
//! version: 1.0.0
//! author: Your Name
//! ---
//!
//! # My Skill
//!
//! ## Instructions
//! [Guidance for Claude]
//!
//! ## Examples
//! - Example 1
//! - Example 2
//! ```

pub mod authoring;
pub mod auto_verification;
pub mod cli_bridge;
pub mod container;
pub mod container_validation;
pub mod context_manager;
pub mod discovery;
pub mod document_processor;
pub mod enhanced_harness;
pub mod enhanced_validator;
pub mod executor;
pub mod file_references;
pub mod injection;
pub mod loader;
pub mod locations;
pub mod manager;
pub mod manifest;
pub mod model;
pub mod prompt_integration;
pub mod render;
pub mod skill_file_tracker;
pub mod streaming;
pub mod system;
pub mod templates;
pub mod types;
pub mod validation;
pub mod validation_report;

pub use authoring::{
    SkillAuthor, SkillFrontmatter, ValidationReport as AuthoringValidationReport,
    render_skills_lean,
};
pub use cli_bridge::{CliToolBridge, CliToolConfig, CliToolResult, discover_cli_tools};
pub use container::{SkillContainer, SkillSpec, SkillType, SkillVersion};
pub use container_validation::{
    ContainerSkillsRequirement, ContainerSkillsValidator, ContainerValidationResult,
    IncompatibleSkillInfo,
};
pub use context_manager::{
    ContextConfig, ContextLevel, ContextManager, ContextStats, PersistentContextManager,
};
pub use discovery::{
    DiscoveryConfig, DiscoveryResult, DiscoveryStats, ProgressiveSkillLoader, SkillDiscovery,
};
pub use document_processor::{
    DocumentMetadata, DocumentProcessor, DocumentProcessorConfig, DocumentType, ProcessedDocument,
};
pub use enhanced_validator::ComprehensiveSkillValidator;
pub use executor::execute_skill_with_sub_llm;
pub use file_references::FileReferenceValidator;
pub use loader::{
    EnhancedSkill, EnhancedSkillLoader, SkillLoaderConfig, SkillRoot, detect_skill_mentions,
    discover_skill_metadata_lightweight, load_skill_resources, load_skills,
};

pub use injection::{SkillInjections, build_skill_injections};
pub use locations::{
    DiscoveredSkill, DiscoveryStats as LocationDiscoveryStats, SkillLocation, SkillLocationType,
    SkillLocations,
};
pub use manager::SkillsManager;
pub use manifest::{SkillYaml, generate_skill_template, parse_skill_content, parse_skill_file};
pub use model::{SkillErrorInfo, SkillLoadOutcome, SkillMetadata};
pub use prompt_integration::{
    SkillsRenderMode, generate_skills_prompt, generate_skills_prompt_with_mode,
};
pub use render::render_skills_section;
pub use streaming::{StreamEvent, StreamingConfig, StreamingExecution, StreamingSkillExecutor};

pub use templates::{
    SkillTemplate, SkillTemplateBuilder, TemplateEngine, TemplateType, TemplateVariable,
};
pub use types::{
    Skill, SkillContext, SkillManifest, SkillRegistryEntry, SkillResource, SkillScope,
};
pub use validation::{SkillValidator, ValidationConfig, ValidationReport, ValidationStatus};
pub use validation_report::{SkillValidationReport, ValidationIssue, ValidationLevel};
