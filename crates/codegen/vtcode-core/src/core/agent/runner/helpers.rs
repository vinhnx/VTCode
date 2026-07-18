use std::path::Path;

use crate::config::VTCodeConfig;
use crate::project_doc::build_instruction_appendix;
use crate::prompts::PromptContext;
use crate::prompts::system::{SystemPromptReport, compose_system_instruction_with_report};
use anyhow::Result;
use serde_json::Value;

pub(super) fn detect_textual_exec_tool_call(text: &str) -> Option<Value> {
    const FENCE_PREFIXES: [&str; 4] = [
        "```tool:command_session",
        "```command_session",
        "```tool:run_pty_cmd",
        "```run_pty_cmd",
    ];

    let (start_idx, prefix) = FENCE_PREFIXES
        .iter()
        .filter_map(|candidate| text.find(candidate).map(|idx| (idx, *candidate)))
        .min_by_key(|(idx, _)| *idx)?;

    // Require a fenced block owned by the model to avoid executing echoed examples.
    let mut remainder = &text[start_idx + prefix.len()..];
    if remainder.starts_with('\r') {
        remainder = &remainder[1..];
    }
    remainder = remainder.strip_prefix('\n')?;

    let fence_close = remainder.find("```")?;
    let block = remainder[..fence_close].trim();
    if block.is_empty() {
        return None;
    }

    let parsed = serde_json::from_str::<Value>(block)
        .or_else(|_| json5::from_str::<Value>(block))
        .ok()?;
    let payload = parsed.as_object()?;
    if payload
        .get("action")
        .and_then(Value::as_str)
        .is_some_and(|action| action != "run")
    {
        return None;
    }

    let command = crate::tools::command_args::command_text(&parsed).ok().flatten()?;
    let mut public_args = serde_json::Map::new();
    public_args.insert("cmd".to_string(), Value::String(command));
    for key in [
        "yield_time_ms",
        "max_output_tokens",
        "workdir",
        "tty",
        "sandbox_permissions",
        "additional_permissions",
        "justification",
    ] {
        if let Some(value) = payload.get(key) {
            public_args.insert(key.to_string(), value.clone());
        }
    }
    Some(Value::Object(public_args))
}

/// Compose the system prompt for a workspace using the configured prompt
/// context, then optionally append an instruction appendix derived from
/// `AGENTS.md`, `CLAUDE.md`, and `.vtcode/rules/` files.
///
/// The persistent-memory section of the appendix is disabled when
/// `config.memories_enabled()` returns `false` so the resulting prompt stays
/// consistent with the configured memory policy.
///
/// Returns the token-budget [`SystemPromptReport`] for the composed section
/// text (measured before the appendix is appended, since the appendix is not
/// one of the trimmable layers `compose_system_instruction_with_report`
/// manages).
pub(super) async fn compose_system_prompt_with_appendix(
    workspace: &Path,
    config: &VTCodeConfig,
    prompt_context: &PromptContext,
) -> Result<(String, SystemPromptReport)> {
    let (mut system_prompt, report) =
        compose_system_instruction_with_report(workspace, Some(config), Some(prompt_context)).await;

    let mut appendix_config = config.agent.clone();
    if !config.memories_enabled() {
        appendix_config.persistent_memory.enabled = false;
    }
    if let Some(appendix) = build_instruction_appendix(&appendix_config, workspace).await {
        system_prompt.push_str("\n\n# INSTRUCTIONS\n");
        system_prompt.push_str(&appendix);
    }

    Ok((system_prompt, report))
}

#[cfg(test)]
mod tests {
    use super::detect_textual_exec_tool_call;
    use serde_json::json;

    #[test]
    fn detects_command_session_fence() {
        let text = "```tool:command_session\n{\"command\":\"pwd\"}\n```";
        assert_eq!(detect_textual_exec_tool_call(text), Some(json!({"cmd":"pwd"})));
    }

    #[test]
    fn detects_legacy_run_pty_cmd_fence() {
        let text = "```run_pty_cmd\n{\"command\":\"pwd\"}\n```";
        assert_eq!(detect_textual_exec_tool_call(text), Some(json!({"cmd":"pwd"})));
    }

    #[test]
    fn converts_command_arrays_to_public_exec_command_shape() {
        let text = "```command_session\n{\"action\":\"run\",\"command\":[\"printf\",\"hello world\"]}\n```";
        assert_eq!(detect_textual_exec_tool_call(text), Some(json!({"cmd":"printf 'hello world'"})));
    }

    #[test]
    fn ignores_non_object_payloads() {
        let text = "```command_session\n[\"pwd\"]\n```";
        assert!(detect_textual_exec_tool_call(text).is_none());
    }
}
