use super::*;

use tempfile::TempDir;
use vtcode_commons::message_metadata::MessageMetadata;
use vtcode_core::compaction::memory_envelope::{SessionMemoryEnvelope, default_memory_envelope_path_for_session};
use vtcode_core::core::agent::session::AgentSessionState;
use vtcode_core::core::agent::steering::{FollowUpProvenance, QueuedFollowUpIntent};
use vtcode_core::llm::provider::Message;

struct ResumedInputCase {
    metadata: MessageMetadata,
    expected_id: &'static str,
    expected_provenance: FollowUpProvenance,
}

struct TrackerResumeCase {
    adopted: bool,
    stopped: bool,
    expected_queued: bool,
}

fn runtime(messages: Vec<Message>) -> AgentRuntime {
    let mut state = AgentSessionState::new("restore-test".to_string(), 10, 10, 4096);
    state.messages = messages.into();
    AgentRuntime::new(state, None, None)
}

#[tokio::test]
async fn fresh_session_preserves_trailing_user_input_without_queuing_it() {
    let temp = TempDir::new().unwrap();
    let registry = ToolRegistry::new_for_schema(temp.path().to_path_buf()).await;
    let messages = vec![
        Message::system("initial context".into()),
        Message::user("fresh request".into()),
    ];
    let mut runtime = runtime(messages.clone());
    let ctrl_c_state = CtrlCState::new();
    let mut renderer = AnsiRenderer::stdout();

    restore_session_follow_ups(
        &mut runtime,
        &registry,
        FollowUpRestoreContext {
            workspace_root: temp.path(),
            vt_cfg: None,
            resumed: false,
            ctrl_c_state: &ctrl_c_state,
            renderer: &mut renderer,
        },
    )
    .await;

    assert_eq!(runtime.state.messages.as_ref(), &messages);
    assert!(!runtime.has_pending_follow_up_inputs());
}

#[tokio::test]
async fn resumed_input_preserves_user_and_synthetic_identity_and_provenance() {
    let temp = TempDir::new().unwrap();
    let registry = ToolRegistry::new_for_schema(temp.path().to_path_buf()).await;
    let ctrl_c_state = CtrlCState::new();
    let mut renderer = AnsiRenderer::stdout();
    for ResumedInputCase { metadata, expected_id, expected_provenance } in [
        ResumedInputCase {
            metadata: MessageMetadata::user_input(1, 6).with_intent_id("resumed-user"),
            expected_id: "resumed-user",
            expected_provenance: FollowUpProvenance::UserInput,
        },
        ResumedInputCase {
            metadata: MessageMetadata::harness_notice(1, 6).with_intent_id("resumed-synthetic"),
            expected_id: "resumed-synthetic",
            expected_provenance: FollowUpProvenance::Synthetic,
        },
    ] {
        let mut runtime = runtime(vec![
            Message::assistant("earlier result".into()),
            Message::user("interrupted request".into()).with_metadata(metadata),
            Message::system("resume note".into()),
        ]);

        restore_session_follow_ups(
            &mut runtime,
            &registry,
            FollowUpRestoreContext {
                workspace_root: temp.path(),
                vt_cfg: None,
                resumed: true,
                ctrl_c_state: &ctrl_c_state,
                renderer: &mut renderer,
            },
        )
        .await;

        assert_eq!(runtime.state.messages.len(), 2);
        assert_eq!(runtime.state.messages[0].content.as_text(), "earlier result");
        assert_eq!(runtime.state.messages[1].content.as_text(), "resume note");
        let pending = runtime.split_mut().1.pending_follow_up_intents_snapshot();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id(), expected_id);
        assert_eq!(pending[0].text(), "interrupted request");
        assert_eq!(pending[0].provenance(), expected_provenance);
    }
}

#[tokio::test]
async fn durable_intents_precede_interrupted_input_and_applied_ids_are_not_replayed() {
    let temp = TempDir::new().unwrap();
    let registry = ToolRegistry::new_for_schema(temp.path().to_path_buf()).await;
    registry.set_harness_session("durable-restore");
    let envelope = SessionMemoryEnvelope {
        session_id: "durable-restore".into(),
        pending_intents: vec![
            QueuedFollowUpIntent::from_parts_with_provenance(
                "already-applied",
                "must not repeat",
                FollowUpProvenance::UserInput,
            ),
            QueuedFollowUpIntent::from_parts_with_provenance(
                "pending-synthetic",
                "saved recovery",
                FollowUpProvenance::Synthetic,
            ),
        ],
        applied_intent_ids: vec!["already-applied".into()],
        ..Default::default()
    };
    let path = default_memory_envelope_path_for_session(temp.path(), "durable-restore");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, serde_json::to_vec(&envelope).unwrap()).unwrap();
    let mut runtime = runtime(vec![
        Message::user("latest user input".into())
            .with_metadata(MessageMetadata::user_input(2, 5).with_intent_id("interrupted-user")),
    ]);
    let ctrl_c_state = CtrlCState::new();
    let mut renderer = AnsiRenderer::stdout();

    restore_session_follow_ups(
        &mut runtime,
        &registry,
        FollowUpRestoreContext {
            workspace_root: temp.path(),
            vt_cfg: None,
            resumed: true,
            ctrl_c_state: &ctrl_c_state,
            renderer: &mut renderer,
        },
    )
    .await;

    let pending = runtime.split_mut().1.pending_follow_up_intents_snapshot();
    assert_eq!(
        pending.iter().map(|intent| intent.id()).collect::<Vec<_>>(),
        ["pending-synthetic", "interrupted-user"]
    );
    assert_eq!(pending[0].text(), "saved recovery");
    assert_eq!(pending[0].provenance(), FollowUpProvenance::Synthetic);
    assert_eq!(pending[1].text(), "latest user input");
    assert_eq!(pending[1].provenance(), FollowUpProvenance::UserInput);
    assert!(runtime.state.messages.is_empty());
    assert!(runtime.applied_follow_up_intent_ids().iter().any(|id| id == "already-applied"));
}

#[tokio::test]
async fn tracker_resume_requires_adoption_and_no_stop_request() {
    let temp = TempDir::new().unwrap();
    let registry = ToolRegistry::new_for_schema(temp.path().to_path_buf()).await;
    registry
        .get_tool("task_tracker")
        .unwrap()
        .execute(serde_json::json!({
            "action": "create", "title": "resume boundary", "items": ["unfinished first step", "unfinished second step"]
        }))
        .await
        .unwrap();
    let mut renderer = AnsiRenderer::stdout();

    for TrackerResumeCase { adopted, stopped, expected_queued } in [
        TrackerResumeCase {
            adopted: false,
            stopped: false,
            expected_queued: false,
        },
        TrackerResumeCase {
            adopted: true,
            stopped: true,
            expected_queued: false,
        },
        TrackerResumeCase {
            adopted: true,
            stopped: false,
            expected_queued: true,
        },
    ] {
        registry.begin_tracker_request(adopted);
        let ctrl_c_state = CtrlCState::new();
        if stopped {
            ctrl_c_state.request_exit();
        }
        let messages = vec![
            Message::user("earlier request".into()),
            Message::assistant("earlier result".into()),
        ];
        let mut runtime = runtime(messages.clone());

        restore_session_follow_ups(
            &mut runtime,
            &registry,
            FollowUpRestoreContext {
                workspace_root: temp.path(),
                vt_cfg: None,
                resumed: true,
                ctrl_c_state: &ctrl_c_state,
                renderer: &mut renderer,
            },
        )
        .await;

        let pending = runtime.split_mut().1.pending_follow_up_intents_snapshot();
        assert_eq!(pending.len(), usize::from(expected_queued), "adopted={adopted}, stopped={stopped}");
        if expected_queued {
            assert_eq!(pending[0].provenance(), FollowUpProvenance::Synthetic);
            assert!(pending[0].text().contains("unfinished first step"));
            assert!(pending[0].text().contains("unfinished second step"));
            assert_eq!(runtime.state.messages.len(), messages.len() + 1);
        } else {
            assert_eq!(runtime.state.messages.as_ref(), &messages);
        }
    }
}
