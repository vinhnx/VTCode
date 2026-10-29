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
    spawn_session_pump(
        "session-a".into(),
        command_receiver,
        event_receiver,
        state,
        notifications,
        std::sync::Arc::new(Notify::new()),
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
