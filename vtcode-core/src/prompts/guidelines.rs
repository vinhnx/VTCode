use crate::config::types::CapabilityLevel;

// Tool name constants - single source of truth for tool identifiers
const TOOL_UNIFIED_EXEC: &str = "unified_exec";
const TOOL_UNIFIED_FILE: &str = "unified_file";
const TOOL_UNIFIED_SEARCH: &str = "unified_search";
const TOOL_READ_FILE: &str = "read_file";
const TOOL_LIST_FILES: &str = "list_files";
const TOOL_APPLY_PATCH: &str = "apply_patch";

/// Generate dynamic guidelines based on available tools and capability level
///
/// This function analyzes the available tools and generates context-specific
/// guidance to help the LLM make optimal tool choices and understand its
/// operational constraints.
///
/// # Arguments
/// * `available_tools` - List of tool names currently available
/// * `capability_level` - Optional capability level (can be inferred if None)
///
/// # Returns
/// A formatted string containing relevant guidelines, or empty string if no special guidance needed
///
/// # Examples
/// ```
/// use vtcode_core::prompts::guidelines::generate_tool_guidelines;
/// use vtcode_core::config::types::CapabilityLevel;
///
/// let tools = vec!["unified_search".to_string()];
/// let guidelines = generate_tool_guidelines(&tools, None);
/// assert!(guidelines.contains("Mode: read-only"));
/// ```
pub fn generate_tool_guidelines(
    available_tools: &[String],
    capability_level: Option<CapabilityLevel>,
) -> String {
    let mut active_tools = available_tools
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    active_tools.sort_unstable();
    active_tools.dedup();

    let has_exec = active_tools.contains(&TOOL_UNIFIED_EXEC);
    let has_file = active_tools.contains(&TOOL_UNIFIED_FILE);
    let has_search = active_tools.contains(&TOOL_UNIFIED_SEARCH);
    let has_read_file = active_tools.contains(&TOOL_READ_FILE);
    let has_list_files = active_tools.contains(&TOOL_LIST_FILES);
    let has_apply_patch = active_tools.contains(&TOOL_APPLY_PATCH);

    let mut lines = Vec::new();
    if let Some(mode_line) = capability_mode_line(capability_level, has_exec, has_file) {
        lines.push(mode_line.to_string());
    }

    for tool in active_tools {
        lines.push(render_tool_line(tool));
    }

    if has_search && has_exec {
        lines.push(
            "- Rule: Prefer `unified_search` over `unified_exec` for exploration.".to_string(),
        );
    }
    if has_list_files {
        lines.push(
            "- Rule: Prefer `list_files` for directory discovery; use `page` and `per_page` to continue instead of shell listing.".to_string(),
        );
    }
    if has_read_file {
        lines.push(
            "- Rule: Prefer `read_file` for file contents; use `offset` and `limit` to continue large reads in chunks.".to_string(),
        );
    }
    if has_exec {
        lines.push(
            "- Rule: For diff requests, run `git diff -- <path>` via `unified_exec`; do not fabricate diffs from file reads.".to_string(),
        );
    }
    if has_file || has_apply_patch {
        lines.push(
            "- Rule: If calls repeat without progress, re-plan into smaller verified slices instead of retrying identically.".to_string(),
        );
    }

    if lines.is_empty() {
        return String::new();
    }

    format!("\n\n## Active Tools\n{}", lines.join("\n"))
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

fn render_tool_line(tool: &str) -> String {
    match tool {
        TOOL_UNIFIED_SEARCH => "- `unified_search`: code/text search. Prefer `action='structural'` for code, set `lang` when known, and use `action='grep'` for plain text.".to_string(),
        TOOL_UNIFIED_FILE => "- `unified_file`: file reads and edits. Read before `edit`; use `write` for new files.".to_string(),
        TOOL_READ_FILE => "- `read_file`: chunked file reads. Use `offset` and `limit` to continue large files without re-reading everything.".to_string(),
        TOOL_LIST_FILES => "- `list_files`: paginated file discovery. Use `page` and `per_page` for traversal; keep `unified_search` for grep/structural search.".to_string(),
        TOOL_UNIFIED_EXEC => "- `unified_exec`: shell commands and verification. prefer `rg` over shell `grep`; use it for `git diff` and checks.".to_string(),
        TOOL_APPLY_PATCH => "- `apply_patch`: surgical patches. Keep anchors stable and patches small.".to_string(),
        _ => format!("- `{tool}`: available in this session."),
    }
}

/// Infer capability level from available tools
///
/// This function analyzes the tool list and determines the most appropriate
/// capability level. The capability level represents the agent's permission scope.
///
/// # Arguments
/// * `available_tools` - List of tool names currently available
///
/// # Returns
/// The inferred capability level based on tool availability
///
/// # Examples
/// ```
/// use vtcode_core::prompts::guidelines::infer_capability_level;
/// use vtcode_core::config::types::CapabilityLevel;
///
/// let tools = vec!["unified_file".to_string()];
/// assert_eq!(infer_capability_level(&tools), CapabilityLevel::Editing);
/// ```
pub fn infer_capability_level(available_tools: &[String]) -> CapabilityLevel {
    let has_search = available_tools
        .iter()
        .any(|t| t == TOOL_UNIFIED_SEARCH || t == TOOL_LIST_FILES);
    let has_file = available_tools
        .iter()
        .any(|t| t == TOOL_UNIFIED_FILE || t == TOOL_READ_FILE);
    let has_exec = available_tools.iter().any(|t| t == TOOL_UNIFIED_EXEC);

    if has_search {
        CapabilityLevel::CodeSearch
    } else if has_file {
        CapabilityLevel::Editing
    } else if has_exec {
        CapabilityLevel::Bash
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
        assert!(
            guidelines.contains("Mode: read-only"),
            "Should detect read-only mode when edit/write/exec tools unavailable"
        );
        assert!(
            guidelines.contains("do not modify files"),
            "Should explain read-only constraints"
        );
    }

    #[test]
    fn test_tool_preference_guidance() {
        let tools = vec!["unified_exec".to_string(), "unified_search".to_string()];
        let guidelines = generate_tool_guidelines(&tools, None);
        assert!(
            guidelines.contains("Prefer `unified_search`"),
            "Should suggest using search over unified_exec"
        );
        assert!(
            guidelines.contains("action='structural'") && guidelines.contains("`lang` when known"),
            "Should prefer structural search for code"
        );
    }

    #[test]
    fn test_edit_workflow_guidance() {
        let tools = vec!["unified_file".to_string()];
        let guidelines = generate_tool_guidelines(&tools, None);
        assert!(
            guidelines.contains("`unified_file`") && guidelines.contains("Read before `edit`"),
            "Should advise reading before editing"
        );
    }

    #[test]
    fn test_write_edit_guidance() {
        let tools = vec!["unified_file".to_string()];
        let guidelines = generate_tool_guidelines(&tools, None);
        assert!(
            guidelines.contains("`write` for new files"),
            "Should explain write for new files"
        );
        assert!(
            guidelines.contains("Read before `edit`"),
            "Should explain edit for modifications"
        );
    }

    #[test]
    fn test_apply_patch_anchor_guidance() {
        let tools = vec!["unified_file".to_string(), "apply_patch".to_string()];
        let guidelines = generate_tool_guidelines(&tools, None);
        assert!(
            guidelines.contains("`apply_patch`: surgical patches"),
            "Should mention apply_patch availability"
        );
    }

    #[test]
    fn test_harness_browse_tool_guidance() {
        let tools = vec!["list_files".to_string(), "read_file".to_string()];
        let guidelines = generate_tool_guidelines(&tools, None);
        assert!(guidelines.contains("Prefer `list_files`"));
        assert!(guidelines.contains("Prefer `read_file`"));
        assert!(guidelines.contains("`offset` and `limit`"));
        assert!(guidelines.contains("`page` and `per_page`"));
    }

    #[test]
    fn test_capability_basic_guidance() {
        let tools = vec![];
        let guidelines = generate_tool_guidelines(&tools, Some(CapabilityLevel::Basic));
        assert!(
            guidelines.contains("Mode: limited"),
            "Should indicate limited mode"
        );
        assert!(
            guidelines.contains("enable more capabilities"),
            "Should suggest asking user for more capabilities"
        );
    }

    #[test]
    fn test_capability_file_reading_guidance() {
        let tools = vec!["unified_file".to_string()];
        let guidelines = generate_tool_guidelines(&tools, Some(CapabilityLevel::FileReading));
        assert!(
            guidelines.contains("Mode: read-only"),
            "Should indicate read-only mode"
        );
        assert!(
            guidelines.contains("do not modify"),
            "Should explain no modification capability"
        );
    }

    #[test]
    fn test_full_capabilities_no_special_guidance() {
        let tools = vec![
            "unified_file".to_string(),
            "unified_exec".to_string(),
            "unified_search".to_string(),
        ];
        let guidelines = generate_tool_guidelines(&tools, Some(CapabilityLevel::Editing));

        // Should not have capability warnings (only tool preference guidance)
        assert!(
            !guidelines.contains("Mode: limited"),
            "Should not show limited mode warning"
        );
        assert!(
            !guidelines.contains("Mode: read-only"),
            "Should not show read-only mode warning"
        );
    }

    #[test]
    fn test_empty_tools_shows_read_only_mode() {
        let tools = vec![];
        let guidelines = generate_tool_guidelines(&tools, None);
        // Empty tools = no modification tools = read-only mode
        assert!(
            guidelines.contains("Mode: read-only"),
            "Empty tools should trigger read-only mode detection"
        );
    }

    #[test]
    fn test_capability_inference_file_reading() {
        let tools = vec!["unified_file".to_string()];
        assert_eq!(
            infer_capability_level(&tools),
            CapabilityLevel::Editing,
            "Should infer Editing from unified_file tool"
        );
    }

    #[test]
    fn test_capability_inference_editing() {
        let tools = vec!["unified_file".to_string()];
        assert_eq!(
            infer_capability_level(&tools),
            CapabilityLevel::Editing,
            "Should infer Editing from unified_file tool"
        );
    }

    #[test]
    fn test_capability_inference_bash() {
        let tools = vec!["unified_exec".to_string()];
        assert_eq!(
            infer_capability_level(&tools),
            CapabilityLevel::Bash,
            "Should infer Bash from unified_exec tool"
        );
    }

    #[test]
    fn test_capability_inference_file_listing() {
        let tools = vec!["unified_search".to_string()];
        assert_eq!(
            infer_capability_level(&tools),
            CapabilityLevel::CodeSearch,
            "Should infer CodeSearch from unified_search tool"
        );
    }

    #[test]
    fn test_capability_inference_code_search() {
        let tools = vec!["unified_search".to_string()];
        assert_eq!(
            infer_capability_level(&tools),
            CapabilityLevel::CodeSearch,
            "Should infer CodeSearch from unified_search tool"
        );
    }

    #[test]
    fn test_capability_inference_basic() {
        let tools = vec![];
        assert_eq!(
            infer_capability_level(&tools),
            CapabilityLevel::Basic,
            "Should infer Basic from empty tool list"
        );

        let tools = vec!["unknown_tool".to_string()];
        assert_eq!(
            infer_capability_level(&tools),
            CapabilityLevel::Basic,
            "Should infer Basic from unrecognized tools"
        );
    }

    #[test]
    fn test_capability_inference_precedence() {
        // Code search should take precedence over editing
        let tools = vec!["unified_file".to_string(), "unified_search".to_string()];
        assert_eq!(
            infer_capability_level(&tools),
            CapabilityLevel::CodeSearch,
            "CodeSearch should have highest precedence"
        );

        // Editing should take precedence over command execution
        let tools = vec!["unified_exec".to_string(), "unified_file".to_string()];
        assert_eq!(
            infer_capability_level(&tools),
            CapabilityLevel::Editing,
            "Editing should take precedence over command execution"
        );
    }
}
