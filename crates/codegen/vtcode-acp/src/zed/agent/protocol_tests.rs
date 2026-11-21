//! Protocol-level regressions for the SDK 5.x v1 entry point.
//!
//! These tests drive the real handler stack over an in-process duplex
//! connection. They pin the handshake and the custom lifecycle methods, whose
//! JSON-RPC traits changed shape in `agent-client-protocol` 5.0
//! (`parse_message` now takes a `&Value`, `JsonRpcResponse::from_value` became
//! `from_json`, and `Builder`/`Responder`/`ConnectionTo` are versioned).

use std::path::Path;
use std::sync::Arc;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::v1::ConnectionTo;
use agent_client_protocol::v1::schema::{InitializeRequest, InitializeResponse, NewSessionRequest, NewSessionResponse};
use agent_client_protocol::{Agent, Channel, Client, Error};
use assert_fs::TempDir;

use super::handlers::install_handlers;
use super::lifecycle::{SessionForkRequest, SessionForkResponse, install_lifecycle_handlers};
use super::test_support::build_agent;

type HandshakeResults = (
    InitializeResponse,
    NewSessionResponse,
    Result<SessionForkResponse, Error>,
    Result<SessionForkResponse, Error>,
);

/// Drive one agent↔client conversation and report the handshake and fork
/// results back to the test body.
async fn run_handshake(workspace: &Path) -> HandshakeResults {
    let agent = Arc::new(build_agent(workspace).await);
    let known_session = agent
        .new_session(NewSessionRequest::new(workspace.to_path_buf()))
        .await
        .expect("seed session");

    let (agent_channel, client_channel) = Channel::duplex();
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();

    // A `connect_with` runner that returns immediately closes the connection,
    // which races the first initialize request. Keep the agent side alive
    // until the client has finished its conversation.
    let agent_task =
        install_lifecycle_handlers(install_handlers(Agent.v1().name("vtcode"), Arc::clone(&agent)), Arc::clone(&agent))
            .connect_with(agent_channel, async move |_cx: ConnectionTo<Client>| {
                drop(done_rx.await);
                Ok(())
            });

    let client_task = Client.v1().connect_with(client_channel, async move |cx: ConnectionTo<Agent>| {
        let initialize = cx
            .send_request(InitializeRequest::new(ProtocolVersion::V1))
            .block_task()
            .await?;
        let created = cx
            .send_request(NewSessionRequest::new(workspace.to_path_buf()))
            .block_task()
            .await?;
        let forked = cx
            .send_request(SessionForkRequest { session_id: known_session.session_id.clone() })
            .block_task()
            .await;
        let missing = cx
            .send_request(SessionForkRequest {
                session_id: crate::acp::SessionId::new(Arc::from("vtcode-zed-session-404")),
            })
            .block_task()
            .await;
        drop(reply_tx.send((initialize, created, forked, missing)));
        assert!(done_tx.send(()).is_ok(), "agent runner should still be waiting");
        Ok(())
    });

    let (agent_result, client_result) = tokio::join!(agent_task, client_task);
    agent_result.expect("agent duplex connection should complete");
    client_result.expect("client duplex connection should complete");
    reply_rx.await.expect("client should report handshake results")
}

#[tokio::test]
async fn sdk_v1_handshake_negotiates_v1_and_routes_lifecycle_extensions() {
    let temp = TempDir::new().expect("workspace");
    let (initialize, created, forked, missing) = run_handshake(temp.path()).await;

    assert_eq!(initialize.protocol_version, ProtocolVersion::V1);
    assert!(initialize.agent_capabilities.load_session, "load_session must be advertised");
    assert!(
        initialize.agent_capabilities.session_capabilities.list.is_some(),
        "session/list capability must be advertised"
    );
    assert!(!initialize.auth_methods.is_empty(), "at least one auth method must be advertised");
    assert_eq!(initialize.agent_info.as_ref().map(|info| info.name.as_ref()), Some("vtcode"));

    assert!(!created.session_id.0.is_empty(), "session/new must allocate an id");
    let forked = forked.expect("session/fork should route through the extension handler");
    assert_ne!(forked.session_id, created.session_id, "fork must allocate a new session id");

    let missing = missing.expect_err("unknown sessions must fail closed");
    assert_eq!(
        missing
            .data
            .as_ref()
            .and_then(|data| data.get("reason"))
            .and_then(|value| value.as_str()),
        Some("unknown_session")
    );
}
