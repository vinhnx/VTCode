use super::*;
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

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
    ComputerTurnRuntime { stop, notify, stats, emitter: None }
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
    let outcome =
        run_computer_turn(&provider(), request(), &disabled, &mut runtime(&stop, &notify, &mut stats), |_| async {
            panic!("approval must not run")
        })
        .await
        .unwrap();
    assert!(matches!(outcome.kind, ComputerTurnOutcomeKind::Disabled { .. }));

    let incapable = ScriptedProvider { computer_use: false, ..provider() };
    let outcome =
        run_computer_turn(&incapable, request(), &config(), &mut runtime(&stop, &notify, &mut stats), |_| async {
            panic!("approval must not run")
        })
        .await
        .unwrap();
    assert!(matches!(outcome.kind, ComputerTurnOutcomeKind::Disabled { .. }));

    let mut untargeted = config();
    untargeted.browser_build = None;
    let outcome =
        run_computer_turn(&provider(), request(), &untargeted, &mut runtime(&stop, &notify, &mut stats), |_| async {
            panic!("approval must not run")
        })
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
    let outcome =
        run_computer_turn(&provider, request(), &config(), &mut runtime(&stop, &notify, &mut stats), |_| async {
            panic!("approval must not run")
        })
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
    let outcome =
        run_computer_turn(&provider, request(), &config, &mut runtime(&stop, &notify, &mut stats), |_| async {
            panic!("approval must not run")
        })
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
        &mut ComputerTurnRuntime {
            stop: &stop,
            notify: &notify,
            stats: &mut stats,
            emitter: Some(&emitter),
        },
        |_| async { panic!("approval must not run") },
    )
    .await
    .unwrap();
    assert!(matches!(outcome.kind, ComputerTurnOutcomeKind::ReviewHalted));
    assert_eq!(*provider.decisions_calls.lock().unwrap(), 1);

    let raw = std::fs::read_to_string(&path).unwrap();
    let events = raw
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert!(events.iter().all(|event| event["event"]["type"] != "item.started"));
    let completed = events
        .iter()
        .filter(|event| event["event"]["type"] == "item.completed")
        .collect::<Vec<_>>();
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0]["event"]["item"]["status"], serde_json::json!("failed"));
    let output = completed[0]["event"]["item"]["output"].as_str().unwrap_or("");
    assert!(output.contains("review halted"), "{output}");
    assert!(output.contains("not launched"), "{output}");
}

#[test]
fn computer_use_summary_message_names_the_review_state() {
    let summary = |review| RedactedTurnSummary {
        target_kind: "browser",
        target: "fixture-app".to_string(),
        recipient: "test-model (test)".to_string(),
        action_kinds: vec!["click".to_string()],
        action_count: 1,
        executed_units: 1,
        review,
        teardown_trigger: "finished",
        teardown: TeardownOutcome::Removed,
    };
    assert!(!summary(None).message("completed").contains("review"));
    assert!(
        summary(Some(BatchReviewVerdict::Proceed))
            .message("completed")
            .contains("; reviewed")
    );
    assert!(
        summary(Some(BatchReviewVerdict::Unreviewed { reason: "timed out" }))
            .message("completed")
            .contains("; review unreviewed (timed out)")
    );
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
    let outcome =
        run_computer_turn(&provider, request(), &config(), &mut runtime(&stop, &notify, &mut stats), |_| async {
            Ok(false)
        })
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
    let outcome =
        run_computer_turn(&provider, request(), &config(), &mut runtime(&stop, &notify, &mut stats), |_| async {
            panic!("approval must not run")
        })
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
    let outcome =
        run_computer_turn(&provider, request(), &config(), &mut runtime(&stop, &notify, &mut stats), |_| async {
            panic!("approval must not run")
        })
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
    let outcome =
        run_computer_turn(&provider, request(), &config(), &mut runtime(&stop, &notify, &mut stats), |_| async {
            stop.request_local_cancel();
            Ok(false)
        })
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
        &mut ComputerTurnRuntime {
            stop: &stop,
            notify: &notify,
            stats: &mut stats,
            emitter: Some(&emitter),
        },
        |_| async { Ok(true) },
    )
    .await
    .unwrap();
    let ComputerTurnOutcomeKind::Failed { stage, message } = outcome.kind else {
        panic!("expected launch failure, got {:?}", outcome.kind);
    };
    assert_eq!(stage, ComputerFailureStage::Launch);
    assert!(!message.is_empty());

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(!raw.contains("data:image"), "events must never carry payloads");
    let events = raw
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(events.iter().filter(|event| event["event"]["type"] == "item.started").count(), 1);
    let completed = events
        .iter()
        .filter(|event| event["event"]["type"] == "item.completed")
        .collect::<Vec<_>>();
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0]["event"]["item"]["tool_call_id"], serde_json::json!("c1"));
    assert_eq!(completed[0]["event"]["item"]["status"], serde_json::json!("failed"));
    assert!(
        completed[0]["event"]["item"]["output"]
            .as_str()
            .is_some_and(|output| output.contains("failed")),
        "completion output names the verdict: {}",
        completed[0]["event"]["item"]["output"]
    );
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
    let (results, units) = execute_native_calls(NativeProvider::Anthropic, &mut control, calls, Duration::from_secs(5))
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
    let (results, units) =
        execute_native_calls(NativeProvider::OpenAi, &mut control, vec![call], Duration::from_secs(5))
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
    let error = execute_native_calls(NativeProvider::OpenAi, &mut control, vec![call], Duration::from_secs(5))
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

fn e2e_config() -> ComputerUseConfig {
    let mut e2e = config();
    e2e.browser_build = Some(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("crates/codegen/vtcode-computer-use/tests/fixtures/browser-app"),
    );
    e2e
}

#[tokio::test]
async fn computer_use_turn_executes_over_real_container() {
    if !docker_available().await {
        eprintln!("skipping computer-use E2E: docker is unavailable");
        return;
    }
    fn shot_and_click() -> Result<NativeResponse, LLMError> {
        let mut response = ScriptedProvider::canned().unwrap();
        response.native_calls = vec![
            NativeCall {
                id: "shot".to_string(),
                provider: NativeProvider::Anthropic,
                index: 1,
                body: NativeCallBody::Anthropic {
                    name: "screenshot".to_string(),
                    action: Ok(Action::Screenshot),
                },
            },
            anthropic_click(3, "poke", 40, 40),
        ];
        Ok(response)
    }

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    let emitter = HarnessEventEmitter::new(path.clone()).unwrap();
    let stop = CtrlCState::new();
    let notify = Notify::new();
    let mut stats = SessionStats::default();
    let provider = ScriptedProvider { native: shot_and_click, ..provider() };
    let outcome = run_computer_turn(
        &provider,
        request(),
        &e2e_config(),
        &mut ComputerTurnRuntime {
            stop: &stop,
            notify: &notify,
            stats: &mut stats,
            emitter: Some(&emitter),
        },
        |_| async { Ok(true) },
    )
    .await
    .unwrap();
    let ComputerTurnOutcomeKind::Completed { results, summary } = outcome.kind else {
        panic!(
            "expected completion, got {:?}; build the image first: docker build --tag {} browser",
            outcome.kind,
            e2e_config().image,
        );
    };
    assert_eq!(summary.action_count, 2);
    assert_eq!(summary.executed_units, 2);
    assert!(summary.review.is_none());
    assert!(
        matches!(summary.teardown, TeardownOutcome::Removed | TeardownOutcome::AlreadyGone),
        "E2E must remove its container: {:?}",
        summary.teardown
    );
    assert_eq!(outcome.usage.unwrap().total_tokens, 18);
    let NativeTurnResults::Anthropic(positioned) = results else {
        panic!("expected Anthropic results, got {results:?}");
    };
    assert_eq!(positioned.len(), 2);
    assert_eq!((positioned[0].0, positioned[1].0), (1, 3));
    assert_eq!(positioned[0].1["is_error"], serde_json::json!(false));
    assert_eq!(positioned[1].1["is_error"], serde_json::json!(false));
    let shot_data = positioned[0].1["content"][0]["source"]["data"].as_str().unwrap_or("");
    assert!(
        shot_data.starts_with("iVBORw0KGgo"),
        "real browser screenshot must be PNG payload, got {} bytes",
        shot_data.len()
    );
    assert!(!summary.message("completed").contains("base64"), "summary must stay redacted");

    let raw = std::fs::read_to_string(&path).unwrap();
    let completed = raw
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|event| event["event"]["type"] == "item.completed")
        .collect::<Vec<_>>();
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0]["event"]["item"]["status"], serde_json::json!("completed"));
}

/// Live dogfood: a real model drives a real computer turn against the real
/// container, asserting verified execution end to end. Default-off: needs
/// docker, network, an `OPENAI_API_KEY` (process env or workspace `.env`),
/// and OpenAI billing, so it runs only with `VTCODE_LIVE_CUA=1` (never in
/// CI or default suites). Uses the probe-verified direct-endpoint tuple.
const LIVE_MODEL: &str = "gpt-6-luna";

#[tokio::test]
async fn computer_use_live_turn_executes_over_real_container() {
    if std::env::var("VTCODE_LIVE_CUA").as_deref() != Ok("1") {
        eprintln!("skipping live computer-use turn: set VTCODE_LIVE_CUA=1 to run");
        return;
    }
    if !docker_available().await {
        eprintln!("skipping live computer-use turn: docker is unavailable");
        return;
    }
    let key = std::env::var("OPENAI_API_KEY")
        .ok()
        .filter(|key| !key.trim().is_empty())
        .or_else(|| {
            vtcode_config::read_workspace_env_value(std::path::Path::new(env!("CARGO_MANIFEST_DIR")), "OPENAI_API_KEY")
                .ok()
                .flatten()
                .filter(|key| !key.trim().is_empty())
        });
    let Some(key) = key else {
        eprintln!("skipping live computer-use turn: no OPENAI_API_KEY in env or workspace .env");
        return;
    };
    let provider = vtcode_llm::providers::openai::OpenAIProvider::from_config(
        Some(key),
        None,
        Some(LIVE_MODEL.to_string()),
        None,
        None,
        None,
        None,
        None,
        None,
    );
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    let emitter = HarnessEventEmitter::new(path.clone()).unwrap();
    let stop = CtrlCState::new();
    let notify = Notify::new();
    let mut stats = SessionStats::default();
    let request = LLMRequest {
        model: LIVE_MODEL.to_string(),
        messages: std::sync::Arc::new(vec![vtcode_core::llm::provider::Message {
            role: vtcode_core::llm::provider::MessageRole::User,
            content: vtcode_core::llm::provider::MessageContent::Text(
                "Take a screenshot of the current screen.".to_string(),
            ),
            ..Default::default()
        }]),
        ..Default::default()
    };
    // This test is its own human: the isolated container plus a read-only
    // screenshot task make auto-approval safe here.
    let outcome = run_computer_turn(
        &provider,
        request,
        &e2e_config(),
        &mut ComputerTurnRuntime {
            stop: &stop,
            notify: &notify,
            stats: &mut stats,
            emitter: Some(&emitter),
        },
        |_| async { Ok(true) },
    )
    .await
    .unwrap();
    let ComputerTurnOutcomeKind::Completed { results, summary } = outcome.kind else {
        panic!("live turn must complete, got {:?}", outcome.kind);
    };
    assert!(summary.executed_units >= 1, "live turn executed nothing: {summary:?}");
    assert!(
        matches!(summary.teardown, TeardownOutcome::Removed | TeardownOutcome::AlreadyGone),
        "live turn must remove its container: {:?}",
        summary.teardown
    );
    let NativeTurnResults::OpenAi(outputs) = results else {
        panic!("expected OpenAI results, got {results:?}");
    };
    assert!(!outputs.is_empty(), "live turn returned no call outputs");
    assert!(outcome.usage.as_ref().is_some_and(|usage| usage.total_tokens > 0), "live turn must report usage");
    assert!(!summary.message("completed").contains("base64"), "summary must stay redacted");
    eprintln!("live turn verified: {} executed units, usage={:?}", summary.executed_units, outcome.usage);
}

/// Unique typed-text sentinel: the release gate scans durable outputs for it.
const TYPED_SENTINEL: &str = "SENTINEL-TYPED-vtcode-7f3a";

#[tokio::test]
async fn computer_use_turn_redacts_sentinels_from_durable_outputs() {
    if !docker_available().await {
        eprintln!("skipping computer-use sentinel E2E: docker is unavailable");
        return;
    }
    fn type_and_shot() -> Result<NativeResponse, LLMError> {
        let mut response = ScriptedProvider::canned().unwrap();
        response.native_calls = vec![
            NativeCall {
                id: "typer".to_string(),
                provider: NativeProvider::Anthropic,
                index: 0,
                body: NativeCallBody::Anthropic {
                    name: "type".to_string(),
                    action: Ok(Action::Type(TYPED_SENTINEL.to_string())),
                },
            },
            NativeCall {
                id: "shot".to_string(),
                provider: NativeProvider::Anthropic,
                index: 2,
                body: NativeCallBody::Anthropic {
                    name: "screenshot".to_string(),
                    action: Ok(Action::Screenshot),
                },
            },
        ];
        Ok(response)
    }

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    let emitter = HarnessEventEmitter::new(path.clone()).unwrap();
    let stop = CtrlCState::new();
    let notify = Notify::new();
    let mut stats = SessionStats::default();
    let provider = ScriptedProvider { native: type_and_shot, ..provider() };
    let outcome = run_computer_turn(
        &provider,
        request(),
        &e2e_config(),
        &mut ComputerTurnRuntime {
            stop: &stop,
            notify: &notify,
            stats: &mut stats,
            emitter: Some(&emitter),
        },
        |approval| async move {
            assert_eq!(approval.action_kinds, vec!["screenshot".to_string(), "type".to_string()]);
            Ok(true)
        },
    )
    .await
    .unwrap();
    let ComputerTurnOutcomeKind::Completed { summary, .. } = outcome.kind else {
        panic!("expected completion, got {:?}", outcome.kind);
    };
    assert_eq!(summary.executed_units, 2);

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(!raw.contains(TYPED_SENTINEL), "typed text must never reach durable events");
    assert!(!raw.contains("iVBORw0KGgo"), "screenshot bytes must never reach durable events");
    assert!(!summary.message("completed").contains(TYPED_SENTINEL));
}

#[tokio::test]
async fn computer_use_turn_cancel_tears_down_launched_target() {
    if !docker_available().await {
        eprintln!("skipping computer-use cancel E2E: docker is unavailable");
        return;
    }
    fn one_shot() -> Result<NativeResponse, LLMError> {
        let mut response = ScriptedProvider::canned().unwrap();
        response.native_calls = vec![NativeCall {
            id: "shot".to_string(),
            provider: NativeProvider::Anthropic,
            index: 1,
            body: NativeCallBody::Anthropic {
                name: "screenshot".to_string(),
                action: Ok(Action::Screenshot),
            },
        }];
        Ok(response)
    }

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    let emitter = HarnessEventEmitter::new(path.clone()).unwrap();
    let stop = CtrlCState::new();
    let notify = Notify::new();
    let mut stats = SessionStats::default();
    let provider = ScriptedProvider { native: one_shot, ..provider() };
    let outcome = run_computer_turn(
        &provider,
        request(),
        &e2e_config(),
        &mut ComputerTurnRuntime {
            stop: &stop,
            notify: &notify,
            stats: &mut stats,
            emitter: Some(&emitter),
        },
        |_| async {
            stop.request_local_cancel();
            notify.notify_one();
            Ok(true)
        },
    )
    .await
    .unwrap();
    assert!(matches!(outcome.kind, ComputerTurnOutcomeKind::Cancelled));

    let raw = std::fs::read_to_string(&path).unwrap();
    let completed = raw
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|event| event["event"]["type"] == "item.completed")
        .collect::<Vec<_>>();
    assert_eq!(completed.len(), 1);
    let output = completed[0]["event"]["item"]["output"].as_str().unwrap_or("");
    assert!(output.contains("cancelled"), "verdict must be cancelled: {output}");
    assert!(
        output.contains("removed") || output.contains("already gone"),
        "cancel must still tear the target down: {output}"
    );
}
