/// Centralized error messages for tool operations.
///
/// This module provides consistent, reusable error messages across tool
/// implementations to ensure uniformity in user-facing error reporting.
///
/// Only actively-used message groups are retained. Submodules that were
/// defined but never referenced by tool implementations have been removed.
pub mod agent_execution {
    /// Marker used when planning workflow blocks a mutating tool call.
    pub const PLANNING_DENIED_CONTEXT: &str = "tool denied by planning workflow";
    /// Prefix for loop detection failures. Classify a failure with
    /// [`is_loop_detection_block_message`], which matches the whole generated
    /// shape, rather than searching for this prefix inside arbitrary text.
    pub const LOOP_DETECTION_PREFIX: &str = "Loop detection";
    const LOOP_BLOCK_TOOL_OPEN: &str = ": Tool '";
    const LOOP_BLOCK_CALLED: &str = "' has been called ";
    const LOOP_BLOCK_BLOCKED: &str = " times with identical parameters and is now blocked.";
    /// Canonical line stating the consequence of a loop detection block.
    pub const LOOP_RETRY_BLOCKED_LINE: &str =
        "The call was not executed, and repeating it with the same parameters will be blocked again.";

    /// Build the canonical Planning workflow denial message.
    pub fn planning_workflow_denial_message(tool_name: &str) -> String {
        format!(
            "Tool '{tool_name}' execution failed: tool denied by planning workflow\n\n\
             This call is not a recognized read-only planning action, so it is blocked during planning.\n\n\
             Available during planning:\n\
             - Read files: exec_command with readonly shell inspection commands such as sed, rg, ls, find, and git show\n\
             - Run recognized commands: cargo check, cargo nextest run, git status, command -v, ls, grep, find, diff\n\
             - Search code: exec_command with rg or other readonly search commands\n\
             - Use task_tracker\n\n\
             Planning shell shapes: use `&&` chains or separate calls; `;`, `||`, redirections (`>`, `2>`), and `*` globs are not recognized. Prefer `rg --files src | rg '\\.md$'` over `ls */*.md`.\n\n\
             To start implementation:\n\
             1. Wait for the user to review and approve the plan in the UI\n\
             2. After approval, use apply_patch for file edits\n\n\
             Fallback if automatic planning handoff keeps failing: type `implement` to present the plan again."
        )
    }

    /// Build the canonical loop-detection block message.
    pub fn loop_detection_block_message(tool_name: &str, repeat_count: u64, original_error: Option<&str>) -> String {
        let mut message = format!(
            "{LOOP_DETECTION_PREFIX}{LOOP_BLOCK_TOOL_OPEN}{tool_name}{LOOP_BLOCK_CALLED}{repeat_count}{LOOP_BLOCK_BLOCKED}\n\n\
             {LOOP_RETRY_BLOCKED_LINE}\n\n\
             If you need the result from this tool:\n\
             1. Check if you already have the result from a previous successful call in your conversation history\n\
             2. If not available, use a different approach or modify your request"
        );

        if let Some(error) = original_error {
            message.push_str("\n\nOriginal error: ");
            message.push_str(error);
        }

        message
    }

    /// Whether `message` is a block message built by
    /// [`loop_detection_block_message`]: it must start with the exact
    /// generated opening, `Loop detection: Tool '<name>' has been called <n>
    /// times with identical parameters and is now blocked.`, so tool stderr
    /// or prose that merely mentions loop detection is not classified.
    pub fn is_loop_detection_block_message(message: &str) -> bool {
        let Some(rest) = message
            .strip_prefix(LOOP_DETECTION_PREFIX)
            .and_then(|rest| rest.strip_prefix(LOOP_BLOCK_TOOL_OPEN))
        else {
            return false;
        };
        let Some((tool_name, rest)) = rest.split_once(LOOP_BLOCK_CALLED) else {
            return false;
        };
        let count_len = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        !tool_name.is_empty() && count_len > 0 && rest[count_len..].starts_with(LOOP_BLOCK_BLOCKED)
    }

    /// Check whether an error string corresponds to planning workflow denial.
    pub fn is_planning_active_denial(error: &str) -> bool {
        error.contains(PLANNING_DENIED_CONTEXT)
    }

    /// Recovery guidance for policy rejection, distinct from malformed JSON.
    pub fn preflight_policy_correction(error: &str) -> Option<&'static str> {
        if is_planning_active_denial(error) {
            return Some(
                "Planning permits only recognized read-only command shapes. Do not retry the same denied command. Use sed, rg, git show, or command -v for inspection; defer unknown scripts until plan approval. This denial does not establish that the script itself mutates files.",
            );
        }
        let lowered = error.to_ascii_lowercase();
        if lowered.contains("command security check failed") || lowered.contains("command injection pattern detected") {
            return Some(
                "The shell command was rejected by security policy, not JSON validation. Do not retry it unchanged or bypass the policy. Use a simpler literal command without dynamic expansion, or an approved dedicated tool; report the blocker if no permitted form satisfies the task.",
            );
        }
        None
    }
}

/// Skill management error messages
pub mod skill_ops {
    pub const SKILL_NOT_FOUND: &str = "Skill not found";
    pub const SKILL_ALREADY_EXISTS: &str = "Skill already exists";
    pub const INVALID_SKILL_FORMAT: &str = "Invalid skill format";
    pub const SKILL_SAVE_FAILED: &str = "Failed to save skill";
    pub const SKILL_LOAD_FAILED: &str = "Failed to load skill";

    /// Build a formatted "skill not found" error message.
    pub fn skill_not_found_error(name: &str) -> anyhow::Error {
        anyhow::anyhow!("Skill '{name}' not found")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn internal_unified_tool_name(suffix: &str) -> String {
        format!("unified_{suffix}")
    }

    #[test]
    fn test_agent_execution_message_helpers() {
        let planning_msg = agent_execution::planning_workflow_denial_message("write_file");
        assert!(agent_execution::is_planning_active_denial(&planning_msg));
        assert!(planning_msg.contains("blocked during planning"));
        assert!(planning_msg.contains("cargo check"));
        assert!(planning_msg.contains("exec_command"));
        assert!(planning_msg.contains("apply_patch"));
        assert!(planning_msg.contains("Planning shell shapes"));
        assert!(planning_msg.contains("rg --files"));
        assert!(!planning_msg.contains(&internal_unified_tool_name("file")));
        assert!(!planning_msg.contains(&internal_unified_tool_name("exec")));
        assert!(!planning_msg.contains(&internal_unified_tool_name("search")));
        assert!(!planning_msg.contains(&format!("/{}", "mode")));
        assert!(!planning_msg.contains("DO NOT retry this tool or use /plan off"));

        let loop_msg = agent_execution::loop_detection_block_message("read_file", 3, Some("base error"));
        assert!(loop_msg.starts_with(agent_execution::LOOP_DETECTION_PREFIX));
        assert!(loop_msg.contains(agent_execution::LOOP_RETRY_BLOCKED_LINE));
        assert!(loop_msg.contains("Original error: base error"));
        assert!(agent_execution::is_loop_detection_block_message(&loop_msg));
    }

    #[test]
    fn loop_detection_classification_ignores_prose_mentions() {
        use agent_execution::is_loop_detection_block_message;

        let generated = agent_execution::loop_detection_block_message("mcp::fs::read", 12, None);
        assert!(is_loop_detection_block_message(&generated));

        for message in [
            "warning: loop detection disabled for this target",
            "Loop detection is off; running tests",
            "stderr: Loop detection: Tool 'x' has been called 3 times with identical parameters and is now blocked.",
            "Loop detection: Tool 'x' has been called many times with identical parameters and is now blocked.",
            "Loop detection: Tool '' has been called 3 times with identical parameters and is now blocked.",
            "LOOP DETECTION: Tool 'x' has been called 3 times with identical parameters and is now blocked.",
        ] {
            assert!(!is_loop_detection_block_message(message), "{message:?}");
        }
    }
}
