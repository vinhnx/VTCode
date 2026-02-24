use crate::config::types::CapabilityLevel;

// Tool name constants - single source of truth for tool identifiers
const TOOL_UNIFIED_EXEC: &str = "unified_exec";
const TOOL_UNIFIED_FILE: &str = "unified_file";
const TOOL_UNIFIED_SEARCH: &str = "unified_search";
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
/// let tools = vec!["unified_file".to_string(), "unified_search".to_string()];
/// let guidelines = generate_tool_guidelines(&tools, None);
/// assert!(guidelines.contains("READ-ONLY MODE"));
/// ```
pub fn generate_tool_guidelines(
    available_tools: &[String],
    capability_level: Option<CapabilityLevel>,
) -> String {
    let mut guidelines = Vec::new();

    // Detect tool availability
    let has_bash = available_tools.iter().any(|t| t == TOOL_UNIFIED_EXEC);
    let has_file = available_tools.iter().any(|t| t == TOOL_UNIFIED_FILE);
    let has_search = available_tools.iter().any(|t| t == TOOL_UNIFIED_SEARCH);
    let has_apply_patch = available_tools.iter().any(|t| t == TOOL_APPLY_PATCH);

    // Read-only mode detection
    if !has_bash && !has_file {
        guidelines.push(
            "**READ-ONLY MODE**: You cannot modify files or execute commands. \
             Focus on analysis, planning, and providing recommendations."
                .to_string(),
        );
    }

    // Tool preference guidelines
    if has_bash && has_search {
        guidelines.push(
            "**Tool Selection**: Prefer `unified_search` (action='grep' or 'list') over `unified_exec` \
             for file exploration - they're faster, provide structured output, and \
             respect .gitignore automatically."
                .to_string(),
        );
    }

    // Git diff guidance - requires bash to run git commands
    if has_bash {
        guidelines.push(
            "**Git Diff Requests**: When user asks to 'show diff', 'git diff', or 'view changes': \
             ALWAYS run `git diff -- <path>` via `unified_exec`. NEVER read file content and fabricate a diff - \
             reading a file shows current content, not what changed. \
             Examples: `git diff` (all changes), `git diff -- src/main.rs` (specific file), \
             `git diff HEAD~1` (vs previous commit)."
                .to_string(),
        );
    }

    if has_file {
        guidelines.push(
            "**File Workflow**: Always use `unified_file` (action='read') to examine content before \
             using action='edit' - this ensures accurate `old_str` matching and \
             prevents edit failures. Use action='write' for new files, action='edit' for \
             modifications to existing files."
                .to_string(),
        );

        if has_apply_patch {
            guidelines.push(
                "**Diff vs Patch**: `git diff` (via `unified_exec`) is READ-ONLY to VIEW changes. \
                 `apply_patch` and `unified_file` (action='patch') are for WRITING changes. \
                 Never use patch tools when user only wants to view a diff."
                    .to_string(),
            );
        } else {
            guidelines.push(
                "**Diff vs Patch**: `git diff` (via `unified_exec`) is READ-ONLY to VIEW changes. \
                 Use `unified_file` with action='patch' for WRITING patch changes. \
                 Do not send patch text to `read_file`/`unified_file` read mode."
                    .to_string(),
            );
        }

        guidelines.push(
            "**Dotfile Protection**: Hidden configuration files (dotfiles like .gitignore, .env, \
             .bashrc, .ssh/*, etc.) are PROTECTED and require explicit user confirmation before \
             modification. If a dotfile modification is blocked, do NOT retry - instead inform \
             the user that dotfile protection requires their explicit approval. Never attempt to \
             modify dotfiles repeatedly or work around the protection."
                .to_string(),
        );

        guidelines.push(
            "**Loop Detection**: If you see 'Loop Detected' or similar errors, STOP retrying the \
             same operation. Instead: (1) Acknowledge the issue to the user, (2) Diagnose the root \
             cause, (3) Re-plan into smaller composable slices with expected outcome + verification \
             (use `task_tracker` when available), (4) Execute one slice and verify before the next. \
             Avoid repeated calls with identical arguments; change approach immediately after a failed repeat."
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

    format!(
        "\n\n## TOOL USAGE GUIDELINES\n\n{}",
        guidelines.join("\n\n")
    )
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
/// assert_eq!(infer_capability_level(&tools), CapabilityLevel::FileReading);
/// ```
pub fn infer_capability_level(available_tools: &[String]) -> CapabilityLevel {
    let has_search = available_tools.iter().any(|t| t == TOOL_UNIFIED_SEARCH);
    let has_file = available_tools.iter().any(|t| t == TOOL_UNIFIED_FILE);
    let has_bash = available_tools.iter().any(|t| t == TOOL_UNIFIED_EXEC);

    if has_search {
        CapabilityLevel::CodeSearch
    } else if has_file {
        CapabilityLevel::Editing
    } else if has_bash {
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
        let tools = vec!["unified_exec".to_string(), "unified_search".to_string()];
        let guidelines = generate_tool_guidelines(&tools, None);
        assert!(
            guidelines.contains("Prefer `unified_search`"),
            "Should suggest using search over bash"
        );
        assert!(
            guidelines.contains("unified_exec"),
            "Should mention bash as alternative"
        );
    }

    #[test]
    fn test_edit_workflow_guidance() {
        let tools = vec!["unified_file".to_string()];
        let guidelines = generate_tool_guidelines(&tools, None);
        assert!(
            guidelines.contains("unified_file") && guidelines.contains("before"),
            "Should advise reading before editing"
        );
        assert!(
            guidelines.contains("old_str"),
            "Should mention old_str matching requirement"
        );
    }

    #[test]
    fn test_write_edit_guidance() {
        let tools = vec!["unified_file".to_string()];
        let guidelines = generate_tool_guidelines(&tools, None);
        assert!(
            guidelines.contains("action='write'") && guidelines.contains("new files"),
            "Should explain write for new files"
        );
        assert!(
            guidelines.contains("action='edit'") && guidelines.contains("modifications"),
            "Should explain edit for modifications"
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
        let tools = vec!["unified_file".to_string()];
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
            "unified_file".to_string(),
            "unified_exec".to_string(),
            "unified_search".to_string(),
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

        // Editing should take precedence over bash
        let tools = vec!["unified_exec".to_string(), "unified_file".to_string()];
        assert_eq!(
            infer_capability_level(&tools),
            CapabilityLevel::Editing,
            "Editing should take precedence over Bash"
        );
    }
}
