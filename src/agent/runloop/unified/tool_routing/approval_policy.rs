use serde_json::Value;
use vtcode_core::exec_policy::AskForApproval;
use vtcode_core::tools::{ToolRiskContext, ToolSource, WorkspaceTrust};

fn command_matches_subcommands(command_words: &[String], subcommands: &[&str]) -> bool {
    command_words
        .get(1)
        .is_some_and(|subcommand| subcommands.iter().any(|candidate| subcommand == candidate))
}

fn shell_command_accesses_network(command_words: &[String]) -> bool {
    match command_words.first().map(String::as_str) {
        Some("curl" | "wget" | "gh") => true,
        Some("cargo") => command_matches_subcommands(
            command_words,
            &["add", "install", "login", "publish", "search", "update"],
        ),
        Some("npm" | "pnpm" | "yarn") => command_matches_subcommands(
            command_words,
            &["add", "install", "login", "publish", "search", "update"],
        ),
        Some("pip") => command_matches_subcommands(
            command_words,
            &["download", "index", "install", "search", "wheel"],
        ),
        _ => false,
    }
}

fn tool_accesses_network(tool_name: &str, tool_args: Option<&Value>) -> bool {
    let canonical = vtcode_core::tools::names::canonical_tool_name(tool_name);
    match canonical.as_ref() {
        "web_search" | "fetch_url" | "unified_search:web" => true,
        vtcode_core::config::constants::tools::UNIFIED_EXEC => {
            vtcode_core::tools::command_args::command_words(tool_args.unwrap_or(&Value::Null))
                .ok()
                .flatten()
                .is_some_and(|words| shell_command_accesses_network(&words))
        }
        _ => false,
    }
}

pub(super) fn build_tool_risk_context(
    tool_name: &str,
    tool_args: Option<&Value>,
) -> ToolRiskContext {
    let mut risk_context = ToolRiskContext::new(
        tool_name.to_string(),
        ToolSource::Internal,
        WorkspaceTrust::Untrusted,
    );
    let args = tool_args.unwrap_or(&Value::Null);
    let intent = vtcode_core::tools::tool_intent::classify_tool_intent(tool_name, args);
    if let Some(command_words) = vtcode_core::tools::command_args::command_words(args)
        .ok()
        .flatten()
    {
        risk_context.command_args = command_words;
    } else if !args.is_null() {
        risk_context.command_args = vec![args.to_string()];
    }
    if intent.mutating {
        risk_context = risk_context.as_write();
    }
    if intent.destructive {
        risk_context = risk_context.as_destructive();
    }
    if tool_accesses_network(tool_name, tool_args) {
        risk_context = risk_context.accesses_network();
    }
    risk_context
}

pub(super) fn trusted_auto_allows_immediate_approval(
    hook_requires_prompt: bool,
    shell_approval_reason: Option<&str>,
    risk_context: &ToolRiskContext,
    risk_level: vtcode_core::tools::RiskLevel,
) -> bool {
    !hook_requires_prompt
        && shell_approval_reason.is_none()
        && !risk_context.is_write
        && !risk_context.is_destructive
        && !risk_context.accesses_network
        && risk_level == vtcode_core::tools::RiskLevel::Low
}

pub(super) fn trusted_auto_allows_history_based_approval(
    hook_requires_prompt: bool,
    shell_approval_reason: Option<&str>,
    risk_context: &ToolRiskContext,
    risk_level: vtcode_core::tools::RiskLevel,
) -> bool {
    !hook_requires_prompt
        && shell_approval_reason.is_none()
        && !risk_context.is_write
        && !risk_context.is_destructive
        && !risk_context.accesses_network
        && risk_level == vtcode_core::tools::RiskLevel::Medium
}

pub(super) fn approval_policy_rejects_prompt(
    approval_policy: AskForApproval,
    requires_rule_prompt: bool,
    requires_sandbox_prompt: bool,
) -> bool {
    (requires_rule_prompt && approval_policy.rejects_rule_prompt())
        || (requires_sandbox_prompt && approval_policy.rejects_sandbox_prompt())
}

#[cfg(test)]
mod tests {
    use super::build_tool_risk_context;
    use serde_json::json;

    #[test]
    fn cargo_check_is_not_marked_as_network_access() {
        let args = json!({
            "action": "run",
            "command": "cargo check -p vtcode",
        });

        let risk_context = build_tool_risk_context("unified_exec", Some(&args));
        assert!(!risk_context.accesses_network);
    }

    #[test]
    fn cargo_install_is_marked_as_network_access() {
        let args = json!({
            "action": "run",
            "command": "cargo install cargo-nextest",
        });

        let risk_context = build_tool_risk_context("unified_exec", Some(&args));
        assert!(risk_context.accesses_network);
    }

    #[test]
    fn gh_commands_are_marked_as_network_access() {
        let args = json!({
            "action": "run",
            "command": "gh pr checks",
        });

        let risk_context = build_tool_risk_context("unified_exec", Some(&args));
        assert!(risk_context.accesses_network);
    }
}
