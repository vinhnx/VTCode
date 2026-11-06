use super::*;

#[tokio::test]
async fn terminal_events_release_accepted_turns_without_a_start_event() {
    use vtcode_core::exec::events::{ThreadEvent, TurnCompletedEvent, TurnFailedEvent, Usage, VersionedThreadEvent};
    for (turn_id, event, expected_status) in [
        (
            "first-turn",
            ThreadEvent::TurnFailed(TurnFailedEvent {
                completed_at: None,
                message: "turn cancelled during preparation".into(),
                usage: None,
            }),
            "failed",
        ),
        (
            "later-turn",
            ThreadEvent::TurnFailed(TurnFailedEvent {
                completed_at: None,
                message: "turn cancelled during preparation".into(),
                usage: None,
            }),
            "failed",
        ),
        (
            "dropped-start",
            ThreadEvent::TurnCompleted(TurnCompletedEvent {
                final_response_item_id: None,
                completed_at: None,
                usage: Usage::default(),
                in_progress_exec_sessions: Vec::new(),
            }),
            "completed",
        ),
    ] {
        let state = Mutex::new(ServerState {
            session_id: Some("session".into()),
            active_turn_id: Some(turn_id.into()),
            cancel_requested_turn: Some(turn_id.into()),
            ..Default::default()
        });
        let (notifications, mut received) = mpsc::channel(4);
        let finished = Notify::new();
        let waiting = finished.notified();
        tokio::pin!(waiting);
        let _already_notified = waiting.as_mut().enable();
        publish_thread_event("session", VersionedThreadEvent::new(event), &state, &notifications, &finished)
            .await
            .unwrap();
        timeout(Duration::from_secs(1), waiting).await.expect("cleanup waiter notified");
        let state = state.lock().await;
        assert!(state.active_turn_id.is_none(), "{turn_id}");
        assert!(state.cancel_requested_turn.is_none(), "{turn_id}");
        let event = received.try_recv().unwrap();
        assert_eq!(event.method, protocol::method::THREAD_EVENT);
        assert_eq!(event.params["turnId"], turn_id);
        let status = received.try_recv().unwrap();
        assert_eq!(status.params["turnId"], turn_id);
        assert_eq!(status.params["status"], expected_status);
    }
}
#[tokio::test]
async fn terminal_cleanup_wakes_when_notification_queue_is_full_or_closed() {
    use vtcode_core::exec::events::{ThreadEvent, TurnFailedEvent, VersionedThreadEvent};
    for closed in [false, true] {
        let state = Mutex::new(ServerState {
            session_id: Some("session".into()),
            active_turn_id: Some("turn".into()),
            ..Default::default()
        });
        let (notifications, mut receiver) = mpsc::channel(1);
        notifications
            .try_send(protocol::Notification {
                jsonrpc: "2.0".into(),
                method: "existing".into(),
                params: json!({}),
            })
            .unwrap();
        if closed {
            receiver.close();
        }
        let finished = Notify::new();
        let waiting = finished.notified();
        tokio::pin!(waiting);
        waiting.as_mut().enable();
        let publishing = publish_thread_event(
            "session",
            VersionedThreadEvent::new(ThreadEvent::TurnFailed(TurnFailedEvent {
                completed_at: None,
                message: "stopped".into(),
                usage: None,
            })),
            &state,
            &notifications,
            &finished,
        );
        tokio::pin!(publishing);
        let woke = tokio::select! {
            _ = &mut waiting => true,
            result = &mut publishing => {
                assert!(closed && result.is_err(), "full queue must preserve the event");
                false
            },
            _ = tokio::time::sleep(Duration::from_secs(1)) => panic!("cleanup waiter not notified"),
        };
        if !woke {
            timeout(Duration::from_secs(1), &mut waiting)
                .await
                .expect("cleanup waiter notified");
        }
        assert!(state.lock().await.active_turn_id.is_none());
        if !closed {
            assert_eq!(receiver.recv().await.unwrap().method, "existing");
            let (result, (event, status)) =
                tokio::join!(publishing, async { (receiver.recv().await.unwrap(), receiver.recv().await.unwrap()) });
            result.unwrap();
            assert_eq!(event.method, protocol::method::THREAD_EVENT);
            assert_eq!(status.method, protocol::method::TURN_STATUS);
        }
    }
}
#[tokio::test]
async fn old_session_events_do_not_reach_the_selected_session() {
    use vtcode_core::exec::events::{ThreadEvent, TurnStartedEvent, VersionedThreadEvent};
    let state = Mutex::new(ServerState {
        session_id: Some("selected-session".into()),
        active_turn_id: Some("selected-turn".into()),
        ..Default::default()
    });
    let (notifications, mut received) = mpsc::channel(2);
    publish_thread_event(
        "old-session",
        VersionedThreadEvent::new(ThreadEvent::TurnStarted(TurnStartedEvent::default())),
        &state,
        &notifications,
        &Notify::new(),
    )
    .await
    .unwrap();
    assert_eq!(state.lock().await.active_turn_id.as_deref(), Some("selected-turn"));
    assert!(received.try_recv().is_err());
}

#[tokio::test]
async fn old_runtime_pump_cannot_change_a_replacement_turn_in_the_same_session() {
    use vtcode_commons::ui_protocol::{ProgressOperation, ProgressPhase, ProgressUpdate};
    use vtcode_core::exec::events::{ThreadEvent, TurnCompletedEvent, Usage, VersionedThreadEvent};
    use vtcode_ui::tui::app::{ListOverlayRequest, TransientRequest};

    let old_generation = uuid::Uuid::new_v4();
    let current_generation = uuid::Uuid::new_v4();
    let state = std::sync::Arc::new(Mutex::new(ServerState {
        session_id: Some("session".into()),
        runtime_generation: Some(current_generation),
        active_turn_id: Some("replacement".into()),
        ..Default::default()
    }));
    let (notifications, mut received) = mpsc::channel(8);
    let finished = std::sync::Arc::new(Notify::new());
    let (commands, command_receiver) = mpsc::channel(3);
    let (events, event_receiver) = protocol::live_event_channel(2);
    let old_pump = spawn_session_pump(
        "session".into(),
        command_receiver,
        event_receiver,
        state.clone(),
        notifications.clone(),
        finished.clone(),
        Some(old_generation),
    );
    commands
        .send(InlineCommand::UpdateProgress(ProgressUpdate::Begin {
            operation: ProgressOperation::start(),
            phase: ProgressPhase::Processing,
        }))
        .await
        .unwrap();
    commands
        .send(InlineCommand::SetPrimaryAgent { name: Some("old agent".into()), color: None })
        .await
        .unwrap();
    commands
        .send(InlineCommand::ShowTransient {
            request: Box::new(TransientRequest::List(ListOverlayRequest {
                title: "old interaction".into(),
                lines: Vec::new(),
                footer_hint: None,
                items: Vec::new(),
                selected: None,
                search: None,
                hotkeys: Vec::new(),
                status: None,
                plan_markdown: None,
            })),
        })
        .await
        .unwrap();
    let completed = || {
        VersionedThreadEvent::new(ThreadEvent::TurnCompleted(TurnCompletedEvent {
            final_response_item_id: None,
            completed_at: None,
            usage: Usage::default(),
            in_progress_exec_sessions: Vec::new(),
        }))
    };
    events.publish(completed()).unwrap();
    drop(commands);
    drop(events);
    timeout(Duration::from_secs(1), old_pump).await.unwrap().unwrap();
    assert_eq!(state.lock().await.active_turn_id.as_deref(), Some("replacement"));
    assert!(state.lock().await.pending_interactions.is_empty());
    assert!(received.try_recv().is_err(), "stale progress and terminal events must be suppressed");

    let (commands, command_receiver) = mpsc::channel(1);
    let (events, event_receiver) = protocol::live_event_channel(1);
    let current_pump = spawn_session_pump(
        "session".into(),
        command_receiver,
        event_receiver,
        state.clone(),
        notifications,
        finished,
        Some(current_generation),
    );
    events.publish(completed()).unwrap();
    drop(commands);
    drop(events);
    timeout(Duration::from_secs(1), current_pump).await.unwrap().unwrap();
    assert!(state.lock().await.active_turn_id.is_none());
    assert_eq!(received.recv().await.unwrap().method, protocol::method::THREAD_EVENT);
    assert_eq!(received.recv().await.unwrap().params["turnId"], "replacement");
}
#[tokio::test]
async fn primary_agent_updates_are_bounded_sanitized_and_session_scoped() {
    let state = Mutex::new(ServerState {
        session_id: Some("selected-session".into()),
        ..Default::default()
    });
    let (notifications, mut received) = mpsc::channel(3);

    publish_primary_agent_changed("old-session", Some("Stale"), &state, &notifications)
        .await
        .expect("stale notifications are ignored");
    assert!(received.try_recv().is_err());

    publish_primary_agent_changed("selected-session", Some(" Plan \u{7}Mode "), &state, &notifications)
        .await
        .expect("current notification is published");
    let update = received.try_recv().expect("current update");
    assert_eq!(update.method, protocol::method::PRIMARY_AGENT_CHANGED);
    assert_eq!(update.params["sessionId"], "selected-session");
    assert_eq!(update.params["displayName"], "Plan Mode");

    publish_primary_agent_changed("selected-session", None, &state, &notifications)
        .await
        .expect("reset is published");
    let reset = received.try_recv().expect("reset update");
    assert!(reset.params["displayName"].is_null());

    let long_name = "a".repeat(160);
    assert_eq!(sanitize_primary_agent_display(Some(&long_name)).unwrap().len(), 128);
    assert_eq!(sanitize_primary_agent_display(Some("\u{1}\n ")), None);
}
#[tokio::test]
async fn session_pump_projects_primary_agent_host_commands() {
    let state = std::sync::Arc::new(Mutex::new(ServerState {
        session_id: Some("session-a".into()),
        ..Default::default()
    }));
    let (commands, command_receiver) = mpsc::channel(1);
    let (event_sender, event_receiver) = protocol::live_event_channel(1);
    let (notifications, mut received) = mpsc::channel(1);
    let _pump = spawn_session_pump(
        "session-a".into(),
        command_receiver,
        event_receiver,
        state,
        notifications,
        std::sync::Arc::new(Notify::new()),
        None,
    );

    commands
        .send(InlineCommand::SetPrimaryAgent { name: Some("Plan".into()), color: None })
        .await
        .expect("queue active-agent command");
    drop(commands);
    drop(event_sender);

    let update = timeout(Duration::from_secs(1), received.recv())
        .await
        .expect("active-agent notification deadline")
        .expect("active-agent notification");
    assert_eq!(update.method, protocol::method::PRIMARY_AGENT_CHANGED);
    assert_eq!(update.params["sessionId"], "session-a");
    assert_eq!(update.params["displayName"], "Plan");
}

#[tokio::test]
async fn session_pump_ignores_stale_progress_and_clears_on_finish() {
    use vtcode_commons::ui_protocol::{ProgressOperation, ProgressPhase, ProgressUpdate};
    let state = std::sync::Arc::new(Mutex::new(ServerState {
        session_id: Some("session-a".into()),
        ..Default::default()
    }));
    let (commands, command_receiver) = mpsc::channel(8);
    let (event_sender, event_receiver) = protocol::live_event_channel(1);
    let (notifications, mut received) = mpsc::channel(8);
    let _pump = spawn_session_pump(
        "session-a".into(),
        command_receiver,
        event_receiver,
        state,
        notifications,
        std::sync::Arc::new(Notify::new()),
        None,
    );
    let old = ProgressOperation::start();
    let new = ProgressOperation::start();
    for update in [
        ProgressUpdate::Begin { operation: old, phase: ProgressPhase::Processing },
        ProgressUpdate::Begin {
            operation: new,
            phase: ProgressPhase::WaitingForModel,
        },
        ProgressUpdate::Finish { operation: old },
        ProgressUpdate::Finish { operation: new },
    ] {
        commands.send(InlineCommand::UpdateProgress(update)).await.unwrap();
    }
    drop(commands);
    drop(event_sender);
    let mut values = Vec::new();
    while let Some(event) = timeout(Duration::from_secs(1), received.recv()).await.unwrap() {
        if event.method == protocol::method::UI_PROGRESS {
            values.push(event.params["active"].as_bool().unwrap());
        }
    }
    assert_eq!(values, [true, true, false]);
}

#[tokio::test]
async fn session_pump_tracks_live_background_and_blocked_states() {
    use vtcode_commons::ui_protocol::ActivityState;
    use vtcode_ui::tui::app::{LocalAgentEntry, LocalAgentKind, LocalAgentStatus};
    let state = std::sync::Arc::new(Mutex::new(ServerState {
        session_id: Some("session-b".into()),
        ..Default::default()
    }));
    let (commands, command_receiver) = mpsc::channel(8);
    let (event_sender, event_receiver) = protocol::live_event_channel(1);
    let (notifications, mut received) = mpsc::channel(8);
    let _pump = spawn_session_pump(
        "session-b".into(),
        command_receiver,
        event_receiver,
        state,
        notifications,
        std::sync::Arc::new(Notify::new()),
        None,
    );
    let entry = LocalAgentEntry {
        id: "job".into(),
        display_label: "Job".into(),
        agent_name: "Job".into(),
        color: None,
        kind: LocalAgentKind::ExecSession,
        program_status: vtcode_commons::program_status::ProgramState::Working,
        updated_at: 0,
        status: LocalAgentStatus::Running,
        summary: None,
        preview: String::new(),
        transcript_path: None,
    };
    commands
        .send(InlineCommand::SetLocalAgents { entries: vec![entry] })
        .await
        .unwrap();
    commands
        .send(InlineCommand::SetActivityState(ActivityState::Blocked))
        .await
        .unwrap();
    commands
        .send(InlineCommand::SetActivityState(ActivityState::Idle))
        .await
        .unwrap();
    commands.send(InlineCommand::SetLocalAgents { entries: vec![] }).await.unwrap();
    drop(commands);
    drop(event_sender);
    let mut values = Vec::new();
    while let Some(event) = timeout(Duration::from_secs(1), received.recv()).await.unwrap() {
        if event.method == protocol::method::UI_PROGRESS {
            values.push(event.params["active"].as_bool().unwrap());
        }
    }
    assert_eq!(values, [true, false, true, false]);
}

#[tokio::test]
async fn session_pump_drops_progress_after_session_switch() {
    use vtcode_commons::ui_protocol::{ProgressOperation, ProgressPhase, ProgressUpdate};
    let state = std::sync::Arc::new(Mutex::new(ServerState {
        session_id: Some("session-a".into()),
        ..Default::default()
    }));
    let (commands, command_receiver) = mpsc::channel(8);
    let (event_sender, event_receiver) = protocol::live_event_channel(1);
    let (notifications, mut received) = mpsc::channel(8);
    let _pump = spawn_session_pump(
        "session-a".into(),
        command_receiver,
        event_receiver,
        state.clone(),
        notifications,
        std::sync::Arc::new(Notify::new()),
        None,
    );
    commands
        .send(InlineCommand::UpdateProgress(ProgressUpdate::Begin {
            operation: ProgressOperation::start(),
            phase: ProgressPhase::Processing,
        }))
        .await
        .unwrap();
    let first = timeout(Duration::from_secs(1), received.recv()).await.unwrap().unwrap();
    assert_eq!(first.method, protocol::method::UI_PROGRESS);
    assert_eq!(first.params["active"], true);

    // The selection moves away: later updates from the stale pump, including
    // its shutdown clear, must not leak to the newly selected session.
    state.lock().await.session_id = Some("session-b".into());
    commands
        .send(InlineCommand::UpdateProgress(ProgressUpdate::Begin {
            operation: ProgressOperation::start(),
            phase: ProgressPhase::WaitingForModel,
        }))
        .await
        .unwrap();
    drop(commands);
    drop(event_sender);
    assert!(timeout(Duration::from_secs(1), received.recv()).await.unwrap().is_none());
}

#[tokio::test]
async fn session_pump_clears_live_progress_on_shutdown() {
    use vtcode_commons::ui_protocol::{ProgressOperation, ProgressPhase, ProgressUpdate};
    let state = std::sync::Arc::new(Mutex::new(ServerState {
        session_id: Some("session-c".into()),
        ..Default::default()
    }));
    let (commands, command_receiver) = mpsc::channel(2);
    let (event_sender, event_receiver) = protocol::live_event_channel(1);
    let (notifications, mut received) = mpsc::channel(4);
    let _pump = spawn_session_pump(
        "session-c".into(),
        command_receiver,
        event_receiver,
        state,
        notifications,
        std::sync::Arc::new(Notify::new()),
        None,
    );
    commands
        .send(InlineCommand::UpdateProgress(ProgressUpdate::Begin {
            operation: ProgressOperation::start(),
            phase: ProgressPhase::Processing,
        }))
        .await
        .unwrap();
    drop(commands);
    drop(event_sender);
    let first = timeout(Duration::from_secs(1), received.recv()).await.unwrap().unwrap();
    let last = timeout(Duration::from_secs(1), received.recv()).await.unwrap().unwrap();
    assert_eq!(first.params["active"], true);
    assert_eq!(last.method, protocol::method::UI_PROGRESS);
    assert_eq!(last.params["active"], false);
}

#[tokio::test]
async fn shutdown_clears_progress_after_full_queue_without_delaying_turn_cleanup() {
    use vtcode_commons::ui_protocol::{ProgressOperation, ProgressPhase, ProgressUpdate};
    let state = std::sync::Arc::new(Mutex::new(ServerState {
        session_id: Some("session-full".into()),
        active_turn_id: Some("turn".into()),
        ..Default::default()
    }));
    let (commands, command_receiver) = mpsc::channel(2);
    let (event_sender, event_receiver) = protocol::live_event_channel(1);
    let (notifications, mut received) = mpsc::channel(1);
    let finished = std::sync::Arc::new(Notify::new());
    let waiting = finished.notified();
    tokio::pin!(waiting);
    waiting.as_mut().enable();
    let _pump = spawn_session_pump(
        "session-full".into(),
        command_receiver,
        event_receiver,
        state.clone(),
        notifications,
        finished.clone(),
        None,
    );
    commands
        .send(InlineCommand::UpdateProgress(ProgressUpdate::Begin {
            operation: ProgressOperation::start(),
            phase: ProgressPhase::Processing,
        }))
        .await
        .unwrap();
    drop(commands);
    drop(event_sender);
    timeout(Duration::from_secs(1), waiting)
        .await
        .expect("turn cleanup must not wait for the full queue");
    assert!(state.lock().await.active_turn_id.is_none());
    assert_eq!(received.recv().await.unwrap().params["active"], true);
    let cleared = timeout(Duration::from_secs(1), received.recv()).await.unwrap().unwrap();
    assert_eq!(cleared.method, protocol::method::UI_PROGRESS);
    assert_eq!(cleared.params["active"], false);
}
