use crate::config::types::CapabilityLevel;

// Tool name constants - single source of truth for tool identifiers
const TOOL_RUN_PTY_CMD: &str = "run_pty_cmd";
const TOOL_EDIT_FILE: &str = "edit_file";
const TOOL_WRITE_FILE: &str = "write_file";
const TOOL_READ_FILE: &str = "read_file";
const TOOL_GREP_FILE: &str = "grep_file";
const TOOL_LIST_FILES: &str = "list_files";
const TOOL_CODE_INTELLIGENCE: &str = "code_intelligence";

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
/// let tools = vec!["read_file".to_string(), "grep_file".to_string()];
/// let guidelines = generate_tool_guidelines(&tools, None);
/// assert!(guidelines.contains("READ-ONLY MODE"));
/// ```
pub fn generate_tool_guidelines(
    available_tools: &[String],
    capability_level: Option<CapabilityLevel>,
) -> String {
    let mut guidelines = Vec::new();

    // Detect tool availability
    let has_bash = available_tools.iter().any(|t| t == TOOL_RUN_PTY_CMD);
    let has_edit = available_tools.iter().any(|t| t == TOOL_EDIT_FILE);
    let has_write = available_tools.iter().any(|t| t == TOOL_WRITE_FILE);
    let has_read = available_tools.iter().any(|t| t == TOOL_READ_FILE);
    let has_grep = available_tools.iter().any(|t| t == TOOL_GREP_FILE);
    let has_list = available_tools.iter().any(|t| t == TOOL_LIST_FILES);

    // Read-only mode detection
    if !has_bash && !has_edit && !has_write {
        guidelines.push(
            "**READ-ONLY MODE**: You cannot modify files or execute commands. \
             Focus on analysis, planning, and providing recommendations."
                .to_string(),
        );
    }

    // Tool preference guidelines
    if has_bash && (has_grep || has_list) {
        guidelines.push(
            "**Tool Selection**: Prefer `grep_file`/`list_files` over `run_pty_cmd` \
             for file exploration - they're faster, provide structured output, and \
             respect .gitignore automatically."
                .to_string(),
        );
    }

    if has_read && has_edit {
        guidelines.push(
            "**Edit Workflow**: Always use `read_file` to examine content before \
             using `edit_file` - this ensures accurate `old_str` matching and \
             prevents edit failures."
                .to_string(),
        );
    }

    if has_write && has_edit {
        guidelines.push(
            "**File Creation**: Use `write_file` for new files, `edit_file` for \
             modifications to existing files. Never use both on the same file."
                .to_string(),
        );
    }

    // Capability-based guidance
    if let Some(level) = capability_level {
        match level {
            CapabilityLevel::Basic => {
                guidelines.push(
                    "**Limited Mode**: Only basic chat available. If you need to \
                     read or modify files, ask the user to enable additional capabilities."
                        .to_string(),
                );
            }
            CapabilityLevel::FileReading => {
                guidelines.push(
                    "**Analysis Mode**: You can read and search files but not modify them. \
                     Provide detailed analysis and suggest changes for the user to implement."
                        .to_string(),
                );
            }
            CapabilityLevel::FileListing | CapabilityLevel::Bash => {
                // Normal operational mode - no special guidance needed
            }
            CapabilityLevel::Editing | CapabilityLevel::CodeSearch => {
                // Full capabilities - no special guidance needed
            }
        }
    }

    if guidelines.is_empty() {
        return String::new();
    }

    format!("\n\n## TOOL USAGE GUIDELINES\n\n{}", guidelines.join("\n\n"))
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
/// let tools = vec!["read_file".to_string()];
/// assert_eq!(infer_capability_level(&tools), CapabilityLevel::FileReading);
///
/// let tools = vec!["edit_file".to_string(), "write_file".to_string()];
/// assert_eq!(infer_capability_level(&tools), CapabilityLevel::Editing);
/// ```
pub fn infer_capability_level(available_tools: &[String]) -> CapabilityLevel {
    let has_code_search = available_tools
        .iter()
        .any(|t| t.contains(TOOL_CODE_INTELLIGENCE));
    let has_edit = available_tools
        .iter()
        .any(|t| t == TOOL_EDIT_FILE || t == TOOL_WRITE_FILE);
    let has_bash = available_tools.iter().any(|t| t == TOOL_RUN_PTY_CMD);
    let has_list = available_tools.iter().any(|t| t == TOOL_LIST_FILES);
    let has_read = available_tools.iter().any(|t| t == TOOL_READ_FILE);

    if has_code_search {
        CapabilityLevel::CodeSearch
    } else if has_edit {
        CapabilityLevel::Editing
    } else if has_bash {
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
        let tools = vec!["read_file".to_string(), "grep_file".to_string()];
        let guidelines = generate_tool_guidelines(&tools, None);
        assert!(
            guidelines.contains("READ-ONLY MODE"),
            "Should detect read-only mode when edit/write/bash tools unavailable"
        );
        assert!(
            guidelines.contains("cannot modify files"),
            "Should explain read-only constraints"
        );
    }

    #[test]
    fn test_tool_preference_guidance() {
        let tools = vec![
            "run_pty_cmd".to_string(),
            "grep_file".to_string(),
            "list_files".to_string(),
        ];
        let guidelines = generate_tool_guidelines(&tools, None);
        assert!(
            guidelines.contains("Prefer `grep_file`/`list_files`"),
            "Should suggest using grep/list over bash"
        );
        assert!(
            guidelines.contains("run_pty_cmd"),
            "Should mention bash as alternative"
        );
    }

    #[test]
    fn test_edit_workflow_guidance() {
        let tools = vec!["read_file".to_string(), "edit_file".to_string()];
        let guidelines = generate_tool_guidelines(&tools, None);
        assert!(
            guidelines.contains("read_file") && guidelines.contains("before"),
            "Should advise reading before editing"
        );
        assert!(
            guidelines.contains("old_str"),
            "Should mention old_str matching requirement"
        );
    }

    #[test]
    fn test_write_edit_guidance() {
        let tools = vec![
            "write_file".to_string(),
            "edit_file".to_string(),
            "read_file".to_string(),
        ];
        let guidelines = generate_tool_guidelines(&tools, None);
        assert!(
            guidelines.contains("write_file") && guidelines.contains("new files"),
            "Should explain write_file for new files"
        );
        assert!(
            guidelines.contains("edit_file") && guidelines.contains("modifications"),
            "Should explain edit_file for modifications"
        );
    }

    #[test]
    fn test_capability_basic_guidance() {
        let tools = vec![];
        let guidelines = generate_tool_guidelines(&tools, Some(CapabilityLevel::Basic));
        assert!(
            guidelines.contains("Limited Mode"),
            "Should indicate limited mode"
        );
        assert!(
            guidelines.contains("ask the user"),
            "Should suggest asking user for more capabilities"
        );
    }

    #[test]
    fn test_capability_file_reading_guidance() {
        let tools = vec!["read_file".to_string()];
        let guidelines = generate_tool_guidelines(&tools, Some(CapabilityLevel::FileReading));
        assert!(
            guidelines.contains("Analysis Mode"),
            "Should indicate analysis mode"
        );
        assert!(
            guidelines.contains("not modify"),
            "Should explain no modification capability"
        );
    }

    #[test]
    fn test_full_capabilities_no_special_guidance() {
        let tools = vec![
            "read_file".to_string(),
            "write_file".to_string(),
            "edit_file".to_string(),
            "run_pty_cmd".to_string(),
            "grep_file".to_string(),
        ];
        let guidelines = generate_tool_guidelines(&tools, Some(CapabilityLevel::Editing));

        // Should not have capability warnings (only tool preference guidance)
        assert!(
            !guidelines.contains("Limited Mode"),
            "Should not show limited mode warning"
        );
        assert!(
            !guidelines.contains("Analysis Mode"),
            "Should not show analysis mode warning"
        );
    }

    #[test]
    fn test_empty_tools_shows_read_only_mode() {
        let tools = vec![];
        let guidelines = generate_tool_guidelines(&tools, None);
        // Empty tools = no modification tools = read-only mode
        assert!(
            guidelines.contains("READ-ONLY MODE"),
            "Empty tools should trigger read-only mode detection"
        );
    }

    #[test]
    fn test_capability_inference_file_reading() {
        let tools = vec!["read_file".to_string()];
        assert_eq!(
            infer_capability_level(&tools),
            CapabilityLevel::FileReading,
            "Should infer FileReading from read_file tool"
        );
    }

    #[test]
    fn test_capability_inference_editing() {
        let tools = vec!["edit_file".to_string()];
        assert_eq!(
            infer_capability_level(&tools),
            CapabilityLevel::Editing,
            "Should infer Editing from edit_file tool"
        );

        let tools = vec!["write_file".to_string()];
        assert_eq!(
            infer_capability_level(&tools),
            CapabilityLevel::Editing,
            "Should infer Editing from write_file tool"
        );
    }

    #[test]
    fn test_capability_inference_bash() {
        let tools = vec!["run_pty_cmd".to_string()];
        assert_eq!(
            infer_capability_level(&tools),
            CapabilityLevel::Bash,
            "Should infer Bash from run_pty_cmd tool"
        );
    }

    #[test]
    fn test_capability_inference_file_listing() {
        let tools = vec!["list_files".to_string()];
        assert_eq!(
            infer_capability_level(&tools),
            CapabilityLevel::FileListing,
            "Should infer FileListing from list_files tool"
        );
    }

    #[test]
    fn test_capability_inference_code_search() {
        let tools = vec!["code_intelligence".to_string()];
        assert_eq!(
            infer_capability_level(&tools),
            CapabilityLevel::CodeSearch,
            "Should infer CodeSearch from code_intelligence tool"
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
        let tools = vec![
            "edit_file".to_string(),
            "code_intelligence".to_string(),
        ];
        assert_eq!(
            infer_capability_level(&tools),
            CapabilityLevel::CodeSearch,
            "CodeSearch should have highest precedence"
        );

        // Editing should take precedence over bash
        let tools = vec!["run_pty_cmd".to_string(), "edit_file".to_string()];
        assert_eq!(
            infer_capability_level(&tools),
            CapabilityLevel::Editing,
            "Editing should take precedence over Bash"
        );
    }
}
