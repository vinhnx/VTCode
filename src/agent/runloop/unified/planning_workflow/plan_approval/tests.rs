use std::sync::Arc;

use tokio::sync::{Notify, mpsc};

use super::{
    PlanApprovalRoute, PlanConfirmationOutcome, build_plan_confirmation_request_with_context,
    execute_plan_confirmation, execute_plan_confirmation_with_context, plan_approval_route,
    plan_confirmation_submission_to_outcome, render_plan_summary, render_structured_plan,
};
use crate::agent::runloop::unified::state::CtrlCState;
use vtcode_core::config::EditorToolConfig;
use vtcode_ui::tui::app::{
    InlineCommand, InlineEvent, InlineHandle, InlineListSelection, InlineMessageKind, InlineSession,
    ListOverlayRequest, TransientEvent, TransientHotkeyAction, TransientRequest, TransientSubmission,
};
use vtcode_ui::tui::app::{PlanContent, PlanPhase, PlanStep};

fn sample_plan() -> PlanContent {
    PlanContent {
        title: "Add retry to synthesize".to_string(),
        summary: "Make plan-mode synthesize resilient to transient errors.".to_string(),
        file_path: Some("docs/plan.md".to_string()),
        phases: vec![PlanPhase {
            name: "Phase 1: Resilience".to_string(),
            completed: false,
            steps: vec![
                PlanStep {
                    number: 1,
                    description: "Wrap generate in retry".to_string(),
                    details: Some("Use RetryPolicy::default()".to_string()),
                    files: vec!["src/a.rs".to_string()],
                    completed: false,
                },
                PlanStep {
                    number: 2,
                    description: "Add tests".to_string(),
                    details: None,
                    files: vec![],
                    completed: false,
                },
            ],
        }],
        open_questions: vec!["Should we cap retries?".to_string()],
        raw_content: "RAW fallback content".to_string(),
        total_steps: 2,
        completed_steps: 0,
    }
}

// --- B: render_structured_plan ----------------------------------------

#[test]
fn approval_route_separates_inline_headless_and_automatic_policies() {
    assert_eq!(plan_approval_route(true, true, false, false), PlanApprovalRoute::Inline);
    assert_eq!(plan_approval_route(true, false, false, false), PlanApprovalRoute::Headless);
    assert_eq!(plan_approval_route(true, false, true, false), PlanApprovalRoute::Automatic);
    assert_eq!(plan_approval_route(false, true, false, false), PlanApprovalRoute::Automatic);
}

#[test]
fn render_structured_plan_prefers_phases_over_raw() {
    let lines = render_structured_plan(&sample_plan());
    let joined = lines.join("\n");
    assert!(joined.contains("## Phase 1: Resilience"));
    assert!(joined.contains("[ ] 1 Wrap generate in retry"));
    assert!(joined.contains("Use RetryPolicy::default()"));
    assert!(joined.contains("files: src/a.rs"));
    assert!(joined.contains("Open questions:"));
    assert!(joined.contains("Should we cap retries?"));
    assert!(!joined.contains("RAW fallback content"), "structured phases must take precedence over raw_content");
}

#[test]
fn render_structured_plan_falls_back_to_raw_content() {
    let mut plan = sample_plan();
    plan.phases = vec![];
    let lines = render_structured_plan(&plan);
    let joined = lines.join("\n");
    assert!(joined.contains("RAW fallback content"));
    assert!(joined.contains("Make plan-mode synthesize resilient"));
    assert!(!joined.contains("## Phase 1"));
}

#[test]
fn render_structured_plan_falls_back_to_title_when_empty() {
    let plan = PlanContent {
        title: "Only a title".to_string(),
        summary: String::new(),
        file_path: None,
        phases: vec![],
        open_questions: vec![],
        raw_content: String::new(),
        total_steps: 0,
        completed_steps: 0,
    };
    assert_eq!(render_structured_plan(&plan), vec!["Only a title".to_string(), String::new()]);
}

#[test]
fn plan_summary_preview_keeps_sparse_markdown_decision_complete() {
    let plan = PlanContent::from_markdown(
        "Implementation Plan".to_string(),
        "Summary\nFocus the launch-time improvement on startup latency.\n\n1. Instrument startup timing -> src/startup.rs -> verify: add logs.\n2. Move refresh off the critical path -> src/update.rs -> verify: UI renders first.\n3. Prefer the cached notice -> src/update.rs -> verify: stale refresh is deferred.\n4. Add a bounded fallback -> src/startup.rs -> verify: startup never waits.\n5. Validate with focused tests -> src/startup.rs -> verify: nextest passes.\n\nValidation\n- cargo check --locked",
        Some(".vtcode/plans/startup.md".to_string()),
    );

    let lines = render_plan_summary(&plan);
    assert_eq!(plan.summary, "Focus the launch-time improvement on startup latency.");
    assert_eq!(lines.len(), 5);
    assert!(lines[0].starts_with("Summary: Focus the launch-time improvement"));
    assert!(lines.iter().any(|line| line.starts_with("1. Instrument startup timing")));
    assert!(lines.iter().any(|line| line == "… and 2 more plan steps"));
    assert!(!lines.iter().any(|line| line == "Summary"));
}

#[test]
fn plan_summary_overview_shows_full_text_within_modal_budget() {
    let long_summary = "Fix vtcode analyze so it runs non-interactively with auto-allowed tools, correct step parsing, and bounded verification across startup and update paths.";
    let plan = PlanContent {
        title: "Implementation Plan".to_string(),
        summary: long_summary.to_string(),
        file_path: Some(".vtcode/plans/analyze.md".to_string()),
        phases: vec![],
        open_questions: vec![],
        raw_content: format!("{long_summary}\n\n1. First step\n2. Second step"),
        total_steps: 2,
        completed_steps: 0,
    };

    let lines = render_plan_summary(&plan);
    assert!(lines.len() <= 5, "summary plus steps must fit the modal budget: {lines:?}");
    let summary_line = lines
        .iter()
        .find(|line| line.starts_with("Summary:"))
        .expect("summary overview");
    assert!(
        summary_line.contains(long_summary),
        "overview must show the full summary without mid-text truncation: {summary_line}"
    );
    assert!(!summary_line.ends_with('…'), "summary must not be elided: {summary_line}");
    assert!(summary_line.contains("non-interactively"), "overview must preserve the change intent");
}

#[test]
fn plan_summary_steps_keep_full_text_without_elision() {
    let plan = PlanContent::from_markdown(
        "Implementation Plan".to_string(),
        "Summary\nImprove VT Code's diff engine across its four layers: core computation, turn aggregation, event contract, and computation hardening.\n\n1. Make TurnDiffTracker bounded and deterministic: sort paths for stable output ordering.\n2. Extend the authoritative event contract with optional unified diff fields.\n3. Harden vtcode-diff computation with a binary-content guard.",
        Some(".vtcode/plans/diff.md".to_string()),
    );

    let lines = render_plan_summary(&plan);
    let step_line = lines.iter().find(|line| line.starts_with("1.")).expect("first step");
    assert!(
        step_line.contains("sort paths for stable output ordering"),
        "steps must not be truncated mid-sentence: {step_line}"
    );
    assert!(!step_line.ends_with('…'), "steps must not be elided: {step_line}");
}

// --- C: approval outcomes (submission mapping, request items) ------

#[test]
fn plan_confirmation_submission_maps_switch_outcomes() {
    assert_eq!(
        plan_confirmation_submission_to_outcome(&TransientSubmission::Selection(
            InlineListSelection::PlanApprovalSwitchBuild
        )),
        Some(PlanConfirmationOutcome::SwitchBuild)
    );
    assert_eq!(
        plan_confirmation_submission_to_outcome(&TransientSubmission::Selection(
            InlineListSelection::PlanApprovalSwitchAuto
        )),
        Some(PlanConfirmationOutcome::SwitchAuto)
    );
    assert_eq!(
        plan_confirmation_submission_to_outcome(&TransientSubmission::Selection(
            InlineListSelection::PlanApprovalExecute
        )),
        Some(PlanConfirmationOutcome::Execute)
    );
    assert_eq!(
        plan_confirmation_submission_to_outcome(&TransientSubmission::Selection(
            InlineListSelection::PlanApprovalFreshContext
        )),
        Some(PlanConfirmationOutcome::FreshContext)
    );
    assert_eq!(
        plan_confirmation_submission_to_outcome(&TransientSubmission::Selection(
            InlineListSelection::PlanApprovalAutoAccept
        )),
        Some(PlanConfirmationOutcome::AutoAccept)
    );
    assert_eq!(
        plan_confirmation_submission_to_outcome(&TransientSubmission::Selection(
            InlineListSelection::PlanApprovalEditPlan
        )),
        Some(PlanConfirmationOutcome::EditPlan)
    );
    // Unrecognized selection cancels.
    assert_eq!(
        plan_confirmation_submission_to_outcome(&TransientSubmission::Selection(InlineListSelection::ConfigAction(
            "x".to_string()
        ))),
        Some(PlanConfirmationOutcome::Cancel)
    );
    assert_eq!(
        plan_confirmation_submission_to_outcome(&TransientSubmission::Hotkey(TransientHotkeyAction::LaunchEditor)),
        Some(PlanConfirmationOutcome::EditPlan)
    );
}

#[test]
fn plan_confirmation_request_has_build_auto_and_revision_choices() {
    let req = build_plan_confirmation_request_with_context(&sample_plan(), false, 7);
    let ListOverlayRequest { items, .. } = match req {
        TransientRequest::List(list) => list,
        _ => panic!("expected a list overlay request"),
    };
    let selections: Vec<InlineListSelection> = items.into_iter().filter_map(|item| item.selection).collect();
    assert!(selections.iter().any(|s| matches!(s, InlineListSelection::PlanApprovalExecute)));
    assert!(
        selections
            .iter()
            .any(|s| matches!(s, InlineListSelection::PlanApprovalFreshContext))
    );
    assert!(
        selections
            .iter()
            .any(|s| matches!(s, InlineListSelection::PlanApprovalEditPlan))
    );
    assert!(
        selections
            .iter()
            .any(|s| matches!(s, InlineListSelection::PlanApprovalSwitchAuto))
    );
    assert_eq!(selections.len(), 4);
    let TransientRequest::List(request) = build_plan_confirmation_request_with_context(&sample_plan(), false, 7) else {
        panic!("expected list request");
    };
    assert_eq!(request.items[1].subtitle.as_deref(), Some("Fresh thread. Context: 7% used."));
}

#[test]
fn plan_confirmation_request_carries_normalized_plan_markdown_for_hosts() {
    let mut plan = sample_plan();
    plan.raw_content = "<proposed_plan>\n# Retry plan\n\nWrap generate in retry.\n</proposed_plan>".to_string();
    let TransientRequest::List(request) = build_plan_confirmation_request_with_context(&plan, false, 7) else {
        panic!("expected list request");
    };
    let markdown = request.plan_markdown.expect("hosts receive the full plan");
    assert!(markdown.contains("Wrap generate in retry."), "plan body survives: {markdown:?}");
    assert!(
        !markdown.contains("<proposed_plan>"),
        "wrappers are stripped like the transcript copy: {markdown:?}"
    );

    // Empty-after-cleanup yields None so clients hide expansion UI.
    plan.raw_content = "   ".to_string();
    let TransientRequest::List(request) = build_plan_confirmation_request_with_context(&plan, false, 7) else {
        panic!("expected list request");
    };
    assert_eq!(request.plan_markdown, None);
}

#[tokio::test]
async fn execute_plan_confirmation_emits_confirmation_overlay() {
    let (command_tx, mut command_rx) = mpsc::unbounded_channel();
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(command_tx);
    let mut session = InlineSession {
        handle: handle.clone(),
        events: event_rx,
        worker: None,
    };
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());

    event_tx
        .send(InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Selection(
            InlineListSelection::PlanApprovalFreshContext,
        ))))
        .expect("send confirmation selection");

    let outcome = execute_plan_confirmation(&handle, &mut session, sample_plan(), false, &ctrl_c_state, &ctrl_c_notify)
        .await
        .expect("confirmation overlay result");
    assert_eq!(outcome, PlanConfirmationOutcome::FreshContext);

    let mut transcript_messages = Vec::new();
    let mut saw_markdown_append_line = false;
    let mut request = None;
    while let Ok(command) = command_rx.try_recv() {
        match command {
            InlineCommand::AppendPastedMessage { kind: InlineMessageKind::Agent, text, .. } => {
                transcript_messages.push(text);
            }
            InlineCommand::AppendLine { kind: InlineMessageKind::Agent, segments } => {
                saw_markdown_append_line = true;
                let text: String = segments.into_iter().map(|seg| seg.text).collect();
                transcript_messages.push(text);
            }
            InlineCommand::ShowTransient { request: req }
                // Overlay-first: the gate is published before the heavy
                // transcript render, so the request may arrive before or
                // after transcript rows. Keep the first gate.
                if request.is_none() => {
                    request = Some(req);
                }
            _ => {}
        }
    }
    let request = request.expect("confirmation overlay request");
    assert!(
        saw_markdown_append_line,
        "plan body must go through the markdown pipeline (AppendLine), not raw pasted text"
    );
    assert!(transcript_messages.iter().any(|text| text.contains("RAW fallback content")));
    assert!(
        transcript_messages.iter().all(|text| !text.contains("<proposed_plan>")),
        "raw plan wrappers must never leak into the transcript: {transcript_messages:?}"
    );

    match *request {
        TransientRequest::List(request) => {
            assert_eq!(request.title, "Ready to code?");
            assert!(request.lines.iter().any(|line| line.contains("A plan is ready")));
            assert!(
                request
                    .items
                    .iter()
                    .any(|item| { item.selection == Some(InlineListSelection::PlanApprovalFreshContext) })
            );
        }
        other => panic!("expected plan confirmation list, got {other:?}"),
    }
}

#[tokio::test]
async fn execute_plan_confirmation_edit_selection_returns_to_planning() {
    let (command_tx, mut command_rx) = mpsc::unbounded_channel();
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(command_tx);
    let mut session = InlineSession {
        handle: handle.clone(),
        events: event_rx,
        worker: None,
    };
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());

    event_tx
        .send(InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Selection(
            InlineListSelection::PlanApprovalEditPlan,
        ))))
        .expect("send edit selection");

    let outcome = execute_plan_confirmation(&handle, &mut session, sample_plan(), false, &ctrl_c_state, &ctrl_c_notify)
        .await
        .expect("edit confirmation result");
    assert_eq!(outcome, PlanConfirmationOutcome::EditPlan);

    let request = loop {
        let command = command_rx.try_recv().expect("confirmation command");
        if let InlineCommand::ShowTransient { request } = command {
            break request;
        }
    };
    assert!(matches!(
        *request,
        TransientRequest::List(ListOverlayRequest { items, .. })
            if items.iter().any(|item| item.selection == Some(InlineListSelection::PlanApprovalEditPlan))
    ));
    loop {
        match command_rx.try_recv() {
            Ok(InlineCommand::CloseTransient) => break,
            Ok(_) => {}
            Err(error) => panic!("expected close command, got {error:?}"),
        }
    }
}

#[tokio::test]
async fn execute_plan_confirmation_cancellation_is_not_approval() {
    let (command_tx, mut command_rx) = mpsc::unbounded_channel();
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(command_tx);
    let mut session = InlineSession {
        handle: handle.clone(),
        events: event_rx,
        worker: None,
    };
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());

    event_tx
        .send(InlineEvent::Transient(TransientEvent::Cancelled))
        .expect("send confirmation cancellation");

    let outcome = execute_plan_confirmation(&handle, &mut session, sample_plan(), false, &ctrl_c_state, &ctrl_c_notify)
        .await
        .expect("cancelled confirmation result");
    assert_eq!(outcome, PlanConfirmationOutcome::Cancel);

    loop {
        match command_rx.try_recv() {
            Ok(InlineCommand::CloseTransient) => break,
            Ok(_) => {}
            Err(error) => panic!("expected close command, got {error:?}"),
        }
    }
}

#[tokio::test]
async fn execute_plan_confirmation_editor_hotkey_reshows_overlay_without_edit_command() {
    let (command_tx, mut command_rx) = mpsc::unbounded_channel();
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(command_tx);
    let mut session = InlineSession {
        handle: handle.clone(),
        events: event_rx,
        worker: None,
    };
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());

    event_tx
        .send(InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Hotkey(
            TransientHotkeyAction::LaunchEditor,
        ))))
        .expect("send editor hotkey");
    event_tx
        .send(InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Selection(
            InlineListSelection::PlanApprovalExecute,
        ))))
        .expect("send approval after editor hotkey");

    // Disable the external editor so the hotkey exercises the fallback path
    // without spawning a real editor process.
    let editor = EditorToolConfig { enabled: false, ..EditorToolConfig::default() };
    let (outcome, edited_plan) = execute_plan_confirmation_with_context(
        &handle,
        &mut session,
        sample_plan(),
        false,
        0,
        &editor,
        std::path::Path::new("."),
        &ctrl_c_state,
        &ctrl_c_notify,
    )
    .await
    .expect("editor hotkey then approval result");

    assert_eq!(outcome, PlanConfirmationOutcome::Execute);
    assert!(edited_plan.is_none(), "disabled editor must not produce an edited plan");

    let mut show_transient_count = 0usize;
    let mut saw_edit_input = false;
    while let Ok(command) = command_rx.try_recv() {
        match command {
            InlineCommand::ShowTransient { .. } => show_transient_count += 1,
            InlineCommand::SetInput(input) if input == "/edit" => saw_edit_input = true,
            _ => {}
        }
    }
    assert!(
        show_transient_count >= 2,
        "Ctrl+G must re-show the approval overlay instead of dismissing it (saw {show_transient_count})"
    );
    assert!(!saw_edit_input, "Ctrl+G must open the plan file directly, not seed a bare /edit command");
}
