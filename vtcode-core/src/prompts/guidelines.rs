use crate::config::types::CapabilityLevel;

const TOOL_UNIFIED_EXEC: &str = "unified_exec";
const TOOL_UNIFIED_FILE: &str = "unified_file";
const TOOL_UNIFIED_SEARCH: &str = "unified_search";
const TOOL_READ_FILE: &str = "read_file";
const TOOL_LIST_FILES: &str = "list_files";
const TOOL_APPLY_PATCH: &str = "apply_patch";

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
    if has_search && has_exec {
        lines.push("- Prefer search over shell for exploration.".to_string());
    }
    if has_file || has_apply_patch || has_exec {
        lines.push(
            "- If calls repeat without progress, re-plan instead of retrying identically."
                .to_string(),
        );
    }

    if lines.is_empty() {
        return String::new();
    }

    format!("\n\n## Active Tools\n{}", lines.join("\n"))
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
    }

    #[test]
    fn test_edit_workflow_guidance() {
        let tools = vec!["unified_file".to_string()];
        let guidelines = generate_tool_guidelines(&tools, None);
        assert!(guidelines.contains("Read before edit"));
        assert!(guidelines.contains("patches small"));
    }

    #[test]
    fn test_harness_browse_tool_guidance() {
        let tools = vec!["list_files".to_string(), "read_file".to_string()];
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
            "list_files".to_string(),
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

        let tools = vec!["list_files".to_string()];
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
            "list_files".to_string(),
            "apply_patch".to_string(),
        ];
        let guidelines = generate_tool_guidelines(&tools, None);
        let approx_tokens = guidelines.len() / 4;
        assert!(approx_tokens < 110, "got ~{} tokens", approx_tokens);
    }
}
