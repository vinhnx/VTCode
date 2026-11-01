//! Executor tests for cross-process peer-session messaging.

use super::*;
use serde_json::json;

type TestResult = Result<()>;

fn listed_session_ids(listed: &Value) -> Vec<&str> {
    listed["sessions"]
        .as_array()
        .map(|sessions| {
            sessions
                .iter()
                .filter_map(|s| s.get("session_id").and_then(Value::as_str))
                .collect()
        })
        .unwrap_or_default()
}

/// A registry bound to `session` over a shared workspace tempdir, with a live
/// event log held open so the session counts as live to peers.
struct LivePeer {
    registry: ToolRegistry,
    _log: vtcode_memory::SessionEventLog,
}

async fn live_peer(workspace: &std::path::Path, session: &str) -> Result<LivePeer> {
    let log = vtcode_memory::open(workspace, session, vtcode_memory::DEFAULT_MAX_EVENTS)?;
    let registry = ToolRegistry::new(workspace.to_path_buf()).await;
    registry.set_harness_session(session);
    Ok(LivePeer { registry, _log: log })
}

#[tokio::test]
async fn ask_answer_round_trip_routes_reply_to_asker() -> TestResult {
    let dir = tempfile::tempdir()?;
    let peer_a = live_peer(dir.path(), "sess-a").await?;
    let peer_b = live_peer(dir.path(), "sess-b").await?;

    let asked = peer_a
        .registry
        .ask_session_executor(json!({"session": "sess-b", "question": "should we retry the cache?"}))
        .await?;
    let question_id = asked.get("message_id").and_then(Value::as_str).unwrap_or_default().to_owned();
    assert!(!question_id.is_empty(), "ask returns a message id: {asked}");
    assert_eq!(asked.get("to_session").and_then(Value::as_str), Some("sess-b"));

    // B's next turn drains and acks the question before the model replies,
    // so the question already sits in the read archive when answer runs.
    let drain = vtcode_memory::drain_inbox(dir.path(), "sess-b")?;
    assert_eq!(drain.live.len(), 1);
    assert!(vtcode_memory::ack_mail(dir.path(), "sess-b", &question_id)?);

    // B answers naming only the question id; the executor resolves sess-a
    // as the reply target from B's own inbox/archive.
    let answered = peer_b
        .registry
        .answer_session_executor(json!({"message_id": question_id, "answer": "retry with backoff"}))
        .await?;
    assert_eq!(answered.get("to_session").and_then(Value::as_str), Some("sess-a"));

    let drain = vtcode_memory::drain_inbox(dir.path(), "sess-a")?;
    assert_eq!(drain.live.len(), 1, "one live answer for A");
    assert_eq!(drain.live[0].kind, vtcode_memory::MailKind::Answer);
    assert_eq!(drain.live[0].from_session, "sess-b");
    assert_eq!(drain.live[0].in_reply_to.as_deref(), Some(question_id.as_str()));
    Ok(())
}

#[tokio::test]
async fn ask_rejects_self_dead_and_unknown_targets() -> TestResult {
    let dir = tempfile::tempdir()?;
    let peer = live_peer(dir.path(), "sess-a").await?;

    let err = peer
        .registry
        .ask_session_executor(json!({"session": "sess-a", "question": "talking to myself"}))
        .await
        .map(|_| ())
        .expect_err("self-send must fail");
    assert!(err.to_string().contains("itself"), "got: {err}");

    // A session directory that exists but holds no live process refuses mail,
    // exactly like a session that never existed.
    drop(vtcode_memory::open(dir.path(), "dead-peer", vtcode_memory::DEFAULT_MAX_EVENTS)?);
    for target in ["dead-peer", "ghost"] {
        let err = peer
            .registry
            .ask_session_executor(json!({"session": target, "question": "ping"}))
            .await
            .map(|_| ())
            .expect_err("non-live target must fail");
        assert!(format!("{err:#}").contains("is not live"), "target {target}, got: {err:#}");
    }
    Ok(())
}

#[tokio::test]
async fn answer_unknown_message_fails_closed() -> TestResult {
    let dir = tempfile::tempdir()?;
    let peer = live_peer(dir.path(), "sess-a").await?;

    let err = peer
        .registry
        .answer_session_executor(json!({"message_id": "00000000-0000-0000-0000-000000000000", "answer": "stale"}))
        .await
        .map(|_| ())
        .expect_err("unknown message must fail");
    assert!(err.to_string().contains("unknown question"), "got: {err}");
    Ok(())
}

#[tokio::test]
async fn list_sessions_filters_self_unless_requested() -> TestResult {
    let dir = tempfile::tempdir()?;
    let peer_a = live_peer(dir.path(), "sess-a").await?;
    let _peer_b = live_peer(dir.path(), "sess-b").await?;

    // The list tool needs manifests, not just live locks: record a turn each.
    for session in ["sess-a", "sess-b"] {
        let log = vtcode_memory::open(dir.path(), session, vtcode_memory::DEFAULT_MAX_EVENTS)?;
        log.append(&crate::exec::events::ThreadEvent::TurnStarted(Default::default()))?;
        log.append(&crate::exec::events::ThreadEvent::TurnCompleted(crate::exec::events::TurnCompletedEvent {
            completed_at: None,
            usage: crate::exec::events::Usage::default(),
            in_progress_exec_sessions: Vec::new(),
        }))?;
    }

    let listed = peer_a.registry.list_sessions_executor(json!({})).await?;
    let ids = listed_session_ids(&listed);
    assert_eq!(ids, vec!["sess-b"], "self excluded by default, got {ids:?}");

    let listed = peer_a.registry.list_sessions_executor(json!({"include_self": true})).await?;
    let mut ids = listed_session_ids(&listed);
    ids.sort_unstable();
    assert_eq!(ids, vec!["sess-a", "sess-b"], "self included on request, got {ids:?}");
    Ok(())
}
