use super::*;
use crate::zed::helpers::PrimaryAgentCatalog;
use assert_fs::TempDir;
use chrono::Utc;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use vtcode_config::{SubagentDiscoveryInput, discover_subagents};
use vtcode_core::core::agent::snapshots::{DEFAULT_CHECKPOINTS_ENABLED, DEFAULT_MAX_AGE_DAYS, DEFAULT_MAX_SNAPSHOTS};
use vtcode_core::utils::session_archive::{SessionArchiveMetadata, SessionListing, SessionSnapshot};

use super::super::handlers::build_session_provider;
use super::super::test_support::{build_agent, build_agent_with_default_primary_agent, build_agent_with_policies};

fn primary_agent(session: &SessionHandle) -> String {
    session.data.lock().map(|data| data.primary_agent.clone()).unwrap_or_default()
}

fn reasoning_effort(session: &SessionHandle) -> ReasoningEffortLevel {
    session
        .data
        .lock()
        .map(|data| data.reasoning_effort)
        .unwrap_or(ReasoningEffortLevel::Low)
}

fn provider(session: &SessionHandle) -> String {
    session.data.lock().map(|data| data.provider.clone()).unwrap_or_default()
}

fn model(session: &SessionHandle) -> String {
    session.data.lock().map(|data| data.model.clone()).unwrap_or_default()
}

#[tokio::test]
async fn build_session_handle_restores_session_config_from_thread_metadata() {
    let temp = TempDir::new().unwrap();
    let agent = build_agent(temp.path()).await;
    let listing = SessionListing {
        path: temp.path().join("session-vtcode-acp-archive.json"),
        snapshot: SessionSnapshot {
            metadata: SessionArchiveMetadata::new(
                "vtcode",
                temp.path().to_string_lossy(),
                "gpt-5.6-sol",
                "openai",
                "test",
                "xhigh",
            )
            .with_primary_agent("build"),
            started_at: Utc::now(),
            ended_at: Utc::now(),
            total_messages: 0,
            distinct_tools: Vec::new(),
            transcript: Vec::new(),
            messages: Vec::new(),
            progress: None,
            error_logs: Vec::new(),
        },
    };
    let thread = agent
        .thread_manager
        .start_thread_with_identifier("session-vtcode-acp-archive", ThreadBootstrap::from_listing(listing));

    let handle = agent.build_session_handle(acp::SessionId::new("session-1"), thread);

    assert_eq!(primary_agent(&handle), "build");
    assert_eq!(reasoning_effort(&handle), ReasoningEffortLevel::XHigh);
    assert_eq!(provider(&handle), "openai");
    assert_eq!(model(&handle), "gpt-5.6-sol");
}

#[tokio::test]
async fn build_session_handle_falls_back_for_unknown_metadata_primary_agent() {
    let temp = TempDir::new().unwrap();
    let agent = build_agent(temp.path()).await;
    let listing = SessionListing {
        path: temp.path().join("session-vtcode-acp-archive.json"),
        snapshot: SessionSnapshot {
            metadata: SessionArchiveMetadata::new(
                "vtcode",
                temp.path().to_string_lossy(),
                "gpt-5.6-sol",
                "openai",
                "test",
                "low",
            )
            .with_primary_agent("missing"),
            started_at: Utc::now(),
            ended_at: Utc::now(),
            total_messages: 0,
            distinct_tools: Vec::new(),
            transcript: Vec::new(),
            messages: Vec::new(),
            progress: None,
            error_logs: Vec::new(),
        },
    };
    let thread = agent
        .thread_manager
        .start_thread_with_identifier("session-vtcode-acp-archive", ThreadBootstrap::from_listing(listing));

    let handle = agent.build_session_handle(acp::SessionId::new("session-1"), thread);

    assert_eq!(primary_agent(&handle), "build");
}

#[tokio::test]
async fn register_session_falls_back_for_unknown_default_primary_agent() {
    let temp = TempDir::new().unwrap();
    let agent = build_agent_with_default_primary_agent(temp.path(), "research").await;
    let session_id = agent.register_session();
    let session = agent.session_handle(&session_id).unwrap();

    // "research" is not in the discovered specs, so the resolver falls
    // back to the built-in "build" agent.
    assert_eq!(primary_agent(&session), "build");
}

#[tokio::test]
async fn register_session_uses_known_default_primary_agent_ids() {
    for primary_agent in ["duck", "plan", "build", "auto"] {
        let temp = TempDir::new().unwrap();
        let agent = build_agent_with_default_primary_agent(temp.path(), primary_agent).await;
        let session_id = agent.register_session();
        let session = agent.session_handle(&session_id).unwrap();

        assert_eq!(self::primary_agent(&session), primary_agent);
    }
}

#[tokio::test]
async fn register_session_uses_custom_default_primary_agent() {
    let temp = TempDir::new().unwrap();
    fs::create_dir_all(temp.path().join(".vtcode/agents")).unwrap();
    fs::write(
        temp.path().join(".vtcode/agents/research.md"),
        r#"---
name: research
description: Research primary
mode: primary
permissions:
  default: deny
---
Research primary prompt."#,
    )
    .unwrap();

    let agent = build_agent_with_default_primary_agent(temp.path(), "research").await;
    let session_id = agent.register_session();
    let session = agent.session_handle(&session_id).unwrap();

    assert_eq!(primary_agent(&session), "research");
}

#[tokio::test]
async fn update_session_primary_agent_updates_session_data() {
    let temp = TempDir::new().unwrap();
    let agent = build_agent(temp.path()).await;
    let session_id = agent.register_session();
    let session = agent.session_handle(&session_id).unwrap();

    // `build_agent` already defaults to "build", so switch to a different
    // known agent to exercise a real transition (an unchanged id is a no-op).
    assert!(agent.update_session_primary_agent(&session, "plan".to_string()));
    assert_eq!(primary_agent(&session), "plan");
    assert_eq!(
        session
            .data
            .lock()
            .ok()
            .and_then(|data| data.thread.metadata())
            .and_then(|metadata| metadata.primary_agent)
            .as_deref(),
        Some("plan")
    );

    // Re-applying the same id is a no-op.
    assert!(!agent.update_session_primary_agent(&session, "plan".to_string()));
}

#[tokio::test]
async fn update_session_reasoning_effort_syncs_thread_metadata() {
    let temp = TempDir::new().unwrap();
    let agent = build_agent(temp.path()).await;
    let session_id = agent.register_session();
    let session = agent.session_handle(&session_id).unwrap();

    assert!(agent.update_session_reasoning_effort(&session, ReasoningEffortLevel::High));
    assert_eq!(
        session
            .data
            .lock()
            .ok()
            .and_then(|data| data.thread.metadata())
            .map(|metadata| metadata.reasoning_effort)
            .as_deref(),
        Some("high")
    );
}

#[tokio::test]
async fn current_session_config_options_omit_thought_level_when_model_lacks_support() {
    let temp = TempDir::new().unwrap();
    let mut agent = build_agent(temp.path()).await;
    agent.config.model = "claude-haiku-3".to_string();
    agent.config.provider = "anthropic".to_string();
    let session_id = agent.register_session();
    let session = agent.session_handle(&session_id).unwrap();

    let config_options = agent.current_session_config_options(&session);

    assert_eq!(config_options.len(), 3);
    assert_eq!(config_options[0].id, acp::SessionConfigId::new("primary_agent"));
}

#[tokio::test]
async fn list_sessions_returns_registered_sessions_and_filters_cwd() {
    let temp = TempDir::new().unwrap();
    let agent = build_agent(temp.path()).await;
    let first = agent.register_session();
    let second = agent.register_session();

    let all = agent.list_sessions(acp::ListSessionsRequest::new()).await.unwrap();
    assert_eq!(all.sessions.len(), 2);
    assert!(all.sessions.iter().any(|info| info.session_id == first));
    assert!(all.sessions.iter().any(|info| info.session_id == second));
    assert!(
        all.sessions.iter().all(|info| info.cwd == temp.path().to_path_buf()),
        "single-workspace bridge reports workspace cwd"
    );

    let matching = agent
        .list_sessions(acp::ListSessionsRequest::new().cwd(temp.path().to_path_buf()))
        .await
        .unwrap();
    assert_eq!(matching.sessions.len(), 2);

    let other_cwd = temp.path().join("other-workspace");
    assert_ne!(other_cwd, temp.path().to_path_buf());
    let other = agent
        .list_sessions(acp::ListSessionsRequest::new().cwd(other_cwd))
        .await
        .unwrap();
    assert!(other.sessions.is_empty());
}

#[tokio::test]
async fn resume_existing_session_returns_config_options() {
    let temp = TempDir::new().unwrap();
    let agent = build_agent(temp.path()).await;
    let session_id = agent.register_session();

    let response = agent
        .resume_session(acp::ResumeSessionRequest::new(session_id.clone(), temp.path()))
        .await
        .unwrap();
    assert!(!response.config_options.unwrap_or_default().is_empty());
    assert!(agent.session_handle(&session_id).is_some());
}

#[tokio::test]
async fn resume_unknown_session_fails_closed() {
    let temp = TempDir::new().unwrap();
    let agent = build_agent(temp.path()).await;

    let result = agent
        .resume_session(acp::ResumeSessionRequest::new(acp::SessionId::new("vtcode-zed-session-missing"), temp.path()))
        .await;
    let error = format!("{:?}", result.expect_err("unknown session must fail"));
    assert!(error.contains("unknown_session"));
}

#[tokio::test]
async fn close_session_removes_handle_and_second_close_fails() {
    let temp = TempDir::new().unwrap();
    let agent = build_agent(temp.path()).await;
    let session_id = agent.register_session();

    drop(
        agent
            .close_session(acp::CloseSessionRequest::new(session_id.clone()))
            .await
            .unwrap(),
    );
    assert!(agent.session_handle(&session_id).is_none());

    let listed = agent.list_sessions(acp::ListSessionsRequest::new()).await.unwrap();
    assert!(!listed.sessions.iter().any(|info| info.session_id == session_id));

    let again = agent.close_session(acp::CloseSessionRequest::new(session_id)).await;
    let error = format!("{:?}", again.expect_err("second close must fail"));
    assert!(error.contains("unknown_session"));
}

#[tokio::test]
async fn delete_session_clears_live_handle_but_list_stays_consistent() {
    let temp = TempDir::new().unwrap();
    let agent = build_agent(temp.path()).await;
    let keep = agent.register_session();
    let remove = agent.register_session();

    drop(
        agent
            .delete_session(acp::DeleteSessionRequest::new(remove.clone()))
            .await
            .unwrap(),
    );
    assert!(agent.session_handle(&remove).is_none());
    assert!(agent.session_handle(&keep).is_some());

    let listed = agent.list_sessions(acp::ListSessionsRequest::new()).await.unwrap();
    assert!(listed.sessions.iter().any(|info| info.session_id == keep));
    assert!(!listed.sessions.iter().any(|info| info.session_id == remove));
}

#[tokio::test]
async fn new_session_rejects_relative_cwd() {
    let temp = TempDir::new().unwrap();
    let agent = build_agent(temp.path()).await;

    let result = agent.new_session(acp::NewSessionRequest::new("relative/path")).await;
    let error = format!("{:?}", result.expect_err("relative cwd must fail"));
    assert!(error.contains("invalid_cwd"));
}

#[tokio::test]
async fn resume_rejects_relative_cwd() {
    let temp = TempDir::new().unwrap();
    let agent = build_agent(temp.path()).await;
    let session_id = agent.register_session();

    let result = agent
        .resume_session(acp::ResumeSessionRequest::new(session_id, "relative/path"))
        .await;
    let error = format!("{:?}", result.expect_err("relative cwd must fail"));
    assert!(error.contains("invalid_cwd"));
}

#[tokio::test]
async fn resume_after_close_fails_closed() {
    let temp = TempDir::new().unwrap();
    let agent = build_agent(temp.path()).await;
    let session_id = agent.register_session();

    drop(
        agent
            .close_session(acp::CloseSessionRequest::new(session_id.clone()))
            .await
            .unwrap(),
    );
    let result = agent
        .resume_session(acp::ResumeSessionRequest::new(session_id, temp.path()))
        .await;
    let error = format!("{:?}", result.expect_err("resume after close must fail"));
    assert!(error.contains("unknown_session"));
}

#[tokio::test]
async fn delete_unknown_session_fails_closed() {
    let temp = TempDir::new().unwrap();
    let agent = build_agent(temp.path()).await;

    let result = agent
        .delete_session(acp::DeleteSessionRequest::new(acp::SessionId::new("vtcode-zed-session-missing")))
        .await;
    let error = format!("{:?}", result.expect_err("unknown delete must fail"));
    assert!(error.contains("unknown_session"));
}

#[tokio::test]
async fn delete_after_delete_fails_closed() {
    let temp = TempDir::new().unwrap();
    let agent = build_agent(temp.path()).await;
    let session_id = agent.register_session();

    drop(
        agent
            .delete_session(acp::DeleteSessionRequest::new(session_id.clone()))
            .await
            .unwrap(),
    );
    let again = agent.delete_session(acp::DeleteSessionRequest::new(session_id)).await;
    let error = format!("{:?}", again.expect_err("second delete must fail"));
    assert!(error.contains("unknown_session"));
}

#[tokio::test]
async fn provider_whitelist_blocks_excluded_selection_and_construction() {
    let temp = TempDir::new().unwrap();
    let agent = build_agent_with_policies(
        temp.path(),
        "build",
        vtcode_config::CommandsConfig::default(),
        vtcode_config::SandboxConfig::default(),
        vtcode_config::core::permissions::PermissionsConfig::default(),
        vec!["openai".to_string()],
    )
    .await;

    assert!(agent.supports_provider("openai", "openai"));
    assert!(!agent.supports_provider("anthropic", "openai"));
    assert!(
        agent
            .provider_select_options("openai")
            .iter()
            .all(|option| option.value.0.as_ref() != "anthropic"),
        "excluded providers must not be advertised"
    );
    let error = format!(
        "{:?}",
        build_session_provider(&agent, "anthropic", "claude-x")
            .err()
            .expect("excluded provider must fail")
    );
    assert!(error.contains("providers_whitelist"), "{error}");
}
