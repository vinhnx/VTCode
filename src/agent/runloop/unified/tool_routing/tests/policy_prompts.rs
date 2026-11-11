use super::*;

#[test]
fn reject_policy_blocks_sandbox_prompts() {
    assert!(approval_policy_rejects_prompt(
        AskForApproval::Reject(RejectConfig {
            sandbox_approval: true,
            rules: false,
            request_permissions: false,
            mcp_elicitations: false,
        }),
        false,
        true,
    ));
}
#[test]
fn reject_policy_blocks_rule_prompts() {
    assert!(approval_policy_rejects_prompt(
        AskForApproval::Reject(RejectConfig {
            sandbox_approval: false,
            rules: true,
            request_permissions: false,
            mcp_elicitations: false,
        }),
        true,
        false,
    ));
}
#[test]
fn on_request_policy_keeps_prompts_enabled() {
    assert!(!approval_policy_rejects_prompt(AskForApproval::OnRequest, true, true));
}
#[test]
fn shell_learning_target_uses_scoped_prefix_rule() {
    let args = json!({
        "action": "run",
        "command": "cargo test -p vtcode",
        "prefix_rule": ["cargo", "test"],
        "sandbox_permissions": "require_escalated",
    });

    let target = approval_learning_target("exec_command", Some(&args), "Run command");
    assert_eq!(
        target.approval_key,
        "cargo test|sandbox_permissions=\"require_escalated\"|additional_permissions=null"
    );
    assert_eq!(target.display_label, "commands starting with `cargo test`");
}
#[test]
fn shell_learning_target_falls_back_to_exact_command_scope() {
    let args = json!({
        "action": "run",
        "command": "cargo test -p vtcode",
        "prefix_rule": ["cargo", "build"],
        "sandbox_permissions": "require_escalated",
    });

    let target = approval_learning_target("exec_command", Some(&args), "Run command");
    assert_eq!(
        target.approval_key,
        "cargo test -p vtcode|sandbox_permissions=\"require_escalated\"|additional_permissions=null"
    );
    assert_eq!(target.display_label, "command `cargo test -p vtcode`");
}
#[test]
fn non_shell_display_labels_keep_learning_label_stable() {
    let args = json!({
        "path": "src/main.rs"
    });

    let labels = tool_display_labels("read_file", Some(&args));
    // Title case follows the shared `humanize_tool_name` contract (pinned in
    // `vtcode-commons`); the prompt headline keeps its sentence-case verb.
    assert_eq!(labels.learning_label, "Read File");
    assert_eq!(labels.prompt_label, "Read file src/main.rs");
}
