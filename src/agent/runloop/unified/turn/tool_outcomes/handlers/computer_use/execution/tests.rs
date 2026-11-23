use super::*;
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use super::super::NATIVE_SANDBOX_BRIEFING;
use serde_json::Value;
use vtcode_computer_use::browser::container::docker_available;
use vtcode_computer_use::{Action, MouseButton, Observation, Point, Screenshot};
use vtcode_core::llm::provider::{
    ChoiceDecisionAnswer, ChoiceDecisionRequest, ChoiceDecisionResponse, FinishReason, LLMError, LLMProvider,
    LLMRequest, LLMResponse, NativeCall, NativeCallBody, NativeProvider, NativeResponse, ToolCall, Usage,
};

struct ScriptedProvider {
    name: &'static str,
    computer_use: bool,
    decisions: bool,
    native: fn() -> Result<NativeResponse, LLMError>,
    choice: Option<&'static str>,
    generate_calls: Mutex<usize>,
    decisions_calls: Mutex<usize>,
    decisions_inputs: Mutex<Vec<String>>,
}

impl ScriptedProvider {
    fn canned() -> Result<NativeResponse, LLMError> {
        Ok(NativeResponse {
            content: Some("acting now".to_string()),
            reasoning: None,
            model: "test-model".to_string(),
            usage: Some(Usage {
                prompt_tokens: 11,
                completion_tokens: 7,
                total_tokens: 18,
                cached_prompt_tokens: None,
                cache_read_tokens: None,
                cache_creation_tokens: None,
                iterations: None,
            }),
            finish_reason: FinishReason::ToolCalls,
            tool_calls: Vec::new(),
            native_calls: Vec::new(),
            assistant_blocks: Vec::new(),
            request_id: None,
            organization_id: None,
        })
    }
}

#[async_trait]
impl LLMProvider for ScriptedProvider {
    fn name(&self) -> &str {
        self.name
    }

    fn supports_computer_use(&self) -> bool {
        self.computer_use
    }

    fn supports_decisions(&self) -> bool {
        self.decisions
    }

    async fn decide_choice(&self, request: ChoiceDecisionRequest) -> Result<ChoiceDecisionResponse, LLMError> {
        *self.decisions_calls.lock().unwrap() += 1;
        self.decisions_inputs.lock().unwrap().push(request.input);
        Ok(ChoiceDecisionResponse {
            answer: self.choice.map(|choice| ChoiceDecisionAnswer {
                name: request.name,
                choice: choice.into(),
                confidence: 0.9,
                probabilities: vec![],
            }),
            usage: None,
        })
    }

    async fn generate_native(&self, _request: LLMRequest) -> Result<NativeResponse, LLMError> {
        *self.generate_calls.lock().unwrap() += 1;
        (self.native)()
    }

    async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
        panic!("dispatcher must use the bound route");
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["test-model".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }
}

fn provider() -> ScriptedProvider {
    ScriptedProvider {
        name: "anthropic",
        computer_use: true,
        decisions: false,
        native: ScriptedProvider::canned,
        choice: Some("SAFE"),
        generate_calls: Mutex::default(),
        decisions_calls: Mutex::default(),
        decisions_inputs: Mutex::default(),
    }
}

fn request() -> LLMRequest {
    LLMRequest {
        model: "test-model".into(),
        messages: Vec::new().into(),
        ..Default::default()
    }
}

fn config() -> ComputerUseConfig {
    ComputerUseConfig {
        enabled: true,
        browser_build: Some(PathBuf::from("/approved/build")),
        ..Default::default()
    }
}

fn runtime<'a>(stop: &'a CtrlCState, notify: &'a Notify, stats: &'a mut SessionStats) -> ComputerTurnRuntime<'a> {
    ComputerTurnRuntime { stop, notify, stats }
}

fn anthropic_click(index: usize, id: &str, x: u32, y: u32) -> NativeCall {
    NativeCall {
        id: id.to_string(),
        provider: NativeProvider::Anthropic,
        index,
        body: NativeCallBody::Anthropic {
            name: "left_click".to_string(),
            action: Ok(Action::Click {
                at: Some(Point { x, y }),
                button: MouseButton::Left,
                count: 1,
                modifiers: Vec::new(),
            }),
        },
    }
}

#[tokio::test]
async fn computer_use_turn_refuses_without_config_capability_or_target() {
    let stop = CtrlCState::new();
    let notify = Notify::new();
    let mut stats = SessionStats::default();

    let mut disabled = config();
    disabled.enabled = false;
    let outcome = run_computer_turn(
        &provider(),
        request(),
        &disabled,
        &mut runtime(&stop, &notify, &mut stats),
        None,
        "test-item",
        "test-call",
        |_| async { panic!("approval must not run") },
    )
    .await
    .unwrap();
    assert!(matches!(outcome.kind, ComputerTurnOutcomeKind::Disabled { .. }));

    let incapable = ScriptedProvider { computer_use: false, ..provider() };
    let outcome = run_computer_turn(
        &incapable,
        request(),
        &config(),
        &mut runtime(&stop, &notify, &mut stats),
        None,
        "test-item",
        "test-call",
        |_| async { panic!("approval must not run") },
    )
    .await
    .unwrap();
    assert!(matches!(outcome.kind, ComputerTurnOutcomeKind::Disabled { .. }));

    let mut untargeted = config();
    untargeted.browser_build = None;
    let outcome = run_computer_turn(
        &provider(),
        request(),
        &untargeted,
        &mut runtime(&stop, &notify, &mut stats),
        None,
        "test-item",
        "test-call",
        |_| async { panic!("approval must not run") },
    )
    .await
    .unwrap();
    assert!(matches!(outcome.kind, ComputerTurnOutcomeKind::Disabled { .. }));
}

#[tokio::test]
async fn computer_use_turn_returns_ordinaries_when_no_natives() {
    fn text_only() -> Result<NativeResponse, LLMError> {
        let mut response = ScriptedProvider::canned().unwrap();
        response.native_calls.clear();
        response.tool_calls = vec![ToolCall::function("call_1".into(), "shell".into(), "{}".into())];
        Ok(response)
    }

    let stop = CtrlCState::new();
    let notify = Notify::new();
    let mut stats = SessionStats::default();
    let provider = ScriptedProvider { native: text_only, ..provider() };
    let outcome = run_computer_turn(
        &provider,
        request(),
        &config(),
        &mut runtime(&stop, &notify, &mut stats),
        None,
        "test-item",
        "test-call",
        |_| async { panic!("approval must not run") },
    )
    .await
    .unwrap();
    let ComputerTurnOutcomeKind::NoNativeCalls { content, ordinaries } = outcome.kind else {
        panic!("expected ordinaries, got {:?}", outcome.kind);
    };
    assert_eq!(content.as_deref(), Some("acting now"));
    assert_eq!(ordinaries.len(), 1);
    assert_eq!(outcome.usage.unwrap().total_tokens, 18);
}

#[tokio::test]
async fn computer_use_turn_enforces_budget_before_review_and_approval() {
    fn two_actions() -> Result<NativeResponse, LLMError> {
        let mut response = ScriptedProvider::canned().unwrap();
        response.native_calls = vec![anthropic_click(1, "c1", 19, 3), anthropic_click(3, "c2", 7, 27)];
        Ok(response)
    }

    let stop = CtrlCState::new();
    let notify = Notify::new();
    let mut stats = SessionStats::default();
    let mut config = config();
    config.max_actions_per_turn = 1;
    config.decisions_review = true;
    let provider = ScriptedProvider { native: two_actions, decisions: true, ..provider() };
    let outcome = run_computer_turn(
        &provider,
        request(),
        &config,
        &mut runtime(&stop, &notify, &mut stats),
        None,
        "test-item",
        "test-call",
        |_| async { panic!("approval must not run") },
    )
    .await
    .unwrap();
    let ComputerTurnOutcomeKind::BudgetExceeded { proposed, max } = outcome.kind else {
        panic!("expected budget refusal, got {:?}", outcome.kind);
    };
    assert_eq!((proposed, max), (2, 1));
    assert_eq!(*provider.decisions_calls.lock().unwrap(), 0);
}

#[tokio::test]
async fn computer_use_turn_review_halt_skips_approval_and_launch() {
    fn one_action() -> Result<NativeResponse, LLMError> {
        let mut response = ScriptedProvider::canned().unwrap();
        response.native_calls = vec![anthropic_click(2, "c1", 19, 3)];
        Ok(response)
    }

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    let emitter = HarnessEventEmitter::new(path.clone()).unwrap();
    let stop = CtrlCState::new();
    let notify = Notify::new();
    let mut stats = SessionStats::default();
    let mut config = config();
    config.decisions_review = true;
    let provider = ScriptedProvider {
        native: one_action,
        decisions: true,
        choice: Some("SUSPECT"),
        ..provider()
    };
    let outcome = run_computer_turn(
        &provider,
        request(),
        &config,
        &mut ComputerTurnRuntime { stop: &stop, notify: &notify, stats: &mut stats },
        Some(&emitter),
        "test-item",
        "test-call",
        |_| async { panic!("approval must not run") },
    )
    .await
    .unwrap();
    assert!(matches!(outcome.kind, ComputerTurnOutcomeKind::ReviewHalted { .. }));
    assert_eq!(*provider.decisions_calls.lock().unwrap(), 1);

    // The dispatcher streams progress only: one proposing update against the
    // outer row, nothing started or completed (the wrapper owns those, and
    // the halt verdict reaches the transcript through its completion).
    let raw = std::fs::read_to_string(&path).unwrap();
    let events = raw
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert!(events.iter().all(|event| event["event"]["type"] != "item.started"));
    assert!(events.iter().all(|event| event["event"]["type"] != "item.completed"));
    let updated = events
        .iter()
        .filter(|event| event["event"]["type"] == "item.updated")
        .collect::<Vec<_>>();
    assert_eq!(updated.len(), 1);
    assert_eq!(updated[0]["event"]["item"]["id"], serde_json::json!("test-item:output"));
    let output = updated[0]["event"]["item"]["output"].as_str().unwrap_or("");
    assert!(output.contains("Proposing actions with"), "{output}");
    assert!(!output.contains("awaiting approval"), "{output}");
}

#[tokio::test]
async fn computer_use_review_judges_the_requested_task_not_model_prose() {
    // The safety reviewer detects drift from the user's request. Feeding it
    // the acting model's own prose ("acting now") would let a model describe
    // benign intent while proposing drifted actions.
    fn one_action() -> Result<NativeResponse, LLMError> {
        let mut response = ScriptedProvider::canned().unwrap();
        response.native_calls = vec![anthropic_click(2, "c1", 19, 3)];
        Ok(response)
    }
    let requested = "read the order number from the confirmation page";
    let request = LLMRequest {
        model: "test-model".into(),
        messages: vec![
            vtcode_core::llm::provider::Message {
                role: vtcode_core::llm::provider::MessageRole::System,
                content: vtcode_core::llm::provider::MessageContent::Text(NATIVE_SANDBOX_BRIEFING.to_string()),
                ..Default::default()
            },
            vtcode_core::llm::provider::Message {
                role: vtcode_core::llm::provider::MessageRole::User,
                content: vtcode_core::llm::provider::MessageContent::Text(requested.to_string()),
                ..Default::default()
            },
        ]
        .into(),
        ..Default::default()
    };
    let stop = CtrlCState::new();
    let notify = Notify::new();
    let mut stats = SessionStats::default();
    let mut config = config();
    config.decisions_review = true;
    let provider = ScriptedProvider { native: one_action, decisions: true, ..provider() };
    let outcome = run_computer_turn(
        &provider,
        request,
        &config,
        &mut runtime(&stop, &notify, &mut stats),
        None,
        "test-item",
        "test-call",
        |_| async { Ok(false) },
    )
    .await
    .unwrap();
    assert!(matches!(outcome.kind, ComputerTurnOutcomeKind::ApprovalDenied));
    // The review runs during proposal, before the denial, so it saw the task.
    assert_eq!(*provider.decisions_calls.lock().unwrap(), 1);
    let inputs = provider.decisions_inputs.lock().unwrap();
    let input = inputs.first().expect("the reviewer received one input");
    assert!(input.contains(requested), "review must see the caller's task: {input}");
    assert!(
        !input.contains("acting now"),
        "the model's self-description must not stand in for the task: {input}"
    );
}

#[tokio::test]
async fn computer_use_turn_unreviewed_batch_halts_before_approval() {
    fn one_action() -> Result<NativeResponse, LLMError> {
        let mut response = ScriptedProvider::canned().unwrap();
        response.native_calls = vec![anthropic_click(2, "c1", 19, 3)];
        Ok(response)
    }
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    let emitter = HarnessEventEmitter::new(path.clone()).unwrap();
    let stop = CtrlCState::new();
    let notify = Notify::new();
    let mut stats = SessionStats::default();
    let mut config = config();
    config.decisions_review = true;
    // No choice => inconclusive, not an explicit SUSPECT verdict. An enabled
    // review that cannot conclude must still keep the batch off the target.
    let provider = ScriptedProvider {
        native: one_action,
        decisions: true,
        choice: None,
        ..provider()
    };
    let outcome = run_computer_turn(
        &provider,
        request(),
        &config,
        &mut ComputerTurnRuntime { stop: &stop, notify: &notify, stats: &mut stats },
        Some(&emitter),
        "test-item",
        "test-call",
        |_| async { panic!("approval must not run for an unreviewed batch") },
    )
    .await
    .unwrap();
    assert!(matches!(outcome.kind, ComputerTurnOutcomeKind::ReviewHalted { .. }));
    assert_eq!(*provider.decisions_calls.lock().unwrap(), 1);
    // Progress only: the proposing update streams, while the unreviewed
    // verdict itself completes the outer row in the wrapper (which names
    // the reason in its report text).
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains("Proposing actions with"), "{raw}");
    assert!(!raw.contains("item.started"), "{raw}");
    assert!(!raw.contains("item.completed"), "{raw}");
}

#[tokio::test]
async fn computer_use_turn_denied_approval_skips_launch() {
    fn one_action() -> Result<NativeResponse, LLMError> {
        let mut response = ScriptedProvider::canned().unwrap();
        response.native_calls = vec![anthropic_click(2, "c1", 19, 3)];
        Ok(response)
    }

    let stop = CtrlCState::new();
    let notify = Notify::new();
    let mut stats = SessionStats::default();
    let provider = ScriptedProvider { native: one_action, ..provider() };
    let outcome = run_computer_turn(
        &provider,
        request(),
        &config(),
        &mut runtime(&stop, &notify, &mut stats),
        None,
        "test-item",
        "test-call",
        |_| async { Ok(false) },
    )
    .await
    .unwrap();
    assert!(matches!(outcome.kind, ComputerTurnOutcomeKind::ApprovalDenied));
}

#[tokio::test]
async fn computer_use_turn_rejects_mixed_providers() {
    fn mixed() -> Result<NativeResponse, LLMError> {
        let mut response = ScriptedProvider::canned().unwrap();
        response.native_calls = vec![
            anthropic_click(2, "c1", 19, 3),
            NativeCall {
                id: "c2".to_string(),
                provider: NativeProvider::OpenAi,
                index: 4,
                body: NativeCallBody::OpenAi {
                    replay_item: serde_json::json!({}),
                    actions: Vec::new(),
                },
            },
        ];
        Ok(response)
    }

    let stop = CtrlCState::new();
    let notify = Notify::new();
    let mut stats = SessionStats::default();
    let provider = ScriptedProvider { native: mixed, ..provider() };
    let outcome = run_computer_turn(
        &provider,
        request(),
        &config(),
        &mut runtime(&stop, &notify, &mut stats),
        None,
        "test-item",
        "test-call",
        |_| async { panic!("approval must not run") },
    )
    .await
    .unwrap();
    assert!(matches!(
        outcome.kind,
        ComputerTurnOutcomeKind::Failed { stage: ComputerFailureStage::Generate, .. }
    ));
}

#[tokio::test]
async fn computer_use_turn_cancel_before_generate_skips_provider() {
    let stop = CtrlCState::new();
    stop.request_local_cancel();
    let notify = Notify::new();
    notify.notify_one();
    let mut stats = SessionStats::default();
    let provider = provider();
    let outcome = run_computer_turn(
        &provider,
        request(),
        &config(),
        &mut runtime(&stop, &notify, &mut stats),
        None,
        "test-item",
        "test-call",
        |_| async { panic!("approval must not run") },
    )
    .await
    .unwrap();
    assert!(matches!(outcome.kind, ComputerTurnOutcomeKind::Cancelled));
    assert_eq!(*provider.generate_calls.lock().unwrap(), 0);
}

#[tokio::test]
async fn computer_use_turn_denied_during_stop_reports_cancelled() {
    fn one_action() -> Result<NativeResponse, LLMError> {
        let mut response = ScriptedProvider::canned().unwrap();
        response.native_calls = vec![anthropic_click(2, "c1", 19, 3)];
        Ok(response)
    }

    let stop = CtrlCState::new();
    let notify = Notify::new();
    let mut stats = SessionStats::default();
    let provider = ScriptedProvider { native: one_action, ..provider() };
    let outcome = run_computer_turn(
        &provider,
        request(),
        &config(),
        &mut runtime(&stop, &notify, &mut stats),
        None,
        "test-item",
        "test-call",
        |_| async {
            stop.request_local_cancel();
            Ok(false)
        },
    )
    .await
    .unwrap();
    assert!(matches!(outcome.kind, ComputerTurnOutcomeKind::Cancelled));
}

#[tokio::test]
async fn computer_use_turn_launch_failure_emits_redacted_events() {
    fn one_action() -> Result<NativeResponse, LLMError> {
        let mut response = ScriptedProvider::canned().unwrap();
        response.native_calls = vec![anthropic_click(2, "c1", 19, 3)];
        Ok(response)
    }

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    let emitter = HarnessEventEmitter::new(path.clone()).unwrap();
    let stop = CtrlCState::new();
    let notify = Notify::new();
    let mut stats = SessionStats::default();
    let provider = ScriptedProvider { native: one_action, ..provider() };
    let mut bad = config();
    bad.browser_build = Some(PathBuf::from("/definitely/not/a/computer-use/build"));
    let outcome = run_computer_turn(
        &provider,
        request(),
        &bad,
        &mut ComputerTurnRuntime { stop: &stop, notify: &notify, stats: &mut stats },
        Some(&emitter),
        "test-item",
        "test-call",
        |_| async { Ok(true) },
    )
    .await
    .unwrap();
    let ComputerTurnOutcomeKind::Failed { stage, message } = outcome.kind else {
        panic!("expected launch failure, got {:?}", outcome.kind);
    };
    assert_eq!(stage, ComputerFailureStage::Launch);
    assert!(!message.is_empty());

    // Progress streams stage by stage against the outer row; the wrapper
    // owns start/completion, so a launch failure leaves progress only.
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(!raw.contains("data:image"), "events must never carry payloads");
    let events = raw
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert!(events.iter().all(|event| event["event"]["type"] != "item.started"));
    assert!(events.iter().all(|event| event["event"]["type"] != "item.completed"));
    let updated = events
        .iter()
        .filter(|event| event["event"]["type"] == "item.updated")
        .collect::<Vec<_>>();
    assert_eq!(updated.len(), 3);
    for update in &updated {
        assert_eq!(update["event"]["item"]["id"], serde_json::json!("test-item:output"));
        assert_eq!(update["event"]["item"]["tool_call_id"], serde_json::json!("test-call"));
    }
    let outputs = updated
        .iter()
        .map(|update| update["event"]["item"]["output"].as_str().unwrap_or("").to_string())
        .collect::<Vec<_>>();
    assert!(outputs[0].contains("Proposing actions with"), "{}", outputs[0]);
    assert!(outputs[1].contains("awaiting approval"), "{}", outputs[1]);
    assert!(outputs[2].contains("Approved 1 action (click)"), "{}", outputs[2]);
    assert!(!raw.contains("Preview ready"), "launch failed before the target ran: {raw}");
}

struct ScriptedDriver {
    script: Mutex<std::collections::VecDeque<Observation>>,
}

impl Driver for ScriptedDriver {
    async fn execute(&mut self, _action: &Action, _revoked: &CancellationToken) -> Result<Observation> {
        Ok(self.script.lock().unwrap().pop_front().expect("driver script exhausted"))
    }
}

fn probe_png() -> Vec<u8> {
    [b"\x89PNG\r\n\x1a\n".as_slice(), b"E2E-PROBE-BYTES"].concat()
}

/// Precomputed outside the code path: `base64(b"\x89PNG\r\n\x1a\nE2E-PROBE-BYTES")`.
const PROBE_PNG_BASE64: &str = "iVBORw0KGgpFMkUtUFJPQkUtQllURVM=";

#[tokio::test]
async fn computer_use_anthropic_batch_answers_malformed_and_halts() {
    let turn = CancellationToken::new();
    let mut control = TurnControl::after_approval(
        ScriptedDriver {
            script: Mutex::new([Observation::Done].into_iter().collect()),
        },
        &turn,
    );
    let calls = vec![
        anthropic_click(1, "c1", 19, 3),
        NativeCall {
            id: "c2".to_string(),
            provider: NativeProvider::Anthropic,
            index: 3,
            body: NativeCallBody::Anthropic {
                name: "left_click".to_string(),
                action: Err("boom".to_string()),
            },
        },
    ];
    let quiet = ComputerTurnProgress {
        emitter: None,
        item_id: "test-item",
        call_id: "test-call",
    };
    let (results, units) =
        execute_native_calls(NativeProvider::Anthropic, &mut control, calls, Duration::from_secs(5), quiet)
            .await
            .unwrap();
    assert_eq!(units, 2);
    let NativeTurnResults::Anthropic(positioned) = results else {
        panic!("expected Anthropic results, got {results:?}");
    };
    assert_eq!(positioned.len(), 2);
    assert_eq!(positioned[0].0, 1);
    assert_eq!(positioned[0].1["is_error"], serde_json::json!(false));
    assert_eq!(positioned[1].0, 3);
    assert_eq!(positioned[1].1["tool_use_id"], serde_json::json!("c2"));
    assert_eq!(positioned[1].1["is_error"], serde_json::json!(true));
    let Err(refused) = control.execute_batch("c3", &[Action::Screenshot], Duration::from_secs(1)).await else {
        panic!("failure must revoke the target");
    };
    assert!(refused.to_string().contains("has ended"), "unexpected refusal: {refused:#}");
}

#[tokio::test]
async fn computer_use_openai_call_returns_screenshot_output() {
    let turn = CancellationToken::new();
    let mut control = TurnControl::after_approval(
        ScriptedDriver {
            script: Mutex::new(
                [
                    Observation::Done,
                    Observation::Screenshot(Screenshot::png(probe_png()).unwrap()),
                ]
                .into_iter()
                .collect(),
            ),
        },
        &turn,
    );
    let call = NativeCall {
        id: "call_1".to_string(),
        provider: NativeProvider::OpenAi,
        index: 0,
        body: NativeCallBody::OpenAi {
            replay_item: serde_json::json!({"type": "computer_call", "call_id": "call_1"}),
            actions: vec![
                Action::Click {
                    at: Some(Point { x: 5, y: 5 }),
                    button: MouseButton::Left,
                    count: 1,
                    modifiers: Vec::new(),
                },
                Action::Screenshot,
            ],
        },
    };
    let (results, units) = execute_native_calls(
        NativeProvider::OpenAi,
        &mut control,
        vec![call],
        Duration::from_secs(5),
        ComputerTurnProgress {
            emitter: None,
            item_id: "test-item",
            call_id: "test-call",
        },
    )
    .await
    .unwrap();
    assert_eq!(units, 1);
    let NativeTurnResults::OpenAi(outputs) = results else {
        panic!("expected OpenAI results, got {results:?}");
    };
    assert_eq!(outputs.len(), 1);
    assert_eq!(outputs[0]["type"], serde_json::json!("computer_call_output"));
    assert_eq!(outputs[0]["call_id"], serde_json::json!("call_1"));
    assert_eq!(
        outputs[0]["output"]["image_url"],
        serde_json::json!(format!("data:image/png;base64,{PROBE_PNG_BASE64}"))
    );
}

#[tokio::test]
async fn computer_use_openai_call_without_final_screenshot_fails_closed() {
    let turn = CancellationToken::new();
    let mut control = TurnControl::after_approval(
        ScriptedDriver {
            script: Mutex::new([Observation::Done].into_iter().collect()),
        },
        &turn,
    );
    let call = NativeCall {
        id: "call_1".to_string(),
        provider: NativeProvider::OpenAi,
        index: 0,
        body: NativeCallBody::OpenAi {
            replay_item: serde_json::json!({"type": "computer_call", "call_id": "call_1"}),
            actions: vec![Action::Click {
                at: Some(Point { x: 5, y: 5 }),
                button: MouseButton::Left,
                count: 1,
                modifiers: Vec::new(),
            }],
        },
    };
    let error = execute_native_calls(
        NativeProvider::OpenAi,
        &mut control,
        vec![call],
        Duration::from_secs(5),
        ComputerTurnProgress {
            emitter: None,
            item_id: "test-item",
            call_id: "test-call",
        },
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("final screenshot"), "unexpected error: {error:#}");
    let Err(refused) = control
        .execute_batch("call_2", &[Action::Screenshot], Duration::from_secs(1))
        .await
    else {
        panic!("failure must revoke the target");
    };
    assert!(refused.to_string().contains("has ended"), "unexpected refusal: {refused:#}");
}

#[test]
fn requested_task_uses_last_user_message_even_with_parts_content() {
    // The task may arrive as Parts rather than a plain Text message. The
    // reviewer's evidence must be the most recent user message, not an older
    // Text message that happens to be easier to match.
    let request = LLMRequest {
        model: "test-model".into(),
        messages: vec![
            vtcode_core::llm::provider::Message {
                role: vtcode_core::llm::provider::MessageRole::User,
                content: vtcode_core::llm::provider::MessageContent::Text("older task".into()),
                ..Default::default()
            },
            vtcode_core::llm::provider::Message {
                role: vtcode_core::llm::provider::MessageRole::User,
                content: vtcode_core::llm::provider::MessageContent::parts(vec![
                    vtcode_core::llm::provider::ContentPart::text("current task".into()),
                ]),
                ..Default::default()
            },
        ]
        .into(),
        ..Default::default()
    };
    assert_eq!(requested_task(&request), "current task");
}

mod live;

#[test]
#[serial_test::serial]
fn completed_preview_retains_usable_screenshots_and_skips_malformed() {
    super::super::preview::clear_previews();
    let shot = |id: &str| {
        serde_json::json!({"type": "tool_result", "tool_use_id": id,
            "content": [{"type": "image",
                "source": {"type": "base64", "media_type": "image/png", "data": PROBE_PNG_BASE64}}]})
    };
    let dud = |id: &str| serde_json::json!({"type": "tool_result", "tool_use_id": id, "content": "OK"});
    // A malformed result never blocks a usable screenshot on either side.
    for results in [
        NativeTurnResults::Anthropic(vec![(0, dud("c1")), (2, shot("c2"))]),
        NativeTurnResults::Anthropic(vec![(0, shot("c1")), (2, dud("c2"))]),
    ] {
        let preview = retain_completed_preview(&results, "row-a").expect("usable screenshot");
        assert_eq!(preview.uri, "computer-preview://row-a");
        assert_eq!(preview.bytes, u64::try_from(probe_png().len()).unwrap());
        assert_eq!(
            super::super::preview::take_preview("row-a").as_ref().map(|shot| shot.as_png()),
            Some(probe_png().as_slice())
        );
    }
    // Nothing usable anywhere: no reference, nothing retained.
    let bad = NativeTurnResults::OpenAi(vec![serde_json::json!({"type": "computer_call_output", "call_id": "c9"})]);
    assert!(retain_completed_preview(&bad, "row-b").is_none());
    assert!(super::super::preview::take_preview("row-b").is_none());
    super::super::preview::clear_previews();
}
