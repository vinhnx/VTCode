use super::*;
use crate::agent::runloop::unified::state::CtrlCState;
use serial_test::serial;
use vtcode_core::llm::provider::{Message, MessageRole};
use vtcode_core::utils::session_archive::{SessionMessage, SessionSnapshot};
use vtcode_ui::tui::app::SubmittedInput;

fn test_waits() -> EscalationWaits {
    EscalationWaits {
        grace: Duration::from_millis(50),
        turn_wait: Duration::from_millis(200),
        exit_wait: Duration::from_millis(200),
    }
}

/// A fake runtime with a running turn that only moves when the test drives its
/// input and exit channels, so stalled cancellation is deterministic.
struct WatchdogHarness {
    context: ServiceContext,
    _workspace: tempfile::TempDir,
    notifications: mpsc::Receiver<protocol::Notification>,
    input: Option<mpsc::UnboundedReceiver<InlineEvent>>,
    input_sender: mpsc::UnboundedSender<InlineEvent>,
    exit_sender: Option<oneshot::Sender<std::result::Result<(), String>>>,
    runtime_task: tokio::task::JoinHandle<()>,
}

impl WatchdogHarness {
    async fn install(session_id: &str, turn_id: &str) -> Self {
        let workspace = hermetic_workspace();
        let (context, notifications) =
            test_service_context(test_startup_context(workspace.path()), workspace.path().to_path_buf());
        let (input_sender, input) = mpsc::unbounded_channel();
        let (exit_sender, exit) = oneshot::channel();
        let runtime_task = tokio::spawn(std::future::pending::<()>());
        let stop = stop_requests::new_stop_handle_slot();
        stop_requests::publish_stop_handles(
            &stop,
            &std::sync::Arc::new(CtrlCState::new()),
            &std::sync::Arc::new(Notify::new()),
        );
        stop_requests::mark_host_turns_ready(&stop);
        {
            let mut state = context.state.lock().await;
            state.initialized = true;
            state.session_id = Some(session_id.into());
            state.active_turn_id = Some(turn_id.into());
            state.escalation_waits = Some(test_waits());
            state.runtime = Some(RuntimeSession {
                session_id: session_id.into(),
                input: input_sender.clone(),
                stop,
                exit,
                abort: runtime_task.abort_handle(),
                command_forwarder: std::thread::spawn(|| {}),
            });
        }
        Self {
            context,
            _workspace: workspace,
            notifications,
            input: Some(input),
            input_sender,
            exit_sender: Some(exit_sender),
            runtime_task,
        }
    }

    async fn cancel_turn(&self, session_id: &str, turn_id: &str) {
        let request = Request {
            jsonrpc: "2.0".into(),
            id: RequestId::Number(1),
            method: protocol::method::TURN_CANCEL.into(),
            params: json!({ "sessionId": session_id, "turnId": turn_id }),
        };
        handle_request(&self.context, &request).await.expect("cancel accepted");
    }

    async fn force_cancel_turn(&self, session_id: &str, turn_id: &str) {
        let request = Request {
            jsonrpc: "2.0".into(),
            id: RequestId::Number(2),
            method: protocol::method::TURN_CANCEL.into(),
            params: json!({ "sessionId": session_id, "turnId": turn_id, "force": true }),
        };
        handle_request(&self.context, &request).await.expect("forced cancel accepted");
    }

    async fn recv_input(&mut self) -> InlineEvent {
        timeout(Duration::from_secs(5), self.input.as_mut().expect("input open").recv())
            .await
            .expect("escalation drives the runtime input")
            .expect("input stays open until exit")
    }

    /// Simulate runloop death after `Exit`: the input channel closes and the
    /// exit oneshot resolves, which is what the watchdog confirms.
    fn finish_exit(&mut self) {
        drop(self.input.take());
        if let Some(sender) = self.exit_sender.take() {
            let _ = sender.send(Ok(()));
        }
    }

    /// Drain queued notifications, returning the `restarted` marker of every
    /// failed `turn/status` (absent counts as `false`, like normal outcomes).
    fn drain_failed_markers(&mut self) -> Vec<bool> {
        let mut markers = Vec::new();
        while let Ok(notification) = self.notifications.try_recv() {
            if notification.method == protocol::method::TURN_STATUS && notification.params["status"] == "failed" {
                markers.push(notification.params.get("restarted").and_then(Value::as_bool).unwrap_or(false));
            }
        }
        markers
    }
}

#[test]
fn escalation_grace_matches_the_planned_five_seconds() {
    assert_eq!(CANCEL_ESCALATION.grace, Duration::from_secs(5));
}

#[tokio::test]
async fn watchdog_stands_down_when_the_turn_completes_first() {
    use vtcode_core::exec::events::{ThreadEvent, TurnFailedEvent, VersionedThreadEvent};

    let mut harness = WatchdogHarness::install("stand-down", "completing-turn").await;
    harness.cancel_turn("stand-down", "completing-turn").await;
    assert!(matches!(harness.recv_input().await, InlineEvent::Cancel));
    publish_thread_event(
        "stand-down",
        VersionedThreadEvent::new(ThreadEvent::TurnFailed(TurnFailedEvent {
            completed_at: None,
            message: "cooperative stop landed".into(),
            usage: None,
        })),
        &harness.context.state,
        &harness.context.notifications,
        &harness.context.turn_finished,
    )
    .await
    .expect("terminal event publishes");
    assert!(harness.context.state.lock().await.active_turn_id.is_none());
    // Past grace plus the force-stop window: a stood-down watchdog sends
    // nothing more and leaves the healthy runtime installed.
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert!(harness.input.as_mut().expect("input open").try_recv().is_err());
    let state = harness.context.state.lock().await;
    assert!(state.runtime.is_some(), "no escalation teardown ran");
    assert!(state.runtime_generation.is_none(), "no replacement runtime was installed");
    assert!(!state.session_transitioning);
    drop(state);
    assert_eq!(
        harness.drain_failed_markers(),
        vec![false],
        "exactly the cooperative terminal outcome, without an escalation marker"
    );
    harness.runtime_task.abort();
}

#[tokio::test]
async fn terminal_event_during_escalation_keeps_the_healthy_runtime() {
    use vtcode_core::exec::events::{ThreadEvent, TurnFailedEvent, VersionedThreadEvent};

    let mut harness = WatchdogHarness::install("race-session", "racing-turn").await;
    harness.cancel_turn("race-session", "racing-turn").await;
    assert!(matches!(harness.recv_input().await, InlineEvent::Cancel));
    // The force-stop proves the watchdog claimed the escalation; a terminal
    // event racing it must win the turn without any teardown.
    assert!(matches!(harness.recv_input().await, InlineEvent::ForceCancelPtySession));
    publish_thread_event(
        "race-session",
        VersionedThreadEvent::new(ThreadEvent::TurnFailed(TurnFailedEvent {
            completed_at: None,
            message: "stop landed mid-escalation".into(),
            usage: None,
        })),
        &harness.context.state,
        &harness.context.notifications,
        &harness.context.turn_finished,
    )
    .await
    .expect("terminal event publishes");
    // Past the force-stop window: no exit handshake and no restart.
    tokio::time::sleep(Duration::from_millis(600)).await;
    let mut rest = Vec::new();
    while let Ok(event) = harness.input.as_mut().expect("input open").try_recv() {
        rest.push(event);
    }
    assert!(matches!(rest.as_slice(), [InlineEvent::Cancel]), "only the re-asserted cancel survives");
    {
        let state = harness.context.state.lock().await;
        let runtime = state.runtime.as_ref().expect("healthy runtime is kept, not restarted");
        assert!(harness.input_sender.same_channel(&runtime.input));
        assert!(!state.session_transitioning);
        assert!(state.active_turn_id.is_none());
    }
    assert_eq!(harness.drain_failed_markers(), vec![false], "the cooperative outcome stands alone");
    harness.runtime_task.abort();
}

#[test]
#[serial]
fn escalation_tears_down_stalled_turn_and_restarts_session_from_history() {
    block_on_with_desktop_stack(async {
        let sessions = IsolatedSessionStore::new();
        let mut harness = WatchdogHarness::install("watchdog-session", "stalled-turn").await;
        // Asymmetric two-message thread: reversal, duplication, or truncation
        // all fail the ordered comparison below.
        let seeded = vec![
            SessionMessage::from(&Message::user("escalation-first-question".into())),
            SessionMessage::from(&Message::assistant("escalation-first-answer".into())),
        ];
        seed_archive(sessions.path(), "watchdog-session", &harness.context.workspace, seeded.clone()).await;
        {
            let mut state = harness.context.state.lock().await;
            state
                .queue
                .push_back(QueuedInput::new(SubmittedInput::new("queued behind stall", Vec::new()), None));
        }
        harness.cancel_turn("watchdog-session", "stalled-turn").await;
        // The stalled runtime ignores cancellation; escalation must force it.
        assert!(matches!(harness.recv_input().await, InlineEvent::Cancel));
        assert!(matches!(harness.recv_input().await, InlineEvent::ForceCancelPtySession));
        assert!(matches!(harness.recv_input().await, InlineEvent::Cancel));
        assert!(matches!(harness.recv_input().await, InlineEvent::Exit));
        harness.finish_exit();

        // Both orders are legal: the failed outcome is emitted right after the
        // restart installs, while the queued prompt dispatches once the
        // replacement runtime reports ready.
        let (mut saw_failed_restart, mut saw_redispatched) = (false, false);
        timeout(Duration::from_secs(30), async {
            while !(saw_failed_restart && saw_redispatched) {
                let notification = harness.notifications.recv().await.expect("service remains connected");
                if notification.method != protocol::method::TURN_STATUS {
                    continue;
                }
                let turn = notification.params["turnId"].as_str().unwrap_or_default();
                let status = notification.params["status"].as_str().unwrap_or_default();
                if turn == "stalled-turn" && status == "failed" {
                    assert_eq!(notification.params["restarted"], true);
                    saw_failed_restart = true;
                } else if turn != "stalled-turn" && status == "running" {
                    saw_redispatched = true;
                }
            }
        })
        .await
        .expect("escalation restarts and redispatches the queue");
        {
            let state = harness.context.state.lock().await;
            assert!(state.runtime_generation.is_some(), "restart installs a new generation");
            assert!(!state.session_transitioning, "transition gate releases");
            assert!(state.queue.is_empty(), "the queued prompt dispatched to the replacement runtime");
            let runtime = state.runtime.as_ref().expect("rebuilt runtime");
            assert!(!runtime.input.is_closed(), "new runtime accepts turns");
            assert!(!harness.input_sender.same_channel(&runtime.input), "the replacement runtime owns fresh channels");
        }
        assert!(harness.input_sender.is_closed(), "the stalled runtime is gone");

        // Overwrite the seed with a marker: the rebooted runloop holds the
        // resumed history in memory, so its exit-time persist must replace the
        // marker with the full thread.
        seed_archive(
            sessions.path(),
            "watchdog-session",
            &harness.context.workspace,
            vec![SessionMessage::from(&Message::user(
                "marker-should-be-overwritten".into(),
            ))],
        )
        .await;
        timeout(Duration::from_secs(30), shutdown_current_session(&harness.context, true))
            .await
            .expect("shutdown completes before the test timeout")
            .expect("shutdown succeeds");
        harness.runtime_task.abort();

        // Independent oracle: raw archive bytes, never `load_resume_session`.
        // The resumed thread keeps the seeded prefix; the redispatched queued
        // prompt joins it as the next user message.
        let archive_path = sessions.path().join("watchdog-session.json");
        let deadline = Instant::now() + Duration::from_secs(10);
        let snapshot = loop {
            let bytes = std::fs::read(&archive_path).expect("re-persisted archive");
            let snapshot: SessionSnapshot = serde_json::from_slice(&bytes).expect("archive parses");
            let texts: Vec<String> = snapshot
                .messages
                .iter()
                .map(|message| message.content.as_text().into_owned())
                .collect();
            let seeded_texts: Vec<String> =
                seeded.iter().map(|message| message.content.as_text().into_owned()).collect();
            if texts.len() > seeded.len()
                && texts[..seeded.len()] == seeded_texts[..]
                && !texts.iter().any(|text| text.contains("marker-should-be-overwritten"))
            {
                break snapshot;
            }
            assert!(Instant::now() < deadline, "runloop never re-persisted the resumed thread: {texts:?}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        };
        let round_tripped: Vec<(MessageRole, String)> = snapshot
            .messages
            .iter()
            .map(|message| (message.role, message.content.as_text().into_owned()))
            .collect();
        let expected_prefix: Vec<(MessageRole, String)> = seeded
            .iter()
            .map(|message| (message.role, message.content.as_text().into_owned()))
            .collect();
        assert_eq!(
            round_tripped[..seeded.len()],
            expected_prefix[..],
            "seeded thread history survives the escalation restart"
        );
        assert_eq!(
            round_tripped[seeded.len()],
            (MessageRole::User, "queued behind stall".to_string()),
            "the queued prompt dispatched into the resumed thread"
        );
    });
}

#[tokio::test]
#[serial]
async fn duplicate_cancel_escalates_exactly_once_without_restart_on_corrupt_history() {
    let sessions = IsolatedSessionStore::new();
    std::fs::write(sessions.path().join("watchdog-session.json"), b"{not-json")
        .expect("corrupt archive fails the history reload");
    let mut harness = WatchdogHarness::install("watchdog-session", "stalled-turn").await;
    harness.cancel_turn("watchdog-session", "stalled-turn").await;
    harness.cancel_turn("watchdog-session", "stalled-turn").await;
    // One queued Cancel across both presses; one forced teardown total.
    assert!(matches!(harness.recv_input().await, InlineEvent::Cancel));
    assert!(matches!(harness.recv_input().await, InlineEvent::ForceCancelPtySession));
    assert!(matches!(harness.recv_input().await, InlineEvent::Cancel));
    assert!(matches!(harness.recv_input().await, InlineEvent::Exit));
    harness.finish_exit();

    let failed = timeout(Duration::from_secs(5), async {
        loop {
            let notification = harness.notifications.recv().await.expect("service remains connected");
            if notification.method == protocol::method::TURN_STATUS && notification.params["status"] == "failed" {
                break notification;
            }
        }
    })
    .await
    .expect("stopped turn emits its terminal outcome");
    assert_eq!(failed.params["turnId"], "stalled-turn");
    assert_eq!(failed.params["restarted"], false, "corrupt history skips the restart");
    // The transition gate releases asynchronously; poll instead of racing it.
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if !harness.context.state.lock().await.session_transitioning {
            break;
        }
        assert!(Instant::now() < deadline, "transition gate releases after escalation");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    {
        let state = harness.context.state.lock().await;
        assert!(state.active_turn_id.is_none());
        assert!(state.runtime.is_none(), "torn-down runtime is not replaced without history");
        assert_eq!(state.session_id.as_deref(), Some("watchdog-session"), "recovery keeps the thread");
    }
    assert!(harness.drain_failed_markers().is_empty(), "one escalation emits one terminal outcome");
    assert!(
        timeout(Duration::from_millis(500), harness.input_sender.closed()).await.is_ok(),
        "the torn-down input stays closed without a restart"
    );
    harness.runtime_task.abort();
}

#[tokio::test]
async fn forced_first_press_skips_grace_but_keeps_a_healthy_runtime() {
    use vtcode_core::exec::events::{ThreadEvent, TurnFailedEvent, VersionedThreadEvent};

    let mut harness = WatchdogHarness::install("force-first", "forced-turn").await;
    {
        let mut state = harness.context.state.lock().await;
        state.escalation_waits = Some(EscalationWaits {
            grace: Duration::from_secs(60),
            turn_wait: Duration::from_millis(200),
            exit_wait: Duration::from_millis(200),
        });
    }
    harness.force_cancel_turn("force-first", "forced-turn").await;
    assert!(matches!(harness.recv_input().await, InlineEvent::Cancel));
    // Prompt arrival despite the 60s grace proves the forced first press
    // skipped it; a terminal event racing the endgame must still win the
    // turn without any teardown.
    assert!(matches!(harness.recv_input().await, InlineEvent::ForceCancelPtySession));
    publish_thread_event(
        "force-first",
        VersionedThreadEvent::new(ThreadEvent::TurnFailed(TurnFailedEvent {
            completed_at: None,
            message: "stop landed mid-escalation".into(),
            usage: None,
        })),
        &harness.context.state,
        &harness.context.notifications,
        &harness.context.turn_finished,
    )
    .await
    .expect("terminal event publishes");
    tokio::time::sleep(Duration::from_millis(600)).await;
    let mut rest = Vec::new();
    while let Ok(event) = harness.input.as_mut().expect("input open").try_recv() {
        rest.push(event);
    }
    assert!(matches!(rest.as_slice(), [InlineEvent::Cancel]), "only the re-asserted cancel survives");
    {
        let state = harness.context.state.lock().await;
        let runtime = state.runtime.as_ref().expect("healthy runtime is kept, not restarted");
        assert!(harness.input_sender.same_channel(&runtime.input));
        assert!(!state.session_transitioning);
        assert!(state.active_turn_id.is_none());
    }
    assert_eq!(harness.drain_failed_markers(), vec![false], "the cooperative outcome stands alone");
    harness.runtime_task.abort();
}

#[tokio::test]
#[serial]
async fn forced_repeat_skips_a_long_grace_and_tears_down_exactly_once() {
    let sessions = IsolatedSessionStore::new();
    std::fs::write(sessions.path().join("watchdog-session.json"), b"{not-json")
        .expect("corrupt archive fails the history reload");
    let mut harness = WatchdogHarness::install("watchdog-session", "stalled-turn").await;
    {
        let mut state = harness.context.state.lock().await;
        state.escalation_waits = Some(EscalationWaits {
            grace: Duration::from_secs(60),
            turn_wait: Duration::from_millis(200),
            exit_wait: Duration::from_millis(200),
        });
    }
    harness.cancel_turn("watchdog-session", "stalled-turn").await;
    // One queued Cancel across both presses; the forced repeat — not the
    // 60s-grace watchdog — owns the single teardown below. Every receive
    // below times out in 5s, so prompt arrival proves the grace was skipped.
    assert!(matches!(harness.recv_input().await, InlineEvent::Cancel));
    harness.force_cancel_turn("watchdog-session", "stalled-turn").await;
    assert!(matches!(harness.recv_input().await, InlineEvent::ForceCancelPtySession));
    assert!(matches!(harness.recv_input().await, InlineEvent::Cancel));
    assert!(matches!(harness.recv_input().await, InlineEvent::Exit));
    harness.finish_exit();

    let failed = timeout(Duration::from_secs(5), async {
        loop {
            let notification = harness.notifications.recv().await.expect("service remains connected");
            if notification.method == protocol::method::TURN_STATUS && notification.params["status"] == "failed" {
                break notification;
            }
        }
    })
    .await
    .expect("stopped turn emits its terminal outcome");
    assert_eq!(failed.params["turnId"], "stalled-turn");
    assert_eq!(failed.params["restarted"], false, "corrupt history skips the restart");
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if !harness.context.state.lock().await.session_transitioning {
            break;
        }
        assert!(Instant::now() < deadline, "transition gate releases after escalation");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    {
        let state = harness.context.state.lock().await;
        assert!(state.active_turn_id.is_none());
        assert!(state.runtime.is_none(), "torn-down runtime is not replaced without history");
        assert_eq!(state.session_id.as_deref(), Some("watchdog-session"), "recovery keeps the thread");
    }
    assert!(harness.drain_failed_markers().is_empty(), "one escalation emits one terminal outcome");
    harness.runtime_task.abort();
}

#[tokio::test]
#[serial]
async fn restart_failure_surfaces_and_leaves_the_session_reloadable() {
    let sessions = IsolatedSessionStore::new();
    std::fs::write(sessions.path().join("watchdog-session.json"), b"{not-json")
        .expect("corrupt archive fails the history reload");
    let mut harness = WatchdogHarness::install("watchdog-session", "stalled-turn").await;
    {
        let mut state = harness.context.state.lock().await;
        state
            .queue
            .push_back(QueuedInput::new(SubmittedInput::new("preserved across teardown", Vec::new()), None));
    }
    harness.cancel_turn("watchdog-session", "stalled-turn").await;
    assert!(matches!(harness.recv_input().await, InlineEvent::Cancel));
    assert!(matches!(harness.recv_input().await, InlineEvent::ForceCancelPtySession));
    assert!(matches!(harness.recv_input().await, InlineEvent::Cancel));
    assert!(matches!(harness.recv_input().await, InlineEvent::Exit));
    harness.finish_exit();
    timeout(Duration::from_secs(5), async {
        loop {
            let notification = harness.notifications.recv().await.expect("service remains connected");
            if notification.method == protocol::method::TURN_STATUS
                && notification.params["turnId"] == "stalled-turn"
                && notification.params["status"] == "failed"
            {
                assert_eq!(notification.params["restarted"], false);
                break;
            }
        }
    })
    .await
    .expect("stopped turn emits its terminal outcome");
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if !harness.context.state.lock().await.session_transitioning {
            break;
        }
        assert!(Instant::now() < deadline, "failed restart releases the transition gate");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // Submissions fail as stopped-runtime, not as stuck-transition, and the
    // queue survives in place for the reloaded session.
    let error = enqueue_turn(
        &harness.context.state,
        &harness.context.notifications,
        "watchdog-session",
        "after failed restart".into(),
        Vec::new(),
        EnqueueMode::Immediate,
    )
    .await
    .expect_err("no runtime can accept the turn");
    assert!(error.to_string().contains("selected session is not running"), "unexpected error: {error}");
    {
        let state = harness.context.state.lock().await;
        assert_eq!(state.queue.len(), 1);
        assert_eq!(state.queue.front().expect("queued").input.text, "preserved across teardown");
    }
    // Transition-gated requests work again: the session is reloadable.
    let request = Request {
        jsonrpc: "2.0".into(),
        id: RequestId::Number(2),
        method: protocol::method::CONFIG_THEME.into(),
        params: json!({ "theme": "ciapre-blue" }),
    };
    handle_request(&harness.context, &request)
        .await
        .expect("settings apply after failed restart");
    assert_eq!(harness.context.state.lock().await.theme_override.as_deref(), Some("ciapre-blue"));
    harness.runtime_task.abort();
}

#[tokio::test]
#[serial]
async fn unconfirmed_termination_keeps_submissions_blocked() {
    let _sessions = IsolatedSessionStore::new();
    let mut harness = WatchdogHarness::install("watchdog-session", "stalled-turn").await;
    harness.cancel_turn("watchdog-session", "stalled-turn").await;
    assert!(matches!(harness.recv_input().await, InlineEvent::Cancel));
    assert!(matches!(harness.recv_input().await, InlineEvent::ForceCancelPtySession));
    assert!(matches!(harness.recv_input().await, InlineEvent::Cancel));
    assert!(matches!(harness.recv_input().await, InlineEvent::Exit));
    // The runtime never dies: its exit stays pending and its input stays open
    // even after the abort, so termination cannot be confirmed.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if harness.context.state.lock().await.session_transitioning {
            break;
        }
        assert!(Instant::now() < deadline, "escalation claims the transition");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    tokio::time::sleep(Duration::from_millis(1500)).await;
    {
        let state = harness.context.state.lock().await;
        assert!(state.session_transitioning, "unconfirmed termination holds the gate");
        assert_eq!(state.active_turn_id.as_deref(), Some("stalled-turn"), "the turn is not reported stopped");
        assert_eq!(state.cancel_requested_turn.as_deref(), Some("stalled-turn"));
    }
    let error = enqueue_turn(
        &harness.context.state,
        &harness.context.notifications,
        "watchdog-session",
        "blocked submission".into(),
        Vec::new(),
        EnqueueMode::Immediate,
    )
    .await
    .expect_err("blocked session rejects submissions");
    assert!(error.to_string().contains("session change is in progress"), "unexpected error: {error}");
    assert!(harness.notifications.try_recv().is_err(), "no terminal outcome is emitted for an unconfirmed stop");
    drop(harness.input.take());
    drop(harness.exit_sender.take());
    harness.runtime_task.abort();
}
