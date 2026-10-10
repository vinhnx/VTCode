//! System instructions and prompt management.
//!
//! Prompt variants share one canonical base contract plus thin mode deltas and
//! compact runtime addenda. Project-specific behavior comes from dynamically
//! loaded instruction maps (`AGENTS.md`/`CLAUDE.md`), dynamic tool guidance,
//! skill metadata, and runtime notices.

mod cache;
mod compose;
mod constants;
mod tokens;
mod types;

pub use compose::{
    apply_coordinator_role_guidance, apply_output_style, compose_system_instruction_text,
    compose_system_instruction_with_report, generate_system_instruction_with_config,
    generate_system_instruction_with_config_and_report, generate_system_instruction_with_context_and_report,
    measure_system_prompt_size,
};
pub use constants::*;
pub use tokens::{
    estimate_token_count, generate_lightweight_instruction, generate_minimal_instruction,
    generate_specialized_instruction,
};
pub use types::{SectionKind, SystemPromptConfig, SystemPromptReport};

pub use crate::prompts::static_prompts::{
    agent_identity_label, default_lightweight_prompt, default_system_prompt, lightweight_instruction_text,
    minimal_instruction_text, minimal_system_prompt, specialized_instruction_text, specialized_system_prompt,
    static_profile_prompt,
};

#[cfg(test)]
mod tests;
