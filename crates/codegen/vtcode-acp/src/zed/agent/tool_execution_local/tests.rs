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
        .execute_local_tool(tools::APPLY_PATCH, &patch_args(), "call-1", permission_granted)
        .await
}

#[tokio::test]
async fn granted_client_permission_satisfies_prompt_policy() {
    let temp = TempDir::new().unwrap();

    let report = run_apply_patch(&temp, true).await;

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

    let report = run_apply_patch(&temp, false).await;

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
        .execute_local_tool(tools::APPLY_PATCH, &patch_args(), "call-1", true)
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
