use super::*;

#[tokio::test]
async fn refusal_stops_runner_immediately_with_reason_instead_of_idle_retries() {
    let temp = TempDir::new().expect("tempdir");
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.orchestration_mode = vtcode_config::core::agent::HarnessOrchestrationMode::Single;
    let mut runner = Box::pin(make_runner(&temp, vt_cfg, "thread-refusal-stop")).await;
    let provider = RecordingQueuedProvider::with_name(
        "queued-test-provider",
        vec![
            refusal_response(None, "cyber"),
            text_response("should never be requested"),
            text_response("should never be requested"),
            text_response("should never be requested"),
        ],
    );
    runner.provider_client = Box::new(provider.clone());

    let result = Box::pin(runner.execute_task(&task("Refused task", "refused-task"), &[]))
        .await
        .expect("task result");

    assert_eq!(provider.recorded_requests().len(), 1, "a refusal must not be resent");
    let TaskOutcome::Refused { reason } = &result.outcome else {
        panic!("expected refused outcome, got {:?}", result.outcome);
    };
    assert!(reason.starts_with("The model declined this request (category: cyber)."));
    assert_eq!(result.outcome.code(), "refused");
    assert!(result.summary.contains("Outcome Code: refused"));
    assert!(result.summary.contains(reason.as_str()));
    assert!(result.thread_events.iter().any(|event| matches!(
        event,
        ThreadEvent::TurnFailed(failed) if failed.message == *reason
    )));
    assert!(
        !runner
            .thread_handle
            .messages()
            .iter()
            .any(|message| message.role == crate::llm::provider::MessageRole::Assistant),
        "refused output must not stay in the kept history"
    );
}
#[tokio::test]
async fn refusal_explanation_falls_back_to_trimmed_content() {
    let temp = TempDir::new().expect("tempdir");
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.orchestration_mode = vtcode_config::core::agent::HarnessOrchestrationMode::Single;
    let mut runner = Box::pin(make_runner(&temp, vt_cfg, "thread-refusal-content")).await;
    let mut response = refusal_response(Some("  I can't help with that request.  "), "");
    response.reasoning_details = None;
    runner.provider_client = Box::new(QueuedProvider::new(vec![response]));

    let result = Box::pin(runner.execute_task(&task("Refused task", "refused-task"), &[]))
        .await
        .expect("task result");

    assert_eq!(
        result.outcome,
        TaskOutcome::refused(
            "The model declined this request: I can't help with that request. \
             The request was not retried; rephrase it or switch models."
                .to_string()
        )
    );
}
#[tokio::test]
async fn headless_changed_code_cannot_skip_verification_with_continuation_off_or_neutral_text() {
    for verifier in [None, Some("rustc --version")] {
        let temp = TempDir::new().unwrap();
        let mut cfg = VTCodeConfig::default();
        cfg.agent.harness.orchestration_mode = vtcode_config::core::agent::HarnessOrchestrationMode::Single;
        cfg.agent.harness.continuation_policy = vtcode_config::core::agent::ContinuationPolicy::Off;
        cfg.agent.harness.verification.default_verifier_override = verifier.map(str::to_owned);
        cfg.automation.full_auto.max_turns = 3;
        let mut runner = Box::pin(make_runner(&temp, cfg, "short-edit")).await;
        runner.enable_full_auto(&[tools::APPLY_PATCH.into()]).await;
        runner.tool_registry.allow_all_tools().await.unwrap();
        let provider = RecordingQueuedProvider::new(vec![
            tool_call_response(
                tools::APPLY_PATCH,
                json!({"input":"*** Begin Patch\n*** Add File: main.rs\n+fn main() {}\n*** End Patch"}),
            ),
            text_response("Applied the requested change."),
        ]);
        runner.provider_client = Box::new(provider.clone());
        let result = Box::pin(runner.execute_task(&task("One code edit", "exec-task"), &[]))
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "{error}; requests: {:?}",
                    provider
                        .recorded_requests()
                        .iter()
                        .map(|request| &request.messages)
                        .collect::<Vec<_>>()
                )
            });
        assert!(workspace_root(&temp).join("main.rs").exists(), "{:?}", result.outcome);
        assert_eq!(result.outcome.is_success(), verifier.is_some(), "{:?}", result.outcome);
        if verifier.is_none() {
            assert!(result.outcome.description().contains("Unverified"));
        }
    }
}
#[tokio::test]
async fn headless_completion_reuses_a_fresh_model_run_check() {
    let temp = TempDir::new().unwrap();
    let mut cfg = VTCodeConfig::default();
    cfg.agent.harness.orchestration_mode = vtcode_config::core::agent::HarnessOrchestrationMode::Single;
    cfg.agent.harness.continuation_policy = vtcode_config::core::agent::ContinuationPolicy::Off;
    let mut runner = Box::pin(make_runner(&temp, cfg, "reuse-verifier")).await;
    runner
        .enable_full_auto(&[tools::APPLY_PATCH.into(), tools::EXEC_COMMAND.into()])
        .await;
    runner.tool_registry.allow_all_tools().await.unwrap();
    runner.provider_client = Box::new(QueuedProvider::new(vec![
        tool_call_response(
            tools::APPLY_PATCH,
            json!({"input":"*** Begin Patch\n*** Add File: main.rs\n+fn main() {}\n*** End Patch"}),
        ),
        tool_call_response(tools::EXEC_COMMAND, json!({"cmd":"rustc --version"})),
        text_response("The task is complete."),
    ]));
    let result = Box::pin(runner.execute_task(&task("Reuse verifier", "exec-task"), &[]))
        .await
        .unwrap();
    assert_eq!(result.outcome, TaskOutcome::Success);
    assert!(!harness_events(&result).contains(&HarnessEventKind::VerificationStarted));
    assert!(workspace_root(&temp).join("main.rs").exists());
}
#[tokio::test]
async fn headless_completion_verification_allows_one_corrective_pass_then_hands_off() {
    let temp = TempDir::new().unwrap();
    let mut cfg = VTCodeConfig::default();
    cfg.agent.harness.orchestration_mode = vtcode_config::core::agent::HarnessOrchestrationMode::Single;
    cfg.agent.harness.continuation_policy = vtcode_config::core::agent::ContinuationPolicy::Off;
    cfg.agent.harness.verification.default_verifier_override = Some("rustc --invalid-flag-xyz".into());
    cfg.automation.full_auto.max_turns = 8;
    let mut runner = Box::pin(make_runner(&temp, cfg, "bounded-verification")).await;
    runner
        .enable_full_auto(&[tools::APPLY_PATCH.into(), tools::EXEC_COMMAND.into()])
        .await;
    runner.tool_registry.allow_all_tools().await.unwrap();
    let provider = RecordingQueuedProvider::new(vec![
        tool_call_response(
            tools::APPLY_PATCH,
            json!({"input":"*** Begin Patch\n*** Add File: main.rs\n+fn main() {}\n*** End Patch"}),
        ),
        text_response("Applied the change."),
        text_response("The correction is complete."),
        text_response("This third completion must never run."),
    ]);
    runner.provider_client = Box::new(provider.clone());
    let result = Box::pin(runner.execute_task(&task("Bound completion verification", "exec-task"), &[]))
        .await
        .unwrap();
    assert!(!result.outcome.is_success(), "{:?}", result.outcome);
    assert_eq!(provider.recorded_requests().len(), 3, "one correction after the first completion attempt");
    assert!(result.outcome.description().contains("rustc --invalid-flag-xyz"));
    assert!(result.outcome.description().contains("exit code 1"));
    assert!(
        result
            .outcome
            .description()
            .to_ascii_lowercase()
            .contains("unrecognized option")
    );
    assert!(workspace_root(&temp).join("main.rs").exists(), "failed verification retains edits");
}
