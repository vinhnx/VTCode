pub(super) use super::cache::*;
pub(super) use super::compose::*;
pub(super) use super::types::*;
pub(super) use super::*;
pub(super) use crate::config::VTCodeConfig;
pub(super) use crate::config::constants::tools;
pub(super) use crate::config::types::ShellPromptProfile;
pub(super) use crate::config::types::{ResolvedShellPromptProfile, SystemPromptMode};
pub(super) use crate::prompts::context::PromptContext;
pub(super) use crate::prompts::guidelines::generate_tool_guidelines_for_profile;
pub(super) use std::path::PathBuf;
pub(super) use vtcode_commons::estimate_tokens;

const REMOVED_MODEL_FACING_TOOL_NAMES: &[&str] = &[
    "command_session",
    "file_operation",
    "search_dispatch",
    "list_files",
    "read_file",
    "write_file",
    "edit_file",
    "grep_file",
];

fn assert_no_removed_model_facing_tool_names(prompt: &str) {
    for tool_name in REMOVED_MODEL_FACING_TOOL_NAMES {
        assert!(!prompt.contains(tool_name), "prompt should not mention removed tool name {tool_name}");
    }
}

mod budget_cache_measure;
mod content_and_dynamic;
mod context_and_identity;
mod prompt_shape;
