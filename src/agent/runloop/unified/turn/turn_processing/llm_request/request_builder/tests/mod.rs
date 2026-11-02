pub(super) use std::sync::Arc;

pub(super) use serde_json::json;
pub(super) use vtcode_config::core::permissions::{AgentPermissionsConfig, PermissionDefault};
pub(super) use vtcode_config::{SubagentMemoryScope, SubagentSource, SubagentSpec};
pub(super) use vtcode_core::config::loader::VTCodeConfig;
pub(super) use vtcode_core::config::types::ReasoningEffortLevel;
pub(super) use vtcode_core::config::{ToolDisplayMode, ToolOutputMode};
pub(super) use vtcode_core::llm::provider::{self as uni, ToolDefinition};

pub(super) use super::super::snapshot::capture_turn_request_snapshot;
pub(super) use super::{build_turn_request, stable_system_prefix_hash};
pub(super) use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;

fn test_primary_agent_spec(name: &str, prompt: &str) -> SubagentSpec {
    SubagentSpec {
        name: name.to_string(),
        description: format!("{name} description"),
        prompt: prompt.to_string(),
        tools: Some(vec!["code_search".to_string()]),
        disallowed_tools: vec!["shell".to_string()],
        model: None,
        color: None,
        reasoning_effort: None,
        permissions: AgentPermissionsConfig::new(PermissionDefault::Deny),
        skills: Vec::new(),
        mcp_servers: Vec::new(),
        hooks: None,
        background: false,
        mode: vtcode_config::AgentMode::Primary,
        max_turns: None,
        nickname_candidates: Vec::new(),
        initial_prompt: None,
        memory: None,
        isolation: None,
        aliases: Vec::new(),
        source: SubagentSource::ProjectVtcode,
        file_path: None,
        warnings: Vec::new(),
        tool_policy_overrides: std::collections::BTreeMap::new(),
    }
}

fn named_tool(name: &str) -> ToolDefinition {
    ToolDefinition::function(
        name.to_string(),
        format!("{name} tool"),
        json!({
            "type": "object",
            "properties": {
                "input": { "type": "string" }
            }
        }),
    )
}

fn request_tool_names(request: &uni::LLMRequest) -> Vec<String> {
    request
        .tools
        .as_deref()
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .map(|tool| tool.function_name().to_string())
        .collect()
}

fn non_runtime_request_messages(request: &uni::LLMRequest) -> Vec<uni::Message> {
    request.messages.as_ref().clone()
}

fn system_prompt_text(request: &uni::LLMRequest) -> &str {
    request.system_prompt.as_ref().expect("system prompt").as_ref()
}

fn write_patch_edit_few_shot_example(workspace: &std::path::Path) {
    let examples_dir = workspace.join(".vtcode/prompts/examples");
    std::fs::create_dir_all(&examples_dir).expect("examples dir");
    std::fs::write(
        examples_dir.join("patch-edit.md"),
        "---\nid: patch-edit\ntags: [patch, edit]\nsummary: Use apply_patch for edits.\n---\n# User\nedit the file\n\n# Assistant\nRead it, then apply_patch.\n",
    )
    .expect("few-shot example");
}

fn is_few_shot_block(message: &uni::Message) -> bool {
    message
        .content
        .as_text()
        .starts_with(vtcode_core::prompts::FEW_SHOT_SECTION_HEADER)
}

fn exec_round(ctx: &mut crate::agent::runloop::unified::turn::context::TurnProcessingContext<'_>, id: &str) {
    ctx.working_history.push(uni::Message::assistant_with_tools(
        String::new(),
        vec![uni::ToolCall::function(
            id.to_string(),
            "exec_command".to_string(),
            "{}".to_string(),
        )],
    ));
    ctx.working_history
        .push(uni::Message::tool_response(id.to_string(), "exit 0".to_string()));
}

fn collapsed_notice_count(messages: &[uni::Message]) -> usize {
    messages
        .iter()
        .filter(|message| {
            message.role == uni::MessageRole::System
                && message.content.as_text().as_ref() == super::COLLAPSED_TOOL_OUTPUT_NOTICE
        })
        .count()
}

mod clearing_compaction;
mod few_shot_collapsed;
mod history_chain;
mod preflight_prefix;
mod primary_agent_state;
mod primary_agent_tools;
mod recovery_deferral;
