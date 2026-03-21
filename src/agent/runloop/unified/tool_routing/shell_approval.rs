use serde_json::Value;

use crate::agent::runloop::unified::tool_summary::{describe_tool_action, humanize_tool_name};

use super::permission_prompt::{
    extract_shell_approval_command_words, extract_shell_command_text,
    extract_shell_permission_scope_signature, extract_shell_persistent_approval_prefix_rule,
    render_shell_approval_command_words, render_shell_persistent_approval_prefix_entry,
};

#[derive(Debug, Clone)]
pub(super) struct ApprovalLearningTarget {
    pub approval_key: String,
    pub display_label: String,
}

#[derive(Debug, Clone)]
pub(super) struct ToolDisplayLabels {
    pub prompt_label: String,
    pub learning_label: String,
}

#[derive(Debug, Clone)]
pub(super) enum PersistentApprovalTarget {
    ToolLevel,
    ExactInvocation {
        display_label: String,
    },
    PrefixRule {
        prefix_rule: Vec<String>,
        display_label: String,
    },
}

fn exact_shell_learning_target(
    tool_name: &str,
    tool_args: Option<&Value>,
    default_learning_label: &str,
) -> Option<ApprovalLearningTarget> {
    let scope_signature = extract_shell_permission_scope_signature(tool_name, tool_args)?;

    if let Some(command_words) = extract_shell_approval_command_words(tool_name, tool_args) {
        let rendered_command = render_shell_approval_command_words(&command_words);
        return Some(ApprovalLearningTarget {
            approval_key: format!("{rendered_command}|{scope_signature}"),
            display_label: format!("command `{rendered_command}`"),
        });
    }

    if let Some(command_text) = extract_shell_command_text(tool_name, tool_args) {
        return Some(ApprovalLearningTarget {
            approval_key: format!("{command_text}|{scope_signature}"),
            display_label: format!("command `{command_text}`"),
        });
    }

    let fallback_key = tool_args
        .map(Value::to_string)
        .unwrap_or_else(|| tool_name.to_string());
    Some(ApprovalLearningTarget {
        approval_key: format!("{fallback_key}|{scope_signature}"),
        display_label: default_learning_label.to_string(),
    })
}

pub(super) fn approval_learning_target(
    tool_name: &str,
    tool_args: Option<&Value>,
    default_learning_label: &str,
) -> ApprovalLearningTarget {
    if let Some(scope_signature) = extract_shell_permission_scope_signature(tool_name, tool_args) {
        if let Some(prefix_rule) =
            extract_shell_persistent_approval_prefix_rule(tool_name, tool_args)
            && let Some(rendered_rule) =
                render_shell_persistent_approval_prefix_entry(tool_name, tool_args, &prefix_rule)
        {
            let rendered_prefix = render_shell_approval_command_words(&prefix_rule);
            return ApprovalLearningTarget {
                approval_key: rendered_rule,
                display_label: format!("commands starting with `{rendered_prefix}`"),
            };
        }

        return exact_shell_learning_target(tool_name, tool_args, default_learning_label)
            .unwrap_or_else(|| ApprovalLearningTarget {
                approval_key: format!("{tool_name}|{scope_signature}"),
                display_label: default_learning_label.to_string(),
            });
    }

    ApprovalLearningTarget {
        approval_key: vtcode_core::tools::names::canonical_tool_name(tool_name).into_owned(),
        display_label: default_learning_label.to_string(),
    }
}

pub(super) fn exact_shell_approval_target(
    tool_name: &str,
    tool_args: Option<&Value>,
    default_learning_label: &str,
) -> Option<ApprovalLearningTarget> {
    exact_shell_learning_target(tool_name, tool_args, default_learning_label)
}

pub(super) fn persistent_approval_target(
    tool_name: &str,
    tool_args: Option<&Value>,
    default_learning_label: &str,
) -> PersistentApprovalTarget {
    if let Some(prefix_rule) = extract_shell_persistent_approval_prefix_rule(tool_name, tool_args) {
        let rendered_prefix = render_shell_approval_command_words(&prefix_rule);
        return PersistentApprovalTarget::PrefixRule {
            prefix_rule,
            display_label: format!("commands starting with `{rendered_prefix}`"),
        };
    }

    if extract_shell_permission_scope_signature(tool_name, tool_args).is_some() {
        let learning = approval_learning_target(tool_name, tool_args, default_learning_label);
        return PersistentApprovalTarget::ExactInvocation {
            display_label: learning.display_label,
        };
    }

    PersistentApprovalTarget::ToolLevel
}

pub(super) fn tool_display_labels(tool_name: &str, tool_args: Option<&Value>) -> ToolDisplayLabels {
    let learning_label = humanize_tool_name(tool_name);
    let prompt_label = tool_args
        .map(|args| describe_tool_action(tool_name, args).0)
        .filter(|headline| !headline.is_empty())
        .unwrap_or_else(|| learning_label.clone());

    ToolDisplayLabels {
        prompt_label,
        learning_label,
    }
}
