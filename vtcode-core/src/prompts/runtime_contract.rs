use std::fmt::Write as _;

use super::system::{
    PLANNING_WORKFLOW_EXIT_INSTRUCTION_LINE, PLANNING_WORKFLOW_INTERVIEW_POLICY_LINE,
    PLANNING_WORKFLOW_NO_AUTO_EXIT_LINE, PLANNING_WORKFLOW_NO_REQUEST_USER_INPUT_POLICY_LINE,
    PLANNING_WORKFLOW_PLAN_QUALITY_LINE, PLANNING_WORKFLOW_READ_ONLY_HEADER,
    PLANNING_WORKFLOW_READ_ONLY_NOTICE_LINE, PLANNING_WORKFLOW_TASK_TRACKER_LINE,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RuntimePromptContract {
    pub full_auto: bool,
    pub planning_active: bool,
    pub request_user_input_enabled: bool,
}

pub fn append_runtime_mode_sections(prompt: &mut String, contract: RuntimePromptContract) {
    if contract.full_auto {
        append_full_auto_notice(prompt, contract);
    }

    if contract.planning_active {
        append_planning_workflow_notice(prompt, contract.request_user_input_enabled);
    }
}

fn append_full_auto_notice(prompt: &mut String, contract: RuntimePromptContract) {
    let header = if contract.planning_active {
        "# FULL-AUTO (PLANNING WORKFLOW): Work autonomously within planning workflow constraints."
    } else {
        "# FULL-AUTO: Complete task autonomously until done or blocked."
    };

    if prompt.contains(header) {
        return;
    }

    let _ = writeln!(prompt, "\n{header}");
    let _ = writeln!(
        prompt,
        "- Stay within the exposed tool list and adapt when a tool is unavailable or denied."
    );
    let _ = writeln!(
        prompt,
        "- Treat completion language as a checkpoint, not proof; only stop when `task_tracker`, verification, and resumable state agree."
    );
    if !contract.request_user_input_enabled {
        let _ = writeln!(
            prompt,
            "- `request_user_input` is unavailable in this runtime; make reasonable assumptions and continue with the available context."
        );
    }
}

fn append_planning_workflow_notice(prompt: &mut String, request_user_input_enabled: bool) {
    if prompt.contains(PLANNING_WORKFLOW_READ_ONLY_HEADER) {
        return;
    }

    prompt.push('\n');
    prompt.push_str(PLANNING_WORKFLOW_READ_ONLY_HEADER);
    prompt.push('\n');
    prompt.push_str(PLANNING_WORKFLOW_READ_ONLY_NOTICE_LINE);
    prompt.push('\n');
    prompt.push_str(PLANNING_WORKFLOW_EXIT_INSTRUCTION_LINE);
    prompt.push('\n');
    prompt.push_str(PLANNING_WORKFLOW_PLAN_QUALITY_LINE);
    prompt.push('\n');
    prompt.push_str(if request_user_input_enabled {
        PLANNING_WORKFLOW_INTERVIEW_POLICY_LINE
    } else {
        PLANNING_WORKFLOW_NO_REQUEST_USER_INPUT_POLICY_LINE
    });
    prompt.push('\n');
    prompt.push_str(PLANNING_WORKFLOW_NO_AUTO_EXIT_LINE);
    prompt.push('\n');
    prompt.push_str(PLANNING_WORKFLOW_TASK_TRACKER_LINE);
    prompt.push('\n');
}

#[cfg(test)]
mod tests {
    use super::{RuntimePromptContract, append_runtime_mode_sections};
    use crate::prompts::system::{
        PLANNING_WORKFLOW_INTERVIEW_POLICY_LINE,
        PLANNING_WORKFLOW_NO_REQUEST_USER_INPUT_POLICY_LINE, PLANNING_WORKFLOW_READ_ONLY_HEADER,
    };

    #[test]
    fn planning_workflow_uses_interview_policy_when_request_user_input_is_enabled() {
        let mut prompt = "Base prompt".to_string();

        append_runtime_mode_sections(
            &mut prompt,
            RuntimePromptContract {
                planning_active: true,
                request_user_input_enabled: true,
                ..RuntimePromptContract::default()
            },
        );

        assert!(prompt.contains(PLANNING_WORKFLOW_READ_ONLY_HEADER));
        assert!(prompt.contains(PLANNING_WORKFLOW_INTERVIEW_POLICY_LINE));
        assert!(!prompt.contains(PLANNING_WORKFLOW_NO_REQUEST_USER_INPUT_POLICY_LINE));
    }

    #[test]
    fn planning_workflow_uses_noninteractive_policy_when_request_user_input_is_disabled() {
        let mut prompt = "Base prompt".to_string();

        append_runtime_mode_sections(
            &mut prompt,
            RuntimePromptContract {
                planning_active: true,
                request_user_input_enabled: false,
                ..RuntimePromptContract::default()
            },
        );

        assert!(prompt.contains(PLANNING_WORKFLOW_READ_ONLY_HEADER));
        assert!(prompt.contains(PLANNING_WORKFLOW_NO_REQUEST_USER_INPUT_POLICY_LINE));
        assert!(!prompt.contains(PLANNING_WORKFLOW_INTERVIEW_POLICY_LINE));
    }

    #[test]
    fn full_auto_notice_mentions_missing_request_user_input_when_disabled() {
        let mut prompt = "Base prompt".to_string();

        append_runtime_mode_sections(
            &mut prompt,
            RuntimePromptContract {
                full_auto: true,
                request_user_input_enabled: false,
                ..RuntimePromptContract::default()
            },
        );

        assert!(prompt.contains("# FULL-AUTO: Complete task autonomously until done or blocked."));
        assert!(prompt.contains("`request_user_input` is unavailable in this runtime"));
        assert!(prompt.contains("completion language as a checkpoint"));
    }
}
