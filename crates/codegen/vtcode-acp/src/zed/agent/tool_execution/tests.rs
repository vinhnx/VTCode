//! End-to-end regressions for the ACP permission → tool execution hand-off.
//!
//! Covers issue #791 (a granted `session/request_permission` must satisfy the
//! local registry policy gate) and issue #792 (PTY command execution must run
//! locally so the model receives output, exit status, and a usable session id).

use super::*;
use crate::reports::{TOOL_PERMISSION_ALLOW_OPTION_ID, TOOL_PERMISSION_DENY_OPTION_ID};
use crate::zed::agent::test_support::{build_agent, build_agent_with_commands};
use agent_client_protocol::schema::v1::{
    ClientCapabilities, CreateTerminalRequest, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, SelectedPermissionOutcome, SessionNotification,
};
use agent_client_protocol::{Agent, Channel, Client, ConnectionTo, on_receive_notification, on_receive_request};
use assert_fs::TempDir;
use serde_json::json;
use tokio::sync::oneshot;
use vtcode_config::CommandsConfig;
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
///
/// `client_advertises_terminal` mirrors a client that advertises the ACP
/// `terminal` capability; command execution must not depend on it.
async fn run_tool_call(
    answer: ClientAnswer,
    tool_name: &str,
    args: Value,
    client_advertises_terminal: bool,
    commands_config: CommandsConfig,
) -> (TempDir, String) {
    let temp = TempDir::new().unwrap();
    let agent = build_agent_with_commands(temp.path(), "build", commands_config).await;
    if client_advertises_terminal {
        *agent.client_capabilities.lock().expect("client capabilities lock") =
            Some(ClientCapabilities::new().terminal(true));
    }
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
        .on_receive_request(
            // A command run must never be delegated to a client terminal: the
            // agent could only report a terminal id, with no output, no exit
            // status, and an id `write_stdin` cannot resolve.
            async move |_request: CreateTerminalRequest, responder, _connection| {
                responder.respond_with_internal_error("ACP terminal/create must not be used for exec_command")
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
    run_tool_call(answer, tools::APPLY_PATCH, json!({ "input": PATCH_ADD_FILE }), false, CommandsConfig::default())
        .await
}

/// The default command policy plus the `exit` builtin used by the issue #792
/// repro script. Every other segment of that script is already allow-listed, so
/// the only addition needed is the builtin that produces the non-zero status.
fn repro_commands_config() -> CommandsConfig {
    let mut commands = CommandsConfig::default();
    commands.allow_list.push("exit".to_string());
    commands
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

/// Issue #792: a terminal-capable client must not redirect `exec_command` to
/// `terminal/create`; the command runs locally and reports its own evidence.
#[tokio::test]
async fn terminal_capable_client_keeps_pty_commands_local() {
    let (_temp, llm_response) = run_tool_call(
        ClientAnswer::Allow,
        tools::EXEC_COMMAND,
        json!({
            "command": "sh -c 'if [ -t 1 ]; then echo TTY_YES; else echo TTY_NO; fi; echo MARKER_7319; exit 3'",
            "tty": true,
            "yield_time_ms": 10_000,
        }),
        true,
        repro_commands_config(),
    )
    .await;

    assert!(llm_response.contains("TTY_YES"), "PTY command output should reach the model: {llm_response}");
    assert!(llm_response.contains("MARKER_7319"), "PTY command output should reach the model: {llm_response}");
    assert!(
        llm_response.contains("\"backend\":\"pty\""),
        "the command must run through the local PTY backend: {llm_response}"
    );
    assert!(
        llm_response.contains("\"exit_code\":3"),
        "the command exit status should reach the model: {llm_response}"
    );
    assert!(
        llm_response.contains("\"status\":\"error\""),
        "a non-zero command exit must not be reported as success: {llm_response}"
    );
    assert!(
        llm_response.contains("\"session_id\""),
        "the response must carry a registry exec session id: {llm_response}"
    );
    assert!(
        !llm_response.contains("terminal_id"),
        "commands must not be delegated to a client terminal: {llm_response}"
    );
}

/// Issue #792: the session id returned by a still-running PTY command is a
/// registry exec session, so `write_stdin` can continue it.
#[tokio::test]
async fn pty_exec_session_accepts_write_stdin() {
    let temp = TempDir::new().unwrap();
    let agent = build_agent(temp.path()).await;

    let started = agent
        .execute_local_tool(
            tools::EXEC_COMMAND,
            &json!({
                "command": "sh -c 'read line; echo GOT:$line'",
                "tty": true,
                "yield_time_ms": 500,
            }),
            "call-1",
            true,
        )
        .await;
    let started_payload: Value = serde_json::from_str(&started.llm_response).expect("exec payload should be JSON");
    let session_id = started_payload["result"]["session_id"]
        .as_str()
        .unwrap_or_else(|| panic!("PTY command should expose a live session id: {}", started.llm_response))
        .to_string();

    let continued = agent
        .execute_local_tool(
            tools::WRITE_STDIN,
            &json!({
                "session_id": session_id,
                "chars": "hello\n",
                "yield_time_ms": 5_000,
            }),
            "call-2",
            true,
        )
        .await;

    assert!(
        continued.llm_response.contains("GOT:hello"),
        "write_stdin should reach the live PTY session: {}",
        continued.llm_response
    );
}
