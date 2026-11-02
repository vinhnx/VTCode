use super::*;

#[test]
fn retryable_llm_error_includes_internal_server_error_message() {
    assert!(is_retryable_llm_error("Provider error: Internal Server Error"));
}
#[test]
fn retryable_llm_error_excludes_non_transient_messages() {
    assert!(!is_retryable_llm_error("Provider error: Invalid API key"));
}
#[tokio::test]
async fn retry_progress_preserves_operation_and_clears_after_completion() {
    use vtcode_commons::ui_protocol::{ProgressPhase, ProgressUpdate};
    use vtcode_core::ui::{InlineCommand, InlineHandle};

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let guard = handle.begin_progress(ProgressPhase::PreparingContext);
    let operation = guard.operation();
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    ctx.handle = &handle;
    *ctx.provider_client = Box::new(ScriptedProvider::new(
        "mycorp",
        false,
        Arc::new(Mutex::new(Vec::new())),
        vec![
            ScriptedProviderOutcome::Error(uni::LLMError::Provider {
                message: "503 Service Unavailable".into(),
                metadata: None,
            }),
            ScriptedProviderOutcome::Success { content: Some("recovered"), request_id: None },
        ],
    ));
    ctx.working_history.push(uni::Message::user("hello".into()));
    assert!(
        execute_llm_request(&mut ctx, 1, "noop-model", Some(320), false, None)
            .await
            .is_ok()
    );
    drop(guard);

    let mut phases = Vec::new();
    let mut finished = false;
    while let Ok(command) = receiver.try_recv() {
        match command {
            InlineCommand::UpdateProgress(ProgressUpdate::Phase { operation: current, phase }) => {
                assert_eq!(current, operation);
                phases.push(phase);
            }
            InlineCommand::UpdateProgress(ProgressUpdate::Finish { operation: current }) => {
                assert_eq!(current, operation);
                finished = true;
            }
            _ => {}
        }
    }
    assert_eq!(phases.iter().filter(|phase| **phase == ProgressPhase::WaitingForModel).count(), 2);
    let retry = phases.iter().position(|phase| *phase == ProgressPhase::Retrying).unwrap();
    assert_eq!(phases[retry + 1], ProgressPhase::WaitingForModel);
    assert_eq!(phases.last(), Some(&ProgressPhase::ReceivingResponse));
    assert!(finished);
    assert!(handle.current_progress_operation().is_none());
}
#[tokio::test]
async fn compatible_previous_response_not_found_without_sent_chain_is_not_recovered() {
    use vtcode_core::utils::transcript;

    transcript::clear();

    let recorded_previous_response_ids = Arc::new(Mutex::new(Vec::new()));
    let provider = ScriptedProvider::new(
        "mycorp",
        true,
        Arc::clone(&recorded_previous_response_ids),
        vec![ScriptedProviderOutcome::Error(uni::LLMError::Provider {
            message: "Provider error: previous_response_not_found: previous response missing".to_string(),
            metadata: None,
        })],
    );

    let mut backing = TestTurnProcessingBacking::new(4).await;
    let prior_messages = vec![uni::Message::user("hello".to_string())];
    let mut ctx = backing.turn_processing_context();
    *ctx.provider_client = Box::new(provider);
    ctx.working_history.extend(prior_messages.clone());
    ctx.working_history.push(uni::Message::user("continue".to_string()));
    ctx.session_stats
        .set_previous_response_chain("mycorp", "noop-model", Some("resp_123"), &prior_messages);

    let result = execute_llm_request(&mut ctx, 1, "noop-model", Some(320), false, None).await;

    assert!(
        result.is_err(),
        "compatible provider should not recover a stale chain when no previous_response_id was sent"
    );
    assert_eq!(
        recorded_previous_response_ids
            .lock()
            .expect("recorded previous_response_ids")
            .as_slice(),
        &[None]
    );

    let transcript_text = transcript::snapshot().join("\n");

    assert_eq!(
        transcript_text
            .matches("Previous response chain expired; retrying with a fresh provider chain.")
            .count(),
        0
    );

    transcript::clear();
}
