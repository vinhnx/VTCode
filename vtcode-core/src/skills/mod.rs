//! # Agent Skills Integration
//!
//! Integration of Anthropic Agent Skills into VTCode's agent harness.
//!
//! Skills are modular capabilities that extend Claude's functionality through:
//! - Skill discovery from filesystem
//! - Progressive disclosure (metadata → instructions → resources)
//! - Execution as first-class tools in the agent harness
//! - Full integration with VTCode's permission, caching, and audit systems
//!
//! ## Quick Start
//!
//! ```ignore
//! use vtcode_core::skills::loader::SkillLoader;
//!
//! // Discover available skills
//! let mut loader = SkillLoader::new(workspace_root);
//! let skills = loader.discover_skills()?;
//!
//! // Load a skill
//! let skill = loader.load_skill("my-skill")?;
//!
//! // Use skill as a tool
//! let adapter = SkillToolAdapter::new(skill);
//! tool_registry.register(Box::new(adapter)).await?;
//! ```
//!
//! ## Architecture
//!
//! Skills follow Anthropic's specification with three levels of loading:
//!
//! **Level 1: Metadata** (~100 tokens)
//! - Always loaded in system prompt
//! - Name and description only
//! - Tells agent which skills are available
//!
//! **Level 2: Instructions** (<5K tokens)
//! - Loaded when skill is triggered
//! - SKILL.md body with workflows and guidance
//! - Consumed only when skill is actually used
//!
//! **Level 3: Resources** (on-demand)
//! - Scripts, templates, reference materials
//! - Executed via bash without loading contents
//! - No context overhead when unused
//!
//! ## Files
//!
//! Skills are directories with SKILL.md file:
//!
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
//! SKILL.md template:
//!
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

pub mod container;
pub mod executor;
pub mod loader;
pub mod manifest;
pub mod types;

pub use container::{SkillContainer, SkillSpec, SkillType, SkillVersion};
pub use executor::{SkillExecutionContext, SkillToolAdapter, execute_skill_with_sub_llm};
pub use loader::{SkillCache, SkillLoader};
pub use manifest::{generate_skill_template, parse_skill_content, parse_skill_file, SkillYaml};
pub use types::{Skill, SkillContext, SkillManifest, SkillRegistryEntry, SkillResource};
