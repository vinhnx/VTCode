//! Regressions for the ACP permission → registry policy hand-off (issue #791).

use super::*;
use crate::acp::ToolCallStatus;
use crate::zed::agent::test_support::build_agent;
use assert_fs::TempDir;
use vtcode_core::tool_policy::ToolPolicy;

const PATCH_ADD_FILE: &str = "*** Begin Patch\n*** Add File: hello.txt\n+hello\n*** End Patch\n";

fn patch_args() -> Value {
    json!({ "input": PATCH_ADD_FILE })
}

async fn run_apply_patch(workspace: &TempDir, permission_granted: bool) -> ToolExecutionReport {
    let agent = build_agent(workspace.path()).await;
    agent
        .execute_local_tool(tools::APPLY_PATCH, &patch_args(), "call-1", permission_granted, "build")
        .await
}

#[tokio::test]
async fn granted_client_permission_satisfies_prompt_policy() {
    let temp = TempDir::new().unwrap();

    let report = Box::pin(run_apply_patch(&temp, true)).await;

    assert!(
        matches!(report.status, ToolCallStatus::Completed),
        "granted apply_patch should run: {}",
        report.llm_response
    );
    assert_eq!(
        std::fs::read_to_string(temp.path().join("hello.txt")).unwrap(),
        "hello\n",
        "granted apply_patch should create the file"
    );
}

#[tokio::test]
async fn missing_client_permission_still_denies_prompt_policy() {
    let temp = TempDir::new().unwrap();

    let report = Box::pin(run_apply_patch(&temp, false)).await;

    assert!(
        matches!(report.status, ToolCallStatus::Failed),
        "ungranted apply_patch must not run: {}",
        report.llm_response
    );
    assert!(
        report.llm_response.contains("denied by policy"),
        "failure should come from the policy gate: {}",
        report.llm_response
    );
    assert!(!temp.path().join("hello.txt").exists(), "ungranted apply_patch must not write files");
}

#[tokio::test]
async fn configured_deny_policy_outranks_client_permission() {
    let temp = TempDir::new().unwrap();
    let agent = build_agent(temp.path()).await;
    agent
        .local_tool_registry
        .set_tool_policy(tools::APPLY_PATCH, ToolPolicy::Deny)
        .await
        .expect("deny policy should persist in the workspace");

    let report = agent
        .execute_local_tool(tools::APPLY_PATCH, &patch_args(), "call-1", true, "build")
        .await;

    assert!(
        matches!(report.status, ToolCallStatus::Failed),
        "workspace deny must win over a client approval: {}",
        report.llm_response
    );
    assert!(
        report.llm_response.contains("denied by policy"),
        "failure should come from the policy gate: {}",
        report.llm_response
    );
    assert!(!temp.path().join("hello.txt").exists(), "denied apply_patch must not write files");
}

#[tokio::test]
async fn read_only_primary_agent_blocks_mutating_local_call() {
    use crate::zed::agent::test_support::build_agent_with_default_primary_agent;

    let temp = TempDir::new().unwrap();
    let agent = build_agent_with_default_primary_agent(temp.path(), "plan").await;

    let report = agent
        .execute_local_tool(tools::APPLY_PATCH, &patch_args(), "call-1", true, "plan")
        .await;

    assert!(
        matches!(report.status, ToolCallStatus::Failed),
        "plan agent must not run apply_patch: {}",
        report.llm_response
    );
    assert!(
        report.llm_response.contains("plan"),
        "failure should name the selected agent: {}",
        report.llm_response
    );
    assert!(!temp.path().join("hello.txt").exists(), "blocked apply_patch must not write files");
}

#[tokio::test]
async fn denied_effective_permissions_block_local_call_for_build_agent() {
    use crate::zed::agent::test_support::build_agent_with_policies;
    use vtcode_config::CommandsConfig;

    let temp = TempDir::new().unwrap();
    let permissions = vtcode_config::core::permissions::PermissionsConfig {
        deny: vec![tools::APPLY_PATCH.to_string()],
        ..vtcode_config::core::permissions::PermissionsConfig::default()
    };
    let agent = build_agent_with_policies(
        temp.path(),
        "build",
        CommandsConfig::default(),
        vtcode_config::SandboxConfig::default(),
        permissions,
        Vec::new(),
    )
    .await;

    let report = agent
        .execute_local_tool(tools::APPLY_PATCH, &patch_args(), "call-1", true, "build")
        .await;

    assert!(
        matches!(report.status, ToolCallStatus::Failed),
        "denied permissions must block apply_patch: {}",
        report.llm_response
    );
    assert!(
        report.llm_response.contains("denied by primary agent"),
        "failure should come from the dispatch permission check: {}",
        report.llm_response
    );
    assert!(!temp.path().join("hello.txt").exists(), "denied apply_patch must not write files");
}

#[tokio::test]
async fn local_registry_receives_operator_sandbox_config() {
    use crate::zed::agent::test_support::build_agent_with_policies;
    use vtcode_config::CommandsConfig;

    let temp = TempDir::new().unwrap();
    let sandbox = vtcode_config::SandboxConfig {
        enabled: true,
        ..vtcode_config::SandboxConfig::default()
    };
    let agent = build_agent_with_policies(
        temp.path(),
        "build",
        CommandsConfig::default(),
        sandbox,
        vtcode_config::core::permissions::PermissionsConfig::default(),
        Vec::new(),
    )
    .await;

    assert!(
        agent.local_tool_registry.sandbox_config().enabled,
        "ACP local tools must run under the operator sandbox"
    );
}
