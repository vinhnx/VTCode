//! Container-backed and live-provider computer-use tests.
//! Split from `tests.rs` to keep both suites under the file-length cap;
//! shared scripted fixtures come from the parent module.

use super::*;

fn e2e_config() -> ComputerUseConfig {
    let mut e2e = config();
    e2e.browser_build = Some(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("crates/codegen/vtcode-computer-use/tests/fixtures/browser-app"),
    );
    e2e
}

/// Container-backed E2E tests need the locally built browser image; CI has
/// Docker but not the image, so they stay opt-in like the crate-level proofs.
async fn require_container_test() -> bool {
    if std::env::var("VTCODE_CUA_CONTAINER").as_deref() != Ok("1") {
        eprintln!("skipping container-backed computer-use E2E: set VTCODE_CUA_CONTAINER=1 to run");
        return false;
    }
    if !docker_available().await {
        eprintln!("skipping container-backed computer-use E2E: docker is unavailable");
        return false;
    }
    true
}

// Serial: completed turns retain into the process-global preview registry,
// so container runs must not interleave with its unit tests.
#[tokio::test]
#[serial_test::serial]
async fn computer_use_turn_executes_over_real_container() {
    if !require_container_test().await {
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
        &mut ComputerTurnRuntime { stop: &stop, notify: &notify, stats: &mut stats },
        Some(&emitter),
        "test-item",
        "test-call",
        |_| async { Ok(true) },
    )
    .await
    .unwrap();
    let report = super::super::super::report::turn_report_lines(&outcome).join("\n");
    assert!(!report.contains("base64"), "report must stay redacted");
    let ComputerTurnOutcomeKind::Completed { results, summary, preview } = outcome.kind else {
        panic!(
            "expected completion, got {:?}; build the image first: docker build --tag {} browser",
            outcome.kind,
            e2e_config().image,
        );
    };
    let preview = preview.expect("completed turn with screenshots must carry a preview reference");
    assert!(preview.uri.starts_with("computer-preview://"), "{}", preview.uri);
    assert_eq!(preview.mime_type.as_deref(), Some("image/png"));
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

    // Progress streams every stage against the outer row; the wrapper owns
    // start/completion, so a dispatcher-only run leaves progress only.
    let raw = std::fs::read_to_string(&path).unwrap();
    let events = raw
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert!(events.iter().all(|event| event["event"]["type"] != "item.completed"));
    let updated = events
        .iter()
        .filter(|event| event["event"]["type"] == "item.updated")
        .collect::<Vec<_>>();
    assert!(!updated.is_empty(), "completed turn must stream progress");
    let stages = updated
        .iter()
        .map(|update| update["event"]["item"]["output"].as_str().unwrap_or("").to_string())
        .collect::<Vec<_>>()
        .join("\n");
    for stage in [
        "Proposing actions with",
        "awaiting approval",
        "Approved 2 actions",
        "Preview ready",
        "Removing preview",
    ] {
        assert!(stages.contains(stage), "missing stage {stage:?}: {stages}");
    }
    assert!(!raw.contains("iVBORw0KGgo"), "screenshot bytes must never reach durable events");
}

/// Live dogfood: a real model drives a real computer turn against the real
/// container, asserting verified execution end to end. Default-off: needs
/// docker, network, an `OPENAI_API_KEY` (process env or workspace `.env`),
/// and OpenAI billing, so it runs only with `VTCODE_LIVE_CUA=1` (never in
/// CI or default suites). Uses the probe-verified direct-endpoint tuple.
const LIVE_MODEL: &str = "gpt-6-luna";

#[tokio::test]
#[serial_test::serial]
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
    // Mirror the handler's proposal shape (briefing + user task) so the live
    // provider validates the exact message layout shipped.
    let request = LLMRequest {
        model: LIVE_MODEL.to_string(),
        messages: std::sync::Arc::new(vec![
            vtcode_core::llm::provider::Message {
                role: vtcode_core::llm::provider::MessageRole::System,
                content: vtcode_core::llm::provider::MessageContent::Text(NATIVE_SANDBOX_BRIEFING.to_string()),
                ..Default::default()
            },
            vtcode_core::llm::provider::Message {
                role: vtcode_core::llm::provider::MessageRole::User,
                content: vtcode_core::llm::provider::MessageContent::Text(
                    "Take a screenshot of the preview page.".to_string(),
                ),
                ..Default::default()
            },
        ]),
        ..Default::default()
    };
    // This test is its own human: the isolated container plus a read-only
    // screenshot task make auto-approval safe here.
    let outcome = run_computer_turn(
        &provider,
        request,
        &e2e_config(),
        &mut ComputerTurnRuntime { stop: &stop, notify: &notify, stats: &mut stats },
        Some(&emitter),
        "test-item",
        "test-call",
        |_| async { Ok(true) },
    )
    .await
    .unwrap();
    let report = super::super::super::report::turn_report_lines(&outcome).join("\n");
    assert!(!report.contains("base64"), "report must stay redacted");
    let ComputerTurnOutcomeKind::Completed { results, summary, preview } = outcome.kind else {
        panic!("live turn must complete, got {:?}", outcome.kind);
    };
    assert!(preview.is_some(), "live turn with screenshots must carry a preview reference");
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
    eprintln!("live turn verified: {} executed units, usage={:?}", summary.executed_units, outcome.usage);
}

/// Unique typed-text sentinel: the release gate scans durable outputs for it.
const TYPED_SENTINEL: &str = "SENTINEL-TYPED-vtcode-7f3a";

#[tokio::test]
#[serial_test::serial]
async fn computer_use_turn_redacts_sentinels_from_durable_outputs() {
    if !require_container_test().await {
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
        &mut ComputerTurnRuntime { stop: &stop, notify: &notify, stats: &mut stats },
        Some(&emitter),
        "test-item",
        "test-call",
        |approval| async move {
            assert_eq!(approval.action_kinds, vec!["screenshot".to_string(), "type".to_string()]);
            Ok(true)
        },
    )
    .await
    .unwrap();
    let report = super::super::super::report::turn_report_lines(&outcome).join("\n");
    assert!(!report.contains(TYPED_SENTINEL), "typed text must never reach the report");
    let ComputerTurnOutcomeKind::Completed { summary, .. } = outcome.kind else {
        panic!("expected completion, got {:?}", outcome.kind);
    };
    assert_eq!(summary.executed_units, 2);

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(!raw.contains(TYPED_SENTINEL), "typed text must never reach durable events");
    assert!(!raw.contains("iVBORw0KGgo"), "screenshot bytes must never reach durable events");
}

#[tokio::test]
async fn computer_use_turn_cancel_tears_down_launched_target() {
    if !require_container_test().await {
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
        &mut ComputerTurnRuntime { stop: &stop, notify: &notify, stats: &mut stats },
        Some(&emitter),
        "test-item",
        "test-call",
        |_| async {
            stop.request_local_cancel();
            notify.notify_one();
            Ok(true)
        },
    )
    .await
    .unwrap();
    assert!(matches!(outcome.kind, ComputerTurnOutcomeKind::Cancelled));

    // Progress only: approval streams, then the stop lands before any input
    // runs. The wrapper completes the cancelled row; teardown still ran.
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(!raw.contains("item.completed"), "{raw}");
    assert!(raw.contains("Approved 1 action (screenshot)"), "{raw}");
    assert!(!raw.contains("Preview ready"), "{raw}");
}

#[cfg(unix)]
#[tokio::test]
async fn launch_failure_message_preserves_error_chain() {
    // Regression: a missing browser build must report WHY (the os error),
    // not just the outer context — the model and user diagnose from this
    // text, and Display-only rendering once hid the ENOENT behind
    // "could not be canonicalized".
    use tokio_util::sync::CancellationToken;

    let target = ComputerUseTarget::Browser {
        build_dir: PathBuf::from("/nonexistent-cua-build-dir-xyz"),
    };
    let summary = NativeCallSummary {
        call_ids: vec!["call_1".to_string()],
        kinds: vec!["screenshot".to_string()],
        valid_actions: 1,
        provider: NativeProvider::OpenAi,
    };
    let stop = CtrlCState::new();
    let notify = Notify::new();
    let mut stats = SessionStats::default();
    let mut runtime = runtime(&stop, &notify, &mut stats);
    let turn = CancellationToken::new();
    let progress = ComputerTurnProgress {
        emitter: None,
        item_id: "test-item",
        call_id: "test-call",
    };
    let execution =
        run_guarded_execution(&summary, Vec::new(), "img:1", &target, &config(), &mut runtime, &turn, progress).await;
    let GuardedOutcome::Failed { stage, message } = execution.outcome else {
        panic!("expected launch failure for missing build dir");
    };
    assert_eq!(stage, ComputerFailureStage::Launch);
    assert!(message.contains("Failed to canonicalize"), "{message}");
    assert!(message.contains("No such file or directory"), "{message}");
}

#[test]
fn unreachable_container_engine_gives_actionable_launch_failure() {
    // A stopped daemon otherwise fails deep inside `docker run` with a bare
    // ENOENT from the missing socket. The guard must refuse with a next step
    // the model can relay, and stay silent when the engine is available.
    let error = ensure_container_engine(false).expect_err("stopped daemon must refuse the turn");
    assert!(error.to_string().contains("Docker daemon is not available"), "{error}");
    assert!(error.to_string().contains("start your container engine"), "{error}");
    assert_eq!(error.to_string(), DOCKER_UNAVAILABLE_GUIDANCE);
    ensure_container_engine(true).expect("available engine must proceed");
}
