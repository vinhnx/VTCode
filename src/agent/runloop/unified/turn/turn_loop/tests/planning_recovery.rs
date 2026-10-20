use super::*;
use crate::agent::runloop::unified::state::CtrlCState;
use std::collections::VecDeque;
use std::time::Duration;
use vtcode_core::exec::events::planning::{PlanningOutcome, PlanningRecoverySnapshot, PlanningResponseClass};

const RETAINED_DRAFT: &str = r#"# Recover the planning handoff

## Summary
Preserve the saved evidence and repair the planning handoff.

## Implementation Steps
1. Repair the planning handoff -> files: [src/agent/runloop/unified/turn/turn_loop.rs] -> verify: [cargo nextest run --locked -p vtcode planning_recovery]
2. Document recovery choices -> files: [docs/guides/planning-workflow.md] -> verify: [review it]

## Test Cases and Validation
1. Exercise the bounded recovery regression and confirm no fourth provider request.

## Assumptions and Defaults
1. Keep implementation subject to separate plan approval.
"#;

const REPAIRED_DRAFT: &str = r#"# Recover the planning handoff

## Summary
Preserve the saved evidence and repair the planning handoff.

## Implementation Steps
1. Repair the planning handoff -> files: [src/agent/runloop/unified/turn/turn_loop.rs] -> verify: [cargo nextest run --locked -p vtcode planning_recovery]
2. Document recovery choices -> files: [docs/guides/planning-workflow.md] -> verify: [python3 scripts/check_markdown.py]

## Test Cases and Validation
1. Exercise the bounded recovery regression and confirm no fourth provider request.
2. Run the Markdown checker for the documented recovery choices.

## Assumptions and Defaults
1. Keep implementation subject to separate plan approval.
"#;

#[derive(Clone, Copy)]
enum Reply {
    Empty,
    NativeToolCall,
    ApprovalOnly,
    InvalidDraft,
    RepairedDraft,
    Cancel,
    ProviderFailure,
}

struct PlanningRecoveryProvider {
    replies: Mutex<VecDeque<Reply>>,
    requests: Arc<Mutex<Vec<uni::LLMRequest>>>,
    cancellation: Option<Arc<CtrlCState>>,
}

#[async_trait::async_trait]
impl uni::LLMProvider for PlanningRecoveryProvider {
    fn name(&self) -> &str {
        "openai"
    }

    fn supports_streaming(&self) -> bool {
        false
    }

    async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
        self.requests.lock().expect("request capture").push(request.clone());
        let reply = self
            .replies
            .lock()
            .expect("reply script")
            .pop_front()
            .expect("unexpected extra model request");
        if matches!(reply, Reply::ProviderFailure) {
            return Err(uni::LLMError::Network {
                message: "test network connection reset".into(),
                metadata: None,
            });
        }
        if matches!(reply, Reply::Cancel) {
            self.cancellation.as_ref().expect("cancellation state").request_local_cancel();
            return Err(uni::LLMError::Provider {
                message: "request cancelled".into(),
                metadata: None,
            });
        }
        let content = match reply {
            Reply::Empty | Reply::NativeToolCall => None,
            Reply::ApprovalOnly => Some("The plan is ready. Please approve implementation.".into()),
            Reply::InvalidDraft => Some(format!("<proposed_plan>\n{RETAINED_DRAFT}\n</proposed_plan>")),
            Reply::RepairedDraft => Some(format!("<proposed_plan>\n{REPAIRED_DRAFT}\n</proposed_plan>")),
            Reply::Cancel | Reply::ProviderFailure => panic!("failures must return before response assembly"),
        };
        let tool_calls = matches!(reply, Reply::NativeToolCall).then(|| {
            vec![uni::ToolCall::function(
                "forbidden-research".into(),
                "read_file".into(),
                r#"{"path":"README.md"}"#.into(),
            )]
        });
        Ok(uni::LLMResponse {
            content,
            model: request.model,
            tool_calls,
            usage: None,
            finish_reason: if matches!(reply, Reply::NativeToolCall) {
                uni::FinishReason::ToolCalls
            } else {
                uni::FinishReason::Stop
            },
            reasoning: None,
            reasoning_details: None,
            organization_id: None,
            request_id: None,
            tool_references: Vec::new(),
            compaction: None,
        })
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["noop-model".into()]
    }

    fn validate_request(&self, _request: &uni::LLMRequest) -> Result<(), uni::LLMError> {
        Ok(())
    }
}

fn install_script(backing: &mut TestTurnProcessingBacking, replies: &[Reply]) -> Arc<Mutex<Vec<uni::LLMRequest>>> {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let cancellation = backing.turn_loop_context().ctrl_c_state.clone();
    backing.set_provider(Box::new(PlanningRecoveryProvider {
        replies: Mutex::new(replies.iter().copied().collect()),
        requests: requests.clone(),
        cancellation: Some(cancellation),
    }));
    requests
}

fn retained_history() -> Vec<uni::Message> {
    vec![
        uni::Message::user("Plan a bounded recovery fix using the saved evidence.".into()),
        uni::Message::assistant_with_tools(
            String::new(),
            vec![uni::ToolCall::function(
                "prior-research".into(),
                "read_file".into(),
                r#"{"path":"README.md"}"#.into(),
            )],
        ),
        uni::Message::tool_response("prior-research".into(), "README.md: previously inspected public evidence".into()),
        uni::Message::assistant(format!("<proposed_plan>\n{RETAINED_DRAFT}\n</proposed_plan>")),
    ]
}

fn events(path: &Path) -> Vec<ThreadEvent> {
    fs::read_to_string(path)
        .expect("harness checkpoint")
        .lines()
        .map(|line| {
            serde_json::from_str::<VersionedThreadEvent>(line)
                .expect("versioned event")
                .into_event()
        })
        .collect()
}

fn checkpoints(path: &Path) -> Vec<PlanningRecoverySnapshot> {
    events(path)
        .into_iter()
        .filter_map(|event| match event {
            ThreadEvent::PlanDelta(delta) => delta.recovery.map(|snapshot| *snapshot),
            _ => None,
        })
        .collect()
}

fn assert_tool_free(requests: &Arc<Mutex<Vec<uni::LLMRequest>>>, count: usize) {
    let requests = requests.lock().expect("captured requests");
    assert_eq!(requests.len(), count);
    for request in requests.iter() {
        assert!(matches!(request.tool_choice, Some(uni::ToolChoice::None)), "post-guard request must disable tools");
        assert!(
            request.messages.iter().any(|message| message.role == uni::MessageRole::System
                && message.content.as_text().contains("research is closed")),
            "synthesis cannot reopen research"
        );
    }
}

#[tokio::test]
async fn planning_recovery_mixed_failures_share_three_requests_and_one_input_handoff() {
    let mut backing = TestTurnProcessingBacking::new(8).await;
    backing.activate_planning_for_test();
    backing.activate_tool_free_recovery_for_test("Planning research guard reached.");
    let harness = backing.enable_harness_emitter();
    let requests = install_script(&mut backing, &[Reply::Empty, Reply::NativeToolCall, Reply::ApprovalOnly]);
    let diagnostics = Arc::new(Mutex::new(Vec::new()));
    let captured = diagnostics.clone();
    let mut renderer = AnsiRenderer::stdout();
    renderer.set_diagnostic_sink(Arc::new(move |diagnostic| {
        captured.lock().expect("diagnostics").push(diagnostic.clone());
        Ok(())
    }));
    let mut history = retained_history();
    let mut ctx = backing.turn_loop_context();
    ctx.renderer = &mut renderer;
    ctx.skip_confirmations = false;
    let outcome = run_turn_loop(&mut history, ctx).await.expect("bounded planning failure");
    assert!(
        matches!(outcome.result, TurnLoopResult::Blocked { reason: Some(reason) } if reason == "planning awaiting input")
    );
    assert!(!outcome.plan_approved_execution_pending);
    assert!(outcome.turn_modified_files.is_empty());
    assert_eq!(
        outcome.turn_diagnostics.admitted_tool_calls, 0,
        "forbidden research must never reach tool execution"
    );
    assert_tool_free(&requests, 3);
    let snapshots = checkpoints(&harness);
    for class in [
        PlanningResponseClass::EmptyContent,
        PlanningResponseClass::NativeToolCalls,
        PlanningResponseClass::MalformedPlan,
    ] {
        assert!(
            snapshots.iter().any(|snapshot| snapshot.response_class == Some(class)),
            "missing classified response {class:?}"
        );
    }
    let saved = snapshots.last().expect("awaiting-input checkpoint");
    assert_eq!(saved.outcome, PlanningOutcome::AwaitingInput);
    assert_eq!(saved.requests_used, 3);
    assert!(saved.research_closed && saved.input_prompt_shown);
    assert_eq!(saved.best_draft.as_deref(), Some(RETAINED_DRAFT.trim()));
    assert_eq!(saved.evidence.len(), 1);
    assert!(
        !events(&harness)
            .iter()
            .any(|event| matches!(event, ThreadEvent::PlanApprovalRequested(_)))
    );
    let notices = diagnostics.lock().expect("diagnostics");
    assert_eq!(
        notices
            .iter()
            .filter(|diagnostic| diagnostic.code == "planning.awaiting_input")
            .count(),
        1
    );
    let notice = notices
        .iter()
        .find(|diagnostic| diagnostic.code == "planning.awaiting_input")
        .unwrap();
    assert_eq!(
        notice.actions.iter().map(|action| action.id.as_str()).collect::<Vec<_>>(),
        ["retry", "clarify", "model"]
    );
}

#[tokio::test]
async fn planning_recovery_repaired_plan_persists_before_approval_publication() {
    let mut backing = TestTurnProcessingBacking::new(8).await;
    backing.activate_planning_for_test();
    backing.mark_interview_denied_for_test();
    backing.activate_tool_free_recovery_for_test("Planning research guard reached.");
    let harness = backing.enable_harness_emitter();
    let requests = install_script(&mut backing, &[Reply::InvalidDraft, Reply::RepairedDraft]);
    let mut history = retained_history();
    let mut renderer = AnsiRenderer::stdout();
    let mut ctx = backing.turn_loop_context();
    ctx.renderer = &mut renderer;
    let outcome = run_turn_loop(&mut history, ctx).await.expect("valid repaired planning handoff");
    assert_tool_free(&requests, 2);
    assert!(!outcome.plan_approved_execution_pending, "recovery must wait for separate plan approval");
    let events = events(&harness);
    assert!(!events.iter().any(|event| matches!(event, ThreadEvent::PlanApprovalResolved(_))));
    let ready = events
        .iter()
        .position(|event| {
            matches!(event, ThreadEvent::PlanDelta(delta)
        if delta.recovery.as_ref().is_some_and(|snapshot| snapshot.outcome == PlanningOutcome::AwaitingApproval))
        })
        .expect("persisted approval-ready checkpoint");
    let requested = events
        .iter()
        .position(|event| matches!(event, ThreadEvent::PlanApprovalRequested(_)))
        .expect("approval event");
    assert!(ready < requested, "readiness must be persisted before approval is offered");
    let ThreadEvent::PlanApprovalRequested(approval) = &events[requested] else {
        panic!("expected approval event at the requested position")
    };
    let path = approval.plan_file.as_ref().expect("persisted plan path");
    let path = backing.workspace_path().join(path);
    let plan = fs::read_to_string(path).expect("approval must reference a persisted artifact");
    assert!(plan.contains("python3 scripts/check_markdown.py"));
    assert!(!plan.contains("verify: [review it]"));
}

#[tokio::test]
async fn planning_recovery_resume_preserves_consumed_admission_and_cannot_reopen_research() {
    let mut original = TestTurnProcessingBacking::new(8).await;
    original.activate_planning_for_test();
    original.activate_tool_free_recovery_for_test("Planning research guard reached.");
    let harness = original.enable_harness_emitter();
    let original_requests = install_script(&mut original, &[Reply::Cancel]);
    let mut history = retained_history();
    let outcome = run_turn_loop(&mut history, original.turn_loop_context())
        .await
        .expect("cancelled synthesis is resumable");
    assert!(matches!(outcome.result, TurnLoopResult::Aborted | TurnLoopResult::Cancelled));
    assert_tool_free(&original_requests, 1);
    let saved = checkpoints(&harness)
        .pop()
        .expect("pre-request admission persisted before cancellation");
    assert_eq!(saved.requests_used, 1);
    let serialized = serde_json::to_string(&saved).expect("serialize resume checkpoint");
    let restored: PlanningRecoverySnapshot = serde_json::from_str(&serialized).expect("rehydrate resume checkpoint");

    let mut resumed = TestTurnProcessingBacking::new(8).await;
    resumed.activate_planning_for_test();
    resumed.mark_interview_denied_for_test();
    resumed.turn_loop_context().plan_session.recovery = restored;
    let requests = install_script(&mut resumed, &[Reply::Empty, Reply::ApprovalOnly]);
    let outcome = run_turn_loop(&mut history, resumed.turn_loop_context())
        .await
        .expect("resumed bounded synthesis");
    assert!(
        matches!(outcome.result, TurnLoopResult::Blocked { reason: Some(reason) } if reason == "planning awaiting input")
    );
    assert_tool_free(&requests, 2);
    let exhausted = {
        let ctx = resumed.turn_loop_context();
        assert_eq!(ctx.plan_session.recovery.requests_used, 3);
        assert_eq!(ctx.plan_session.recovery.outcome, PlanningOutcome::AwaitingInput);
        assert_eq!(ctx.plan_session.recovery.cycle, 0, "automatic resume cannot replenish the cycle");
        assert!(ctx.plan_session.recovery.research_closed);
        serde_json::to_string(&ctx.plan_session.recovery).expect("serialize exhausted handoff")
    };
    // Restoring an already displayed input handoff cannot create another
    // synthesis request or re-present the same incident without human action.
    let mut idle = TestTurnProcessingBacking::new(8).await;
    idle.activate_planning_for_test();
    idle.turn_loop_context().plan_session.recovery =
        serde_json::from_str(&exhausted).expect("restore exhausted handoff");
    let idle_requests = install_script(&mut idle, &[]);
    let notices = Arc::new(Mutex::new(Vec::new()));
    let captured = notices.clone();
    let mut renderer = AnsiRenderer::stdout();
    renderer.set_diagnostic_sink(Arc::new(move |diagnostic| {
        captured.lock().expect("resumed notices").push(diagnostic.clone());
        Ok(())
    }));
    let mut ctx = idle.turn_loop_context();
    ctx.renderer = &mut renderer;
    let outcome = run_turn_loop(&mut history, ctx).await.expect("restore idle input handoff");
    assert!(
        matches!(outcome.result, TurnLoopResult::Blocked { reason: Some(reason) } if reason == "planning awaiting input")
    );
    assert_tool_free(&idle_requests, 0);
    assert!(!outcome.plan_approved_execution_pending);
    assert!(
        notices
            .lock()
            .expect("resumed notices")
            .iter()
            .all(|notice| notice.code != "planning.awaiting_input")
    );
}

#[tokio::test]
async fn planning_recovery_targeted_clarification_research_is_one_batch_and_cannot_survive_budget_denial() {
    for hard_budget_exhausted in [false, true] {
        let mut backing = TestTurnProcessingBacking::new(8).await;
        backing.activate_planning_for_test();
        backing.activate_tool_free_recovery_for_test("Research guard reached before clarification.");
        let mut ctx = backing.turn_processing_context();
        ctx.plan_session.recovery = PlanningRecoverySnapshot {
            outcome: PlanningOutcome::Synthesizing,
            research_closed: true,
            targeted_research: Some("Inspect only the newly identified missing configuration file.".into()),
            ..Default::default()
        };
        if hard_budget_exhausted {
            ctx.harness_state.tool_calls = ctx.harness_state.max_tool_calls;
        }
        assert!(
            ctx.admit_planning_recovery_request("noop-model")
                .await
                .expect("clarification admission")
        );
        assert!(
            ctx.plan_session.recovery.targeted_research.is_none(),
            "research authorization is consumed even when a hard budget denies it"
        );
        assert!(ctx.plan_session.recovery.research_closed);
        if hard_budget_exhausted {
            assert_eq!(ctx.plan_session.recovery.requests_used, 1);
            assert_eq!(ctx.plan_session.recovery.outcome, PlanningOutcome::Synthesizing);
            assert!(ctx.harness_state.recovery_is_tool_free());
            // A later turn can have a fresh local budget, but it must not
            // revive the denied research batch after synthesis has begun.
            ctx.harness_state.tool_calls = 0;
        } else {
            assert_eq!(ctx.plan_session.recovery.requests_used, 0);
            assert_eq!(ctx.plan_session.recovery.outcome, PlanningOutcome::Researching);
            assert!(!ctx.harness_state.recovery_is_tool_free());
        }
        assert!(
            ctx.admit_planning_recovery_request("noop-model")
                .await
                .expect("synthesis admission")
        );
        assert_eq!(ctx.plan_session.recovery.requests_used, if hard_budget_exhausted { 2 } else { 1 });
        assert!(
            ctx.harness_state.recovery_is_tool_free(),
            "only the explicitly authorized first batch may use tools"
        );
        assert_ne!(ctx.plan_session.recovery.outcome, PlanningOutcome::Researching);
    }
}

#[tokio::test]
async fn planning_recovery_transport_retries_share_the_three_actual_dispatch_allowance() {
    for repaired_on_last_dispatch in [false, true] {
        let mut backing = TestTurnProcessingBacking::new(8).await;
        backing.activate_planning_for_test();
        backing.mark_interview_denied_for_test();
        backing.activate_tool_free_recovery_for_test("Planning research guard reached.");
        let harness = backing.enable_harness_emitter();
        let requests = install_script(
            &mut backing,
            &[
                Reply::ProviderFailure,
                Reply::ProviderFailure,
                if repaired_on_last_dispatch {
                    Reply::RepairedDraft
                } else {
                    Reply::ProviderFailure
                },
            ],
        );
        let mut history = retained_history();
        let mut renderer = AnsiRenderer::stdout();
        let mut ctx = backing.turn_loop_context();
        ctx.renderer = &mut renderer;
        let outcome = run_turn_loop(&mut history, ctx).await.expect("bounded transport retries");
        assert_tool_free(&requests, 3);
        let saved = checkpoints(&harness);
        assert!(saved.iter().all(|snapshot| snapshot.requests_used <= 3));
        assert!(saved.iter().any(|snapshot| snapshot.requests_used == 3));
        if repaired_on_last_dispatch {
            assert!(!outcome.plan_approved_execution_pending);
            assert!(
                !events(&harness)
                    .iter()
                    .any(|event| matches!(event, ThreadEvent::PlanApprovalResolved(_)))
            );
            assert_eq!(backing.turn_loop_context().plan_session.recovery.outcome, PlanningOutcome::AwaitingApproval);
            assert!(
                events(&harness)
                    .iter()
                    .any(|event| matches!(event, ThreadEvent::PlanApprovalRequested(_)))
            );
        } else {
            assert!(
                matches!(outcome.result, TurnLoopResult::Blocked { reason: Some(reason) } if reason == "planning awaiting input")
            );
            assert!(!outcome.plan_approved_execution_pending);
            let ctx = backing.turn_loop_context();
            assert_eq!(ctx.plan_session.recovery.requests_used, 3);
            assert_eq!(ctx.plan_session.recovery.outcome, PlanningOutcome::AwaitingInput);
            assert_eq!(ctx.plan_session.recovery.response_class, Some(PlanningResponseClass::ProviderFailure));
            assert!(
                !events(&harness)
                    .iter()
                    .any(|event| matches!(event, ThreadEvent::PlanApprovalRequested(_)))
            );
        }
    }
}

struct RecoveryModalFixture {
    handle: InlineHandle,
    commands: tokio::sync::mpsc::UnboundedReceiver<vtcode_ui::tui::app::InlineCommand>,
    events: Option<tokio::sync::mpsc::UnboundedSender<InlineEvent>>,
    session: InlineSession,
    renderer: AnsiRenderer,
}

enum PopupAnswer {
    Option(usize),
    Text(&'static str),
}

impl RecoveryModalFixture {
    fn new() -> Self {
        let (command_tx, commands) = tokio::sync::mpsc::unbounded_channel();
        let (events, event_rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(command_tx);
        let session = InlineSession {
            handle: handle.clone(),
            events: event_rx,
            worker: None,
        };
        let renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
        Self {
            handle,
            commands,
            events: Some(events),
            session,
            renderer,
        }
    }

    fn answer_displayed_options(
        &mut self,
        replies: Vec<Vec<PopupAnswer>>,
        dismiss_approval: bool,
    ) -> tokio::task::JoinHandle<usize> {
        let (_, replacement) = tokio::sync::mpsc::unbounded_channel();
        let mut commands = std::mem::replace(&mut self.commands, replacement);
        let events = self.events.as_ref().expect("modal event sender").clone();
        tokio::spawn(async move {
            let mut count = 0;
            for replies in replies {
                let request = loop {
                    let command = tokio::time::timeout(Duration::from_secs(3), commands.recv())
                        .await
                        .expect("popup must be presented")
                        .expect("popup command channel");
                    if let vtcode_ui::tui::app::InlineCommand::ShowTransient { request } = command
                        && let vtcode_ui::tui::app::TransientRequest::Wizard(request) = *request
                    {
                        break request;
                    }
                };
                assert_eq!(request.steps.len(), replies.len());
                let selections = request
                    .steps
                    .iter()
                    .zip(replies)
                    .map(|(step, reply)| match reply {
                        PopupAnswer::Option(index) => step.items[index].selection.clone().expect("displayed action"),
                        PopupAnswer::Text(text) => {
                            let mut selection = step
                                .items
                                .iter()
                                .find_map(|item| match item.selection.as_ref() {
                                    Some(
                                        selection @ InlineListSelection::RequestUserInputAnswer {
                                            other: Some(_), ..
                                        },
                                    ) => Some(selection.clone()),
                                    _ => None,
                                })
                                .expect("displayed free-text action");
                            if let InlineListSelection::RequestUserInputAnswer { other, .. } = &mut selection {
                                *other = Some(text.into());
                            }
                            selection
                        }
                    })
                    .collect();
                events
                    .send(InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Wizard(selections))))
                    .expect("submit the actual displayed option payload");
                count += 1;
            }
            if dismiss_approval {
                loop {
                    let command = tokio::time::timeout(Duration::from_secs(3), commands.recv())
                        .await
                        .expect("approval must be presented")
                        .expect("approval command channel");
                    if let vtcode_ui::tui::app::InlineCommand::ShowTransient { request } = command
                        && let vtcode_ui::tui::app::TransientRequest::List(request) = *request
                        && request
                            .items
                            .iter()
                            .any(|item| matches!(item.selection, Some(InlineListSelection::PlanApprovalExecute)))
                    {
                        events
                            .send(InlineEvent::Transient(TransientEvent::Cancelled))
                            .expect("dismiss implementation approval");
                        break;
                    }
                }
            }
            count
        })
    }

    fn wizard_count(&mut self) -> usize {
        let mut count = 0;
        while let Ok(command) = self.commands.try_recv() {
            if let vtcode_ui::tui::app::InlineCommand::ShowTransient { request } = command
                && matches!(*request, vtcode_ui::tui::app::TransientRequest::Wizard(_))
            {
                count += 1;
            }
        }
        count
    }
}

fn exhausted_recovery_snapshot() -> PlanningRecoverySnapshot {
    PlanningRecoverySnapshot {
        outcome: PlanningOutcome::Repairing,
        requests_used: 3,
        research_closed: true,
        latest_request: "Plan the recovery fix".into(),
        best_draft: Some(RETAINED_DRAFT.trim().into()),
        evidence: vec!["retained public evidence".into()],
        ..Default::default()
    }
}

#[tokio::test]
async fn planning_recovery_displayed_retry_reaches_provider_and_persists_plan_before_approval() {
    let mut backing = TestTurnProcessingBacking::new(8).await;
    backing.activate_planning_for_test();
    backing.activate_tool_free_recovery_for_test("Planning research guard reached.");
    let harness = backing.enable_harness_emitter();
    let requests = install_script(
        &mut backing,
        &[
            Reply::Empty,
            Reply::NativeToolCall,
            Reply::ApprovalOnly,
            Reply::RepairedDraft,
        ],
    );
    let mut modal = RecoveryModalFixture::new();
    let popup = modal.answer_displayed_options(vec![vec![PopupAnswer::Option(0)]], true);
    let mut history = retained_history();
    let mut ctx = backing.turn_loop_context();
    ctx.handle = &modal.handle;
    ctx.session = &mut modal.session;
    ctx.renderer = &mut modal.renderer;
    let outcome = run_turn_loop(&mut history, ctx).await.expect("retry from displayed popup");
    assert_eq!(popup.await.expect("popup interaction"), 1);
    assert_tool_free(&requests, 4);
    assert!(!outcome.plan_approved_execution_pending, "retry cannot approve implementation");
    assert!(outcome.turn_modified_files.is_empty());
    assert_eq!(outcome.turn_diagnostics.admitted_tool_calls, 0);
    let saved = checkpoints(&harness);
    let last = saved.last().expect("persisted repaired plan");
    assert_eq!(last.outcome, PlanningOutcome::AwaitingApproval);
    assert_eq!(last.cycle, 1);
    assert_eq!(last.requests_used, 1);
    assert!(
        last.best_draft
            .as_deref()
            .unwrap()
            .contains("python3 scripts/check_markdown.py")
    );
    assert!(
        events(&harness)
            .iter()
            .any(|event| matches!(event, ThreadEvent::PlanApprovalRequested(_)))
    );
}

#[tokio::test]
async fn planning_recovery_runtime_retry_answer_starts_one_cycle_without_approval() {
    let mut backing = TestTurnProcessingBacking::new(8).await;
    backing.activate_planning_for_test();
    let harness = backing.enable_harness_emitter();
    let mut modal = RecoveryModalFixture::new();
    let popup = modal.answer_displayed_options(vec![vec![PopupAnswer::Option(0)]], false);
    let mut ctx = backing.turn_processing_context();
    ctx.plan_session.recovery = exhausted_recovery_snapshot();
    ctx.handle = &modal.handle;
    ctx.session = &mut modal.session;
    ctx.renderer = &mut modal.renderer;
    let outcome = ctx
        .await_planning_recovery_input("all three responses failed")
        .await
        .expect("runtime retry response");
    assert!(matches!(outcome, TurnHandlerOutcome::Continue));
    assert_eq!(ctx.plan_session.recovery.cycle, 1);
    assert_eq!(ctx.plan_session.recovery.requests_used, 0);
    assert!(ctx.plan_session.recovery.research_closed);
    assert_eq!(ctx.plan_session.recovery.best_draft.as_deref(), Some(RETAINED_DRAFT.trim()));
    assert!(!ctx.plan_session.recovery.model_selection_pending);
    assert_eq!(popup.await.expect("popup interaction"), 1);
    assert!(
        !events(&harness)
            .iter()
            .any(|event| matches!(event, ThreadEvent::PlanApprovalRequested(_)))
    );
}

#[tokio::test]
async fn planning_recovery_runtime_clarification_preserves_text_and_requires_explicit_research_choice() {
    for request_research in [false, true] {
        let mut backing = TestTurnProcessingBacking::new(8).await;
        backing.activate_planning_for_test();
        let harness = backing.enable_harness_emitter();
        let mut modal = RecoveryModalFixture::new();
        let popup = modal.answer_displayed_options(
            vec![
                vec![PopupAnswer::Option(1)],
                vec![
                    PopupAnswer::Text("The newly missing fact is the configured model endpoint."),
                    PopupAnswer::Option(usize::from(request_research)),
                ],
            ],
            false,
        );
        let mut ctx = backing.turn_processing_context();
        ctx.plan_session.recovery = exhausted_recovery_snapshot();
        ctx.handle = &modal.handle;
        ctx.session = &mut modal.session;
        ctx.renderer = &mut modal.renderer;
        let outcome = ctx
            .await_planning_recovery_input("draft needs clarification")
            .await
            .expect("runtime clarification response");
        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert_eq!(ctx.plan_session.recovery.cycle, 1);
        assert_eq!(ctx.plan_session.recovery.requests_used, 0);
        assert!(ctx.plan_session.recovery.research_closed);
        assert!(
            ctx.plan_session
                .recovery
                .latest_request
                .contains("The newly missing fact is the configured model endpoint.")
        );
        assert_eq!(ctx.plan_session.recovery.targeted_research.is_some(), request_research);
        assert_eq!(ctx.plan_session.recovery.evidence, ["retained public evidence"]);
        assert_eq!(popup.await.expect("clarification popup interaction"), 2);
        assert!(
            !events(&harness)
                .iter()
                .any(|event| matches!(event, ThreadEvent::PlanApprovalRequested(_)))
        );
    }
}

#[tokio::test]
async fn planning_recovery_runtime_model_answer_waits_for_explicit_picker_selection() {
    let mut backing = TestTurnProcessingBacking::new(8).await;
    backing.activate_planning_for_test();
    let harness = backing.enable_harness_emitter();
    let mut modal = RecoveryModalFixture::new();
    let popup = modal.answer_displayed_options(vec![vec![PopupAnswer::Option(2)]], false);
    let mut ctx = backing.turn_processing_context();
    ctx.plan_session.recovery = exhausted_recovery_snapshot();
    ctx.handle = &modal.handle;
    ctx.session = &mut modal.session;
    ctx.renderer = &mut modal.renderer;
    let outcome = ctx
        .await_planning_recovery_input("provider returned no public content")
        .await
        .expect("model choice handoff");
    assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Blocked { .. })));
    assert_eq!(ctx.plan_session.recovery.outcome, PlanningOutcome::AwaitingInput);
    assert!(ctx.plan_session.recovery.model_selection_pending);
    assert_eq!(ctx.plan_session.recovery.requests_used, 3);
    assert_eq!(ctx.plan_session.recovery.cycle, 0);
    assert_eq!(popup.await.expect("model popup interaction"), 1);
    assert!(
        !events(&harness)
            .iter()
            .any(|event| matches!(event, ThreadEvent::PlanApprovalRequested(_)))
    );
}

#[tokio::test]
async fn planning_recovery_runtime_cancelled_or_unavailable_modal_stays_idle_without_showing_again() {
    for unavailable in [false, true] {
        let mut backing = TestTurnProcessingBacking::new(8).await;
        backing.activate_planning_for_test();
        let harness = backing.enable_harness_emitter();
        let mut modal = RecoveryModalFixture::new();
        if unavailable {
            drop(modal.events.take());
        } else {
            modal
                .events
                .as_ref()
                .expect("modal event sender")
                .send(InlineEvent::Transient(TransientEvent::Cancelled))
                .expect("cancel recovery modal");
        }
        let mut ctx = backing.turn_processing_context();
        ctx.plan_session.recovery = exhausted_recovery_snapshot();
        ctx.handle = &modal.handle;
        ctx.session = &mut modal.session;
        ctx.renderer = &mut modal.renderer;
        for _ in 0..2 {
            let outcome = ctx
                .await_planning_recovery_input("recovery exhausted")
                .await
                .expect("idle input handoff");
            assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Blocked { .. })));
            assert_eq!(ctx.plan_session.recovery.outcome, PlanningOutcome::AwaitingInput);
            assert_eq!(ctx.plan_session.recovery.requests_used, 3);
            assert_eq!(ctx.plan_session.recovery.cycle, 0);
            assert!(!ctx.plan_session.recovery.model_selection_pending);
        }
        assert_eq!(modal.wizard_count(), 1, "the same incident may present its input prompt only once");
        assert!(
            !events(&harness)
                .iter()
                .any(|event| matches!(event, ThreadEvent::PlanApprovalRequested(_)))
        );
    }
}
