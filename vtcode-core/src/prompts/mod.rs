//! System prompt generation with modular architecture
//!
//! This module provides flexible system prompt generation with
//! template-based composition and context-aware customization.

pub mod config;
pub mod context;
pub mod custom;
pub mod generator;
pub mod guidelines;
pub mod system;
pub mod system_prompt_cache;
pub mod templates;
pub mod temporal;

// Re-export main types for backward compatibility
pub use config::SystemPromptConfig;
pub use context::PromptContext;
pub use custom::{BuiltinDocs, CustomPrompt, CustomPromptRegistry, PromptInvocation};
pub use generator::{SystemPromptGenerator, generate_system_instruction_with_config};
pub use guidelines::{generate_tool_guidelines, infer_capability_level};
pub use system::{
    generate_lightweight_instruction, generate_specialized_instruction, generate_system_instruction,
};
pub use system_prompt_cache::{PROMPT_CACHE, PromptProvider, SystemPromptCache, TaskType};
pub use templates::PromptTemplates;
pub use temporal::generate_temporal_context;
