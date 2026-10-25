//! End-to-end regression for the ACP permission → tool execution hand-off.
//!
//! Covers issue #791: a granted `session/request_permission` must satisfy the
//! local registry policy gate before the tool runs.

use super::*;
use crate::reports::{TOOL_PERMISSION_ALLOW_OPTION_ID, TOOL_PERMISSION_DENY_OPTION_ID};
use crate::zed::agent::test_support::build_agent;
use agent_client_protocol::schema::v1::{
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse, SelectedPermissionOutcome,
    SessionNotification,
};
use agent_client_protocol::{Agent, Channel, Client, ConnectionTo, on_receive_notification, on_receive_request};
use assert_fs::TempDir;
use serde_json::json;
use tokio::sync::oneshot;
use vtcode_core::config::constants::tools;
use vtcode_core::llm::provider::ToolCall as ProviderToolCall;

const PATCH_ADD_FILE: &str = "*** Begin Patch\n*** Add File: hello.txt\n+hello\n*** End Patch\n";

#[derive(Clone, Copy)]
enum ClientAnswer {
    Allow,
    Deny,
}

impl ClientAnswer {
    fn option_id(self) -> &'static str {
        match self {
            Self::Allow => TOOL_PERMISSION_ALLOW_OPTION_ID,
            Self::Deny => TOOL_PERMISSION_DENY_OPTION_ID,
        }
    }
}

/// Run one tool call through the ACP loop, answering the
/// `session/request_permission` request with `answer`.
async fn run_tool_call(answer: ClientAnswer, tool_name: &str, args: Value) -> (TempDir, String) {
    let temp = TempDir::new().unwrap();
    let agent = build_agent(temp.path()).await;
    let session_id = agent.register_session();
    let session = agent.session_handle(&session_id).expect("registered session");
    let (agent_channel, client_channel) = Channel::duplex();
    let (result_tx, result_rx) = oneshot::channel();

    let agent_task = Agent
        .builder()
        .connect_with(agent_channel, async move |cx: ConnectionTo<Client>| {
            agent.attach_client(ConnectionHandle::new(cx));
            let call = ProviderToolCall::function("call-1".to_string(), tool_name.to_string(), args.to_string());
            drop(result_tx.send(agent.execute_tool_calls(&session, &session_id, &[call]).await));
            Ok(())
        });

    let client_task = Client
        .builder()
        .on_receive_request(
            async move |_request: RequestPermissionRequest, responder, _connection| {
                responder.respond(RequestPermissionResponse::new(RequestPermissionOutcome::Selected(
                    SelectedPermissionOutcome::new(answer.option_id()),
                )))
            },
            on_receive_request!(),
        )
        .on_receive_notification(
            async move |_notification: SessionNotification, _connection: ConnectionTo<Agent>| Ok::<(), acp::Error>(()),
            on_receive_notification!(),
        )
        .connect_to(client_channel);

    let (agent_result, client_result) = tokio::join!(agent_task, client_task);
    agent_result.expect("agent duplex connection should complete");
    client_result.expect("client duplex connection should complete");

    let results = result_rx
        .await
        .expect("agent should report tool results")
        .expect("tool call loop should not fail");
    assert_eq!(results.len(), 1);
    (temp, results.into_iter().next().expect("one tool result").llm_response)
}

async fn run_apply_patch_call(answer: ClientAnswer) -> (TempDir, String) {
    run_tool_call(answer, tools::APPLY_PATCH, json!({ "input": PATCH_ADD_FILE })).await
}

#[tokio::test]
async fn granted_client_permission_runs_prompt_policy_tool() {
    let (temp, llm_response) = run_apply_patch_call(ClientAnswer::Allow).await;

    assert!(
        llm_response.contains("\"status\":\"success\""),
        "granted apply_patch should succeed: {llm_response}"
    );
    assert_eq!(
        std::fs::read_to_string(temp.path().join("hello.txt")).unwrap(),
        "hello\n",
        "granted apply_patch should create the file"
    );
}

#[tokio::test]
async fn rejected_client_permission_keeps_prompt_policy_tool_unrun() {
    let (temp, llm_response) = run_apply_patch_call(ClientAnswer::Deny).await;

    assert!(
        llm_response.contains("denied by the user"),
        "rejected apply_patch should report the client decision: {llm_response}"
    );
    assert!(!temp.path().join("hello.txt").exists(), "rejected apply_patch must not write files");
}
