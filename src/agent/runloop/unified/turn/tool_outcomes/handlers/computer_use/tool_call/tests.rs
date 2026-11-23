use super::super::execution::{NativeTurnResults, RedactedTurnSummary, TeardownOutcome};
use super::*;
use vtcode_core::llm::provider::{
    FinishReason, LLMError, LLMProvider, LLMRequest, LLMResponse, NativeCall, NativeCallBody, NativeProvider,
    NativeResponse,
};

struct ScriptedNativeProvider;

#[async_trait::async_trait]
impl LLMProvider for ScriptedNativeProvider {
    fn name(&self) -> &str {
        "scripted-native"
    }

    fn supports_streaming(&self) -> bool {
        false
    }

    async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
        Err(LLMError::Provider {
            message: "computer turns generate natives, never chat completions".to_string(),
            metadata: None,
        })
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["scripted-native-model".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }

    fn supports_computer_use(&self) -> bool {
        true
    }

    async fn generate_native(&self, _request: LLMRequest) -> Result<NativeResponse, LLMError> {
        Ok(NativeResponse {
            content: Some("acting now".to_string()),
            reasoning: None,
            model: "scripted-native-model".to_string(),
            usage: None,
            finish_reason: FinishReason::ToolCalls,
            tool_calls: Vec::new(),
            native_calls: vec![NativeCall {
                id: "shot".to_string(),
                provider: NativeProvider::Anthropic,
                index: 1,
                body: NativeCallBody::Anthropic {
                    name: "screenshot".to_string(),
                    action: Ok(vtcode_computer_use::Action::Screenshot),
                },
            }],
            assistant_blocks: Vec::new(),
            request_id: None,
            organization_id: None,
        })
    }
}

fn outcome(kind: ComputerTurnOutcomeKind) -> ComputerTurnOutcome {
    ComputerTurnOutcome { kind, model: "test-model".to_string(), usage: None }
}

#[test]
fn computer_outcome_mapping_keeps_denials_out_of_failures() {
    for kind in [
        ComputerTurnOutcomeKind::ApprovalDenied,
        ComputerTurnOutcomeKind::Cancelled,
    ] {
        let mapped = map_computer_outcome(&outcome(kind));
        assert!(matches!(mapped.status, ToolExecutionStatus::Cancelled), "denials must not trip failure tracking");
        assert!(!mapped.status.is_failure_like());
    }
}

#[test]
fn computer_outcome_mapping_reports_completion_and_refusals() {
    let summary = RedactedTurnSummary {
        target_kind: "browser",
        target: "fixture-app".to_string(),
        recipient: "test-model (test)".to_string(),
        action_kinds: vec!["screenshot".to_string()],
        action_count: 1,
        executed_units: 1,
        review: None,
        teardown_trigger: "finished",
        teardown: TeardownOutcome::Removed,
    };
    let completed = map_computer_outcome(&outcome(ComputerTurnOutcomeKind::Completed {
        results: NativeTurnResults::OpenAi(vec![serde_json::json!({"call_id": "call_1"})]),
        summary,
    }));
    match &completed.status {
        ToolExecutionStatus::Success { output, command_success, .. } => {
            assert!(command_success);
            assert!(
                output
                    .as_str()
                    .is_some_and(|text| text.contains("completed (test-model): 1 units"))
            );
            assert!(output.as_str().is_some_and(|text| text.contains("call call_1: ok")));
        }
        status => panic!("completed turn must succeed, got {status:?}"),
    }
    assert!(!completed.status.is_failure_like());

    for (kind, needle) in [
        (ComputerTurnOutcomeKind::Disabled { reason: "disabled" }, "did not run: disabled."),
        (ComputerTurnOutcomeKind::BudgetExceeded { proposed: 60, max: 50 }, "exceed the budget of 50"),
        (
            ComputerTurnOutcomeKind::ReviewHalted {
                verdict: super::super::review::BatchReviewVerdict::Halt,
            },
            "SUSPECT",
        ),
    ] {
        let mapped = map_computer_outcome(&outcome(kind));
        match &mapped.status {
            ToolExecutionStatus::Failure { error } => assert!(error.message.contains(needle), "{needle}"),
            status => panic!("refusals must fail loudly, got {status:?}"),
        }
        assert!(mapped.status.is_failure_like());
    }
}

#[tokio::test]
async fn tool_call_without_a_task_fails_before_any_model_call() {
    let mut backing = super::super::super::tests::TestContextBacking::new().await;
    let mut ctx = backing.turn_processing_context();
    for args in [serde_json::json!({}), serde_json::json!({"task": "  "})] {
        let outcome = handle_computer_use_tool_call(&mut ctx, &args, "test-call")
            .await
            .expect("tool result");
        match &outcome.status {
            ToolExecutionStatus::Failure { error } => {
                assert!(error.message.contains("non-empty task"), "{}", error.message)
            }
            status => panic!("missing task must fail loudly, got {status:?}"),
        }
    }
}

// The headless harness auto-allows before reaching the interactive
// prompt, so the scope-prompt skip itself is not headless-observable
// (verified by mutation: proceed-without-session-skip passes with and
// without the skip). The headless-pinned properties are that validation
// proceeds by default (usability pin, not a skip proof) and that denies
// still apply, since the skip sits after every deny check.
#[tokio::test]
async fn computer_use_validation_proceeds_by_default() {
    let mut backing = super::super::super::tests::TestContextBacking::new().await;
    let mut ctx = backing.turn_processing_context();
    ctx.session.set_skip_confirmations(false);
    let result = super::super::super::validation::validate_tool_call(
        &mut ctx,
        "proceed-1",
        "computer_use",
        &serde_json::json!({"task": "take a screenshot"}),
    )
    .await
    .expect("validate");
    assert!(
        !matches!(result, super::super::super::types::ValidationResult::Blocked),
        "default validation must proceed for computer_use"
    );
}

#[tokio::test]
async fn computer_use_flagged_scope_review_proceeds_to_batch() {
    // The gateway always flags computer_use (destructive
    // classification); validation must still proceed so the review
    // happens once, at the batch approval. Two-part proof: the
    // premise (gateway flags here) and the reroute itself — without
    // it the headless prompt resolves to Exit (the test session has
    // no event sender), so asserting Proceed fails on a reverted
    // reroute. Single-prompt behavior itself is verified
    // interactively.
    use crate::agent::runloop::unified::tool_call_safety::invocation_id_from_call_id;
    use crate::agent::runloop::unified::tool_pipeline::validation::{
        SafetyValidationFailure, validate_tool_call_safety,
    };

    let mut backing = super::super::super::tests::TestContextBacking::new().await;
    let mut ctx = backing.turn_processing_context();
    ctx.session.set_skip_confirmations(false);
    let args = serde_json::json!({"task": "take a screenshot"});
    let safety = validate_tool_call_safety(
        ctx.safety_validator,
        "computer_use",
        &args,
        invocation_id_from_call_id("reroute-premise"),
    )
    .await;
    assert!(
        matches!(safety, Err(SafetyValidationFailure::NeedsApproval(_))),
        "gateway must flag computer_use (destructive classification)"
    );
    let result = super::super::super::validation::validate_tool_call(&mut ctx, "reroute-1", "computer_use", &args)
        .await
        .expect("validate");
    assert!(
        matches!(result, super::super::super::types::ValidationResult::Proceed(_)),
        "flagged computer_use must proceed past validation so the review reroutes to the batch approval"
    );
}

#[tokio::test]
async fn computer_use_cached_denial_still_blocks() {
    let mut backing = super::super::super::tests::TestContextBacking::new().await;
    let mut ctx = backing.turn_processing_context();
    ctx.tool_permission_cache
        .write()
        .await
        .cache_grant_tool("computer_use", vtcode_core::acp::PermissionGrant::Denied);
    let result = super::super::super::validation::validate_tool_call(
        &mut ctx,
        "deny-1",
        "computer_use",
        &serde_json::json!({"task": "take a screenshot"}),
    )
    .await
    .expect("validate");
    assert!(
        matches!(result, super::super::super::types::ValidationResult::Blocked),
        "cached denial must still block computer_use"
    );
}

#[tokio::test]
async fn computer_use_full_auto_allowlist_denies_at_validation() {
    // The tool stays visible under full-auto (allow-list visibility
    // bypass) so interactive steering keeps working — but execution
    // must still deny when the allow-list omits it. Fail-closed.
    let mut backing = super::super::super::tests::TestContextBacking::new().await;
    let mut ctx = backing.turn_processing_context();
    ctx.tool_registry
        .enable_full_auto_permission(&["exec_command".to_string()])
        .await;
    let result = super::super::super::validation::validate_tool_call(
        &mut ctx,
        "deny-2",
        "computer_use",
        &serde_json::json!({"task": "take a screenshot"}),
    )
    .await
    .expect("validate");
    assert!(
        matches!(result, super::super::super::types::ValidationResult::Blocked),
        "full-auto allow-list must deny computer_use at validation"
    );
}

#[tokio::test]
async fn computer_use_prompts_exactly_once_across_validation_and_batch() {
    // Single-prompt proof: validation must skip the scope prompt
    // (reroute + skip) while the batch approval still prompts. The
    // scripted session approves the first prompt; the sender is then
    // dropped so any second prompt exits fast instead of hanging.
    // Recorder counts approvals, and counts stay under the learned
    // auto-approve threshold, so the count equals prompts shown: 0
    // means the batch review was skipped (unsafe), 2 means the scope
    // prompt leaked back. The post-approval outcome is intentionally
    // unasserted beyond not-cancelled (docker presence varies).
    use vtcode_ui::tui::app::{
        InlineEvent, InlineHandle, InlineListSelection, InlineSession, TransientEvent, TransientSubmission,
    };

    let mut backing = super::super::super::tests::TestContextBacking::new().await;
    let (command_tx, _command_rx) = tokio::sync::mpsc::unbounded_channel();
    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    event_tx
        .send(InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Selection(
            InlineListSelection::ToolApproval(true),
        ))))
        .expect("queue approval");
    drop(event_tx);
    let mut approving = InlineSession {
        handle: InlineHandle::new_for_tests(command_tx),
        events: event_rx,
        worker: None,
    };
    let mut ctx = backing.turn_processing_context();
    ctx.session = &mut approving;
    ctx.session.set_skip_confirmations(false);
    let target = tempfile::tempdir().expect("target dir");
    std::fs::write(target.path().join("index.html"), "<h1>preview</h1>").expect("seed target");
    let mut vt_cfg = vtcode_core::config::loader::VTCodeConfig::default();
    vt_cfg.computer_use.enabled = true;
    vt_cfg.computer_use.browser_build = Some(target.path().to_path_buf());
    ctx.vt_cfg = Some(&vt_cfg);
    *ctx.provider_client = Box::new(ScriptedNativeProvider);
    let args = serde_json::json!({"task": "take a screenshot of the preview"});
    let validation =
        super::super::super::validation::validate_tool_call(&mut ctx, "prompt-count-1", "computer_use", &args)
            .await
            .expect("validate");
    assert!(
        matches!(validation, super::super::super::types::ValidationResult::Proceed(_)),
        "validation must proceed past the skipped scope prompt"
    );
    let outcome = handle_computer_use_tool_call(&mut ctx, &args, "test-call")
        .await
        .expect("tool result");
    assert!(
        !matches!(outcome.status, ToolExecutionStatus::Cancelled),
        "consumed approval must proceed past the batch, got {:?}",
        outcome.status
    );
    let prompts = ctx.approval_recorder.get_approval_count("computer_use").await;
    assert_eq!(prompts, 1, "exactly one prompt must show per computer turn");
}

#[tokio::test]
async fn tool_call_headless_denial_cancels_before_launch() {
    let mut backing = super::super::super::tests::TestContextBacking::new().await;
    let mut ctx = backing.turn_processing_context();
    let mut vt_cfg = vtcode_core::config::loader::VTCodeConfig::default();
    vt_cfg.computer_use.enabled = true;
    vt_cfg.computer_use.browser_build = Some(std::path::PathBuf::from("/tmp/fixture-build"));
    ctx.vt_cfg = Some(&vt_cfg);
    *ctx.provider_client = Box::new(ScriptedNativeProvider);
    let outcome =
        handle_computer_use_tool_call(&mut ctx, &serde_json::json!({"task": "take a screenshot"}), "test-call")
            .await
            .expect("tool result");
    // Headless permission flow denies: the turn must end cancelled with
    // nothing launched (no docker needed to prove it).
    assert!(
        matches!(outcome.status, ToolExecutionStatus::Cancelled),
        "headless denial must cancel, got {:?}",
        outcome.status
    );
    assert!(!outcome.status.is_failure_like());
}

#[test]
fn external_address_matrix_covers_schemes_ips_domains_and_near_misses() {
    let cases: &[(&str, bool)] = &[
        ("screenshot https://apple.com", true),
        ("open HTTP://X.COM/a", true),
        ("go to https://example.org/a/b?q=1#x", true),
        ("screenshot apple.com", true),
        ("'apple.com.'", true),
        ("(see example.io!)", true),
        ("www.example.com/x", true),
        ("ping 192.168.1.5", true),
        ("open http://[::1]:3000/", true),
        ("screenshot localhost:3000", true),
        ("check LOCALHOST", true),
        ("take a screenshot", false),
        ("check index.html renders", false),
        ("runner.mjs threw", false),
        ("version 1.64.1 broke", false),
        ("open report.pdf", false),
        ("check the intranet", false),
        ("screenshot google", false),
        // Known over-match, documented on the detector: a preview
        // task that merely mentions a domain still refuses.
        ("type example.com into the field", true),
    ];
    for (task, expected) in cases {
        assert_eq!(task_requests_external_address(task), *expected, "task={task:?}");
    }
}

struct NeverCalledProvider;

#[async_trait::async_trait]
impl LLMProvider for NeverCalledProvider {
    fn name(&self) -> &str {
        "never-called"
    }

    fn supports_streaming(&self) -> bool {
        false
    }

    async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
        panic!("refused turn must never reach the provider");
    }

    fn supported_models(&self) -> Vec<String> {
        vec![]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        panic!("refused turn must never reach the provider");
    }
}

#[tokio::test]
async fn external_address_task_refuses_before_provider_config_and_approval() {
    let mut backing = super::super::super::tests::TestContextBacking::new().await;
    let mut ctx = backing.turn_processing_context();
    *ctx.provider_client = Box::new(NeverCalledProvider);
    let outcome = handle_computer_use_tool_call(
        &mut ctx,
        &serde_json::json!({"task": "screenshot https://apple.com"}),
        "test-call",
    )
    .await
    .expect("tool result");
    assert!(
        matches!(outcome.status, ToolExecutionStatus::Failure { .. }),
        "refusal must fail closed, got {:?}",
        outcome.status
    );
}

#[tokio::test]
async fn tool_call_full_auto_denial_cancels_before_launch() {
    // Full-auto never auto-approves computer turns: with an active
    // allow-list that omits the tool, the batch approval denies and
    // the turn cancels without launching anything.
    let mut backing = super::super::super::tests::TestContextBacking::new().await;
    let mut ctx = backing.turn_processing_context();
    let mut vt_cfg = vtcode_core::config::loader::VTCodeConfig::default();
    vt_cfg.computer_use.enabled = true;
    vt_cfg.computer_use.browser_build = Some(std::path::PathBuf::from("/tmp/fixture-build"));
    ctx.vt_cfg = Some(&vt_cfg);
    *ctx.provider_client = Box::new(ScriptedNativeProvider);
    ctx.tool_registry
        .enable_full_auto_permission(&["exec_command".to_string()])
        .await;
    let outcome =
        handle_computer_use_tool_call(&mut ctx, &serde_json::json!({"task": "take a screenshot"}), "test-call")
            .await
            .expect("tool result");
    assert!(
        matches!(outcome.status, ToolExecutionStatus::Cancelled),
        "full-auto denial must cancel, got {:?}",
        outcome.status
    );
    assert!(!outcome.status.is_failure_like());
}

#[test]
fn computer_use_footer_status_marks_running_turns() {
    // Producer/consumer contract: the footer text the wrapper sets must
    // satisfy the shared running predicate the TUI input reads back.
    for args in [
        serde_json::json!({"task": "take a screenshot"}),
        serde_json::json!({"action_count": 2, "action_kinds": ["click"]}),
    ] {
        let status = running_status_line(&args);
        assert!(
            vtcode_commons::tool_headlines::status_is_computer_use_running(&status),
            "{status:?} must mark a running turn"
        );
        assert!(status.starts_with(vtcode_commons::tool_headlines::COMPUTER_USE_RUNNING_STATUS_PREFIX));
    }
}

#[test]
fn merged_event_args_keep_the_task_headline_with_batch_details() {
    use vtcode_commons::tool_headlines::describe_computer_use;

    let approval = ApprovalRequest {
        target: super::super::ComputerUseTarget::Browser {
            build_dir: std::path::PathBuf::from("/approved/build"),
        },
        recipient: "test-model (test)".to_string(),
        call_ids: vec!["c1".to_string()],
        action_kinds: vec!["click".to_string(), "screenshot".to_string()],
        action_count: 2,
    };
    let merged = merged_event_args("open settings", &approval);
    // The row keeps its task headline after the approval refresh…
    assert_eq!(describe_computer_use(&merged), ("Computer use: open settings".to_string(), &["task"][..]));
    // …while the batch shape rides along for the detail lines.
    assert_eq!(merged["action_count"], serde_json::json!(2));
    assert_eq!(merged["target_kind"], serde_json::json!("browser"));
    assert_eq!(merged["screenshot_recipient"], serde_json::json!("test-model (test)"));
}

#[test]
fn complete_outer_row_emits_one_invocation_and_output_verdict() {
    use vtcode_core::exec::events::ToolCallStatus;

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    let emitter = HarnessEventEmitter::new(path.clone()).unwrap();
    let args = serde_json::json!({"task": "take a screenshot"});
    complete_outer_row(Some(&emitter), "outer-item", "outer-call", &args, ToolCallStatus::Completed, "done");
    // Asymmetric: a second completion with a failure status proves the
    // verdict rides the status, not the call shape.
    complete_outer_row(Some(&emitter), "outer-item-2", "outer-call-2", &args, ToolCallStatus::Failed, "denied");

    let events = std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(events.len(), 4);
    let kinds = events
        .iter()
        .map(|event| event["event"]["type"].as_str().unwrap_or(""))
        .collect::<Vec<_>>();
    assert_eq!(kinds, ["item.completed", "item.completed", "item.completed", "item.completed"]);
    assert_eq!(events[0]["event"]["item"]["id"], serde_json::json!("outer-item"));
    assert_eq!(events[1]["event"]["item"]["id"], serde_json::json!("outer-item:output"));
    assert_eq!(events[1]["event"]["item"]["output"], serde_json::json!("done"));
    assert_eq!(events[1]["event"]["item"]["status"], serde_json::json!("completed"));
    assert_eq!(events[3]["event"]["item"]["output"], serde_json::json!("denied"));
    assert_eq!(events[3]["event"]["item"]["status"], serde_json::json!("failed"));
    // No emitter, no events, no panic: headless paths stay quiet.
    complete_outer_row(None, "outer-item", "outer-call", &args, ToolCallStatus::Completed, "done");
}
