use std::collections::BTreeSet;
use std::fmt::Write as _;

use crate::config::constants::tools;
use crate::config::types::CapabilityLevel;
use crate::core::agent::harness_kernel::SessionToolCatalogSnapshot;
use crate::prompts::sections::SectionBoundaryMode;

const TOOL_UNIFIED_EXEC: &str = tools::UNIFIED_EXEC;
const TOOL_UNIFIED_FILE: &str = tools::UNIFIED_FILE;
const TOOL_UNIFIED_SEARCH: &str = tools::UNIFIED_SEARCH;
const TOOL_READ_FILE: &str = tools::READ_FILE;
const TOOL_LIST_FILES: &str = tools::LIST_FILES;
const TOOL_APPLY_PATCH: &str = tools::APPLY_PATCH;
const TOOL_REQUEST_USER_INPUT: &str = tools::REQUEST_USER_INPUT;
const TOOL_TASK_TRACKER: &str = tools::TASK_TRACKER;
const TOOL_PLAN_TASK_TRACKER: &str = tools::PLAN_TASK_TRACKER;

/// Generate compact cross-tool guidance based on the tools available in the session.
pub fn generate_tool_guidelines(
    available_tools: &[String],
    capability_level: Option<CapabilityLevel>,
) -> String {
    let has_exec = available_tools.iter().any(|tool| tool == TOOL_UNIFIED_EXEC);
    let has_file = available_tools.iter().any(|tool| tool == TOOL_UNIFIED_FILE);
    let has_search = available_tools
        .iter()
        .any(|tool| tool == TOOL_UNIFIED_SEARCH);
    let has_read_file = available_tools.iter().any(|tool| tool == TOOL_READ_FILE);
    let has_list_files = available_tools.iter().any(|tool| tool == TOOL_LIST_FILES);
    let has_apply_patch = available_tools.iter().any(|tool| tool == TOOL_APPLY_PATCH);

    let mut lines = Vec::new();
    if let Some(mode_line) = capability_mode_line(capability_level, has_exec, has_file) {
        lines.push(mode_line.to_string());
    }
    if let Some(browse_guidance) =
        browse_tool_guidance(has_search, has_file, has_list_files, has_read_file)
    {
        lines.push(browse_guidance);
    }
    if has_file || has_apply_patch {
        lines.push("- Read before edit and keep patches small.".to_string());
    }
    if has_exec {
        lines.push(
            "- Use `unified_exec` for verification, `git diff -- <path>`, and commands the public tools cannot express."
                .to_string(),
        );
    }
    if has_exec || has_file || has_apply_patch {
        lines.push(
            "- Completion is a checkpoint: keep `task_tracker` current and verification resolved."
                .to_string(),
        );
    }
    if has_search && has_exec {
        lines.push("- Prefer search over shell for exploration.".to_string());
    }
    if has_file || has_apply_patch || has_exec {
        lines.push(
            "- If calls repeat without progress, re-plan instead of retrying identically."
                .to_string(),
        );
    }
    if has_search || has_file || has_exec {
        lines.push(
            "- When calling multiple tools with no dependencies, run them in parallel (e.g., read files or run independent commands at once)."
                .to_string(),
        );
    }

    if lines.is_empty() {
        return String::new();
    }

    format!("\n\n## Active Tools\n{}", lines.join("\n"))
}

pub fn append_runtime_tool_prompt_sections(
    prompt: &mut String,
    tool_snapshot: &SessionToolCatalogSnapshot,
    include_catalog_metadata: bool,
) {
    remove_prompt_section(prompt, "## Active Tools");
    remove_prompt_section(prompt, "[Runtime Tool Catalog]");
    while prompt.ends_with('\n') {
        prompt.pop();
    }

    let available_tools = snapshot_tool_names(tool_snapshot);
    let guidelines = generate_runtime_tool_guidelines(&available_tools, tool_snapshot.plan_mode);
    if !guidelines.is_empty() {
        append_prompt_block(prompt, guidelines.trim_start_matches('\n'));
    }

    if include_catalog_metadata && tool_snapshot.snapshot.is_some() {
        let catalog_metadata = format!(
            "[Runtime Tool Catalog]\n- version: {}\n- epoch: {}\n- available_tools: {}\n- request_user_input_enabled: {}",
            tool_snapshot.version,
            tool_snapshot.epoch,
            tool_snapshot.available_tools(),
            tool_snapshot.request_user_input_enabled,
        );
        append_prompt_block(prompt, &catalog_metadata);
    }
}

fn append_prompt_block(prompt: &mut String, block: &str) {
    if block.is_empty() {
        return;
    }

    if prompt.is_empty() {
        prompt.push_str(block);
    } else {
        let _ = write!(prompt, "\n\n{block}");
    }
}

fn remove_prompt_section(prompt: &mut String, section_header: &str) {
    while let Some((section_start, section_end)) =
        find_prompt_section_bounds(prompt, section_header)
    {
        prompt.replace_range(section_start..section_end, "");
    }
}

fn find_prompt_section_bounds(prompt: &str, section_header: &str) -> Option<(usize, usize)> {
    crate::prompts::sections::find_prompt_section_bounds(
        prompt,
        section_header,
        SectionBoundaryMode::BracketOrMarkdown,
    )
}

fn generate_runtime_tool_guidelines(available_tools: &[String], plan_mode: bool) -> String {
    if !plan_mode {
        return generate_tool_guidelines(available_tools, None);
    }

    let has_exec = available_tools.iter().any(|tool| tool == TOOL_UNIFIED_EXEC);
    let has_file = available_tools.iter().any(|tool| tool == TOOL_UNIFIED_FILE);
    let has_search = available_tools
        .iter()
        .any(|tool| tool == TOOL_UNIFIED_SEARCH);
    let has_read_file = available_tools.iter().any(|tool| tool == TOOL_READ_FILE);
    let has_list_files = available_tools.iter().any(|tool| tool == TOOL_LIST_FILES);
    let has_request_user_input = available_tools
        .iter()
        .any(|tool| tool == TOOL_REQUEST_USER_INPUT);
    let has_task_tracker = available_tools
        .iter()
        .any(|tool| matches!(tool.as_str(), TOOL_TASK_TRACKER | TOOL_PLAN_TASK_TRACKER));

    let mut lines = vec![
        "- Mode: read-only. Stay within the plan-mode tool list and use only read-safe actions."
            .to_string(),
    ];
    if let Some(browse_guidance) =
        browse_tool_guidance(has_search, has_file, has_list_files, has_read_file)
    {
        lines.push(browse_guidance);
    }
    if has_file {
        lines.push("- In Plan Mode, use `unified_file` only for read-style access.".to_string());
    }
    if has_exec {
        lines.push(
            "- In Plan Mode, use `unified_exec` only for read-only verification, poll, or inspect actions."
                .to_string(),
        );
    }
    if has_task_tracker {
        lines.push("- Keep `task_tracker` updated as you refine the plan.".to_string());
        lines.push(
            "- Keep blockers and verification open in `task_tracker` until resolved.".to_string(),
        );
    }
    if has_request_user_input {
        lines.push(
            "- Use `request_user_input` only for material blockers that remain after repository exploration."
                .to_string(),
        );
    }
    if has_search || has_file || has_exec {
        lines.push(
            "- If calls repeat without progress, tighten the plan instead of retrying identically."
                .to_string(),
        );
    }

    format!("\n\n## Active Tools\n{}", lines.join("\n"))
}

fn snapshot_tool_names(tool_snapshot: &SessionToolCatalogSnapshot) -> Vec<String> {
    let Some(snapshot) = tool_snapshot.snapshot.as_ref() else {
        return Vec::new();
    };

    snapshot
        .iter()
        .map(|tool| tool.function_name().to_string())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn browse_tool_guidance(
    has_search: bool,
    has_file: bool,
    has_list_files: bool,
    has_read_file: bool,
) -> Option<String> {
    let mut tool_names = Vec::new();
    if has_search {
        tool_names.push("`unified_search`");
    } else if has_list_files {
        tool_names.push("`list_files`");
    }
    if has_file {
        tool_names.push("`unified_file`");
    } else if has_read_file {
        tool_names.push("`read_file`");
    }
    if tool_names.is_empty() {
        return None;
    }

    Some(format!(
        "- Prefer {} over shell browsing.",
        tool_names.join(" and ")
    ))
}

fn capability_mode_line(
    capability_level: Option<CapabilityLevel>,
    has_exec: bool,
    has_file: bool,
) -> Option<&'static str> {
    match capability_level {
        Some(CapabilityLevel::Basic) => Some(
            "- Mode: limited. Ask the user to enable more capabilities if file work is required.",
        ),
        Some(CapabilityLevel::FileReading | CapabilityLevel::FileListing) => Some(
            "- Mode: read-only. Analyze and search, but do not modify files or run shell commands.",
        ),
        _ if !has_exec && !has_file => Some(
            "- Mode: read-only. Analyze and search, but do not modify files or run shell commands.",
        ),
        _ => None,
    }
}

/// Infer capability level from available tools.
pub fn infer_capability_level(available_tools: &[String]) -> CapabilityLevel {
    let has_search = available_tools.iter().any(|t| t == TOOL_UNIFIED_SEARCH);
    let has_edit = available_tools.iter().any(|t| t == TOOL_UNIFIED_FILE);
    let has_read = has_edit || available_tools.iter().any(|t| t == TOOL_READ_FILE);
    let has_list = has_search || available_tools.iter().any(|t| t == TOOL_LIST_FILES);
    let has_exec = available_tools.iter().any(|t| t == TOOL_UNIFIED_EXEC);

    if has_search {
        CapabilityLevel::CodeSearch
    } else if has_edit {
        CapabilityLevel::Editing
    } else if has_exec {
        CapabilityLevel::Bash
    } else if has_list {
        CapabilityLevel::FileListing
    } else if has_read {
        CapabilityLevel::FileReading
    } else {
        CapabilityLevel::Basic
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_only_mode_detection() {
        let tools = vec!["unified_search".to_string()];
        let guidelines = generate_tool_guidelines(&tools, None);
        assert!(guidelines.contains("Mode: read-only"));
        assert!(guidelines.contains("do not modify files"));
    }

    #[test]
    fn test_tool_preference_guidance() {
        let tools = vec!["unified_exec".to_string(), "unified_search".to_string()];
        let guidelines = generate_tool_guidelines(&tools, None);
        assert!(guidelines.contains("Prefer search over shell"));
        assert!(guidelines.contains("git diff -- <path>"));
        assert!(guidelines.contains("Completion is a checkpoint"));
    }

    #[test]
    fn test_edit_workflow_guidance() {
        let tools = vec!["unified_file".to_string()];
        let guidelines = generate_tool_guidelines(&tools, None);
        assert!(guidelines.contains("Read before edit"));
        assert!(guidelines.contains("patches small"));
        assert!(guidelines.contains("verification resolved"));
    }

    #[test]
    fn test_harness_browse_tool_guidance() {
        let tools = vec![TOOL_LIST_FILES.to_string(), TOOL_READ_FILE.to_string()];
        let guidelines = generate_tool_guidelines(&tools, None);
        assert!(guidelines.contains("Prefer `list_files` and `read_file`"));
        assert!(!guidelines.contains("offset"));
        assert!(!guidelines.contains("per_page"));
    }

    #[test]
    fn test_canonical_browse_tool_guidance_prefers_public_tools() {
        let tools = vec![
            "unified_search".to_string(),
            "unified_file".to_string(),
            TOOL_LIST_FILES.to_string(),
            "read_file".to_string(),
        ];
        let guidelines = generate_tool_guidelines(&tools, None);
        assert!(guidelines.contains("Prefer `unified_search` and `unified_file`"));
        assert!(!guidelines.contains("Prefer `list_files` and `read_file`"));
    }

    #[test]
    fn test_capability_basic_guidance() {
        let tools = vec![];
        let guidelines = generate_tool_guidelines(&tools, Some(CapabilityLevel::Basic));
        assert!(guidelines.contains("Mode: limited"));
        assert!(guidelines.contains("enable more capabilities"));
    }

    #[test]
    fn test_capability_file_reading_guidance() {
        let tools = vec!["unified_file".to_string()];
        let guidelines = generate_tool_guidelines(&tools, Some(CapabilityLevel::FileReading));
        assert!(guidelines.contains("Mode: read-only"));
        assert!(guidelines.contains("do not modify"));
    }

    #[test]
    fn test_full_capabilities_no_special_guidance() {
        let tools = vec![
            "unified_file".to_string(),
            "unified_exec".to_string(),
            "unified_search".to_string(),
        ];
        let guidelines = generate_tool_guidelines(&tools, Some(CapabilityLevel::Editing));

        assert!(!guidelines.contains("Mode: limited"));
        assert!(!guidelines.contains("Mode: read-only"));
    }

    #[test]
    fn test_empty_tools_shows_read_only_mode() {
        let tools = vec![];
        let guidelines = generate_tool_guidelines(&tools, None);
        assert!(guidelines.contains("Mode: read-only"));
    }

    #[test]
    fn test_plan_mode_guidance_keeps_verification_open() {
        let tools = vec![
            TOOL_UNIFIED_EXEC.to_string(),
            TOOL_TASK_TRACKER.to_string(),
            TOOL_UNIFIED_SEARCH.to_string(),
        ];
        let guidelines = generate_runtime_tool_guidelines(&tools, true);
        assert!(guidelines.contains("Keep `task_tracker` updated"));
        assert!(guidelines.contains("blockers and verification open"));
    }

    #[test]
    fn test_capability_inference_precedence() {
        let tools = vec!["unified_file".to_string(), "unified_search".to_string()];
        assert_eq!(infer_capability_level(&tools), CapabilityLevel::CodeSearch);

        let tools = vec!["unified_exec".to_string(), "unified_file".to_string()];
        assert_eq!(infer_capability_level(&tools), CapabilityLevel::Editing);
    }

    #[test]
    fn test_capability_inference_variants() {
        let tools = vec!["unified_file".to_string()];
        assert_eq!(infer_capability_level(&tools), CapabilityLevel::Editing);

        let tools = vec!["unified_exec".to_string()];
        assert_eq!(infer_capability_level(&tools), CapabilityLevel::Bash);

        let tools = vec!["unified_search".to_string()];
        assert_eq!(infer_capability_level(&tools), CapabilityLevel::CodeSearch);

        let tools = vec![TOOL_LIST_FILES.to_string()];
        assert_eq!(infer_capability_level(&tools), CapabilityLevel::FileListing);

        let tools = vec!["read_file".to_string()];
        assert_eq!(infer_capability_level(&tools), CapabilityLevel::FileReading);

        let tools = vec!["unknown_tool".to_string()];
        assert_eq!(infer_capability_level(&tools), CapabilityLevel::Basic);
    }

    #[test]
    fn test_guidelines_stay_compact() {
        let tools = vec![
            "unified_exec".to_string(),
            "unified_search".to_string(),
            "unified_file".to_string(),
            "read_file".to_string(),
            TOOL_LIST_FILES.to_string(),
            "apply_patch".to_string(),
        ];
        let guidelines = generate_tool_guidelines(&tools, None);
        let approx_tokens = guidelines.len() / 4;
        assert!(approx_tokens < 145, "got ~{} tokens", approx_tokens);
    }

    #[test]
    fn test_parallel_tool_call_guidance() {
        let tools = vec![
            "unified_exec".to_string(),
            "unified_search".to_string(),
            "unified_file".to_string(),
        ];
        let guidelines = generate_tool_guidelines(&tools, None);
        assert!(
            guidelines.contains("parallel"),
            "Should include parallel tool call guidance"
        );
        assert!(
            guidelines.contains("read files"),
            "Should mention reading files in parallel"
        );
    }

    #[test]
    fn plan_mode_runtime_guidance_keeps_unified_file_read_only() {
        let tools = vec![
            TOOL_UNIFIED_FILE.to_string(),
            TOOL_UNIFIED_EXEC.to_string(),
            TOOL_UNIFIED_SEARCH.to_string(),
        ];
        let guidelines = generate_runtime_tool_guidelines(&tools, true);

        assert!(guidelines.contains("Mode: read-only"));
        assert!(guidelines.contains("`unified_file` only for read-style access"));
        assert!(guidelines.contains("`unified_exec` only for read-only verification"));
        assert!(!guidelines.contains("Read before edit"));
    }

    #[test]
    fn runtime_tool_prompt_sections_include_catalog_metadata() {
        let mut prompt = "Base prompt".to_string();
        let snapshot = SessionToolCatalogSnapshot::new(
            7,
            9,
            true,
            false,
            Some(std::sync::Arc::new(vec![
                crate::llm::provider::ToolDefinition::function(
                    TOOL_UNIFIED_SEARCH.to_string(),
                    "Search".to_string(),
                    serde_json::json!({"type": "object"}),
                ),
                crate::llm::provider::ToolDefinition::function(
                    TOOL_UNIFIED_FILE.to_string(),
                    "File".to_string(),
                    serde_json::json!({"type": "object"}),
                ),
            ])),
            false,
        );

        append_runtime_tool_prompt_sections(&mut prompt, &snapshot, true);

        assert!(prompt.contains("## Active Tools"));
        assert!(prompt.contains("[Runtime Tool Catalog]"));
        assert!(prompt.contains("request_user_input_enabled: false"));
    }

    #[test]
    fn runtime_tool_prompt_sections_replace_existing_runtime_sections() {
        let mut prompt = "Base prompt".to_string();
        let first = SessionToolCatalogSnapshot::new(
            1,
            2,
            false,
            false,
            Some(std::sync::Arc::new(vec![
                crate::llm::provider::ToolDefinition::function(
                    TOOL_UNIFIED_SEARCH.to_string(),
                    "Search".to_string(),
                    serde_json::json!({"type": "object"}),
                ),
            ])),
            false,
        );
        let second = SessionToolCatalogSnapshot::new(
            7,
            9,
            true,
            true,
            Some(std::sync::Arc::new(vec![
                crate::llm::provider::ToolDefinition::function(
                    TOOL_UNIFIED_FILE.to_string(),
                    "File".to_string(),
                    serde_json::json!({"type": "object"}),
                ),
            ])),
            false,
        );

        append_runtime_tool_prompt_sections(&mut prompt, &first, true);
        append_runtime_tool_prompt_sections(&mut prompt, &second, true);

        assert_eq!(prompt.matches("## Active Tools").count(), 1);
        assert_eq!(prompt.matches("[Runtime Tool Catalog]").count(), 1);
        assert!(prompt.contains("version: 7"));
        assert!(!prompt.contains("version: 1"));
        assert!(prompt.contains("request_user_input_enabled: true"));
        assert!(!prompt.contains("request_user_input_enabled: false"));
    }
}
