//! System prompt generation with modular architecture
//!
//! This module provides flexible system prompt generation with
//! template-based composition and context-aware customization.

pub mod config;
pub mod context;
pub mod custom;
pub mod generator;
pub mod system;
pub mod templates;

// Re-export main types for backward compatibility
pub use config::SystemPromptConfig;
pub use context::PromptContext;
pub use custom::{CustomPrompt, CustomPromptRegistry, PromptInvocation};
pub use generator::{SystemPromptGenerator, generate_system_instruction_with_config};
pub use system::{
    generate_lightweight_instruction, generate_specialized_instruction,
    generate_system_instruction, read_system_prompt_from_md,
};
pub use templates::PromptTemplates;
