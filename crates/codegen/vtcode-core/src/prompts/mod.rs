//! System prompt generation with modular architecture
//!
//! This module provides flexible system prompt generation with
//! template-based composition and context-aware customization.

pub mod cache_aware;
pub mod config;
pub mod context;
pub mod few_shot;
pub mod guidelines;
pub mod harness_limits;
pub mod output_styles;
pub mod resources;
pub mod runtime_contract;
pub mod sections;
pub mod system;
pub mod system_prompt_cache;
pub mod templates;
pub mod temporal;

// Re-export main types for backward compatibility
pub use cache_aware::sort_tool_definitions;
pub use config::SystemPromptConfig;
pub use context::PromptContext;
pub use few_shot::{
    DEFAULT_FEW_SHOT_BUDGET_TOKENS, FewShotExample, FewShotStore, render_few_shot_section,
};
pub use guidelines::{
    append_deferred_tools_prompt_section, append_runtime_tool_prompt_sections,
    append_runtime_tool_prompt_sections_for_profile, generate_tool_guidelines,
    infer_capability_level,
};
pub use harness_limits::upsert_harness_limits_section;
pub use resources::{
    PromptTemplate, apply_system_prompt_layers, discover_prompt_templates, expand_prompt_template,
    find_prompt_template, resolve_system_prompt_layers,
};
pub use runtime_contract::{RuntimePromptContract, append_runtime_mode_sections};
pub use system::SystemPromptReport;
pub use system::{
    apply_output_style, generate_lightweight_instruction, generate_specialized_instruction,
    generate_system_instruction, measure_system_prompt_size,
};
pub use system_prompt_cache::{PROMPT_CACHE, PromptProvider, SystemPromptCache, TaskType};
pub use templates::PromptTemplates;
pub use temporal::generate_temporal_context;
