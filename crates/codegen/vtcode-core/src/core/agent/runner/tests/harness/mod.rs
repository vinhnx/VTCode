#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]

pub(super) use super::*;

const COMPLETE_HEADLESS_PLAN: &str = "<proposed_plan>\n## Summary\nUpdate the fixture greeting.\n\n## Implementation Steps\n1. Update greeting -> files: [fixture.txt] -> verify: [rg -n 'greeting' fixture.txt]\n\n## Validation\nThe greeting appears once.\n\n## Assumptions\nKeep unrelated contents unchanged.\n</proposed_plan>";

fn assert_review_request_exposes_only_code_search(request: &LLMRequest) {
    let tool_names = request
        .tools
        .as_deref()
        .map(|definitions| definitions.iter().map(|tool| tool.function_name()).collect::<Vec<_>>())
        .unwrap_or_default();
    assert!(tool_names.contains(&tools::CODE_SEARCH));
    for mutating_tool in [tools::EXEC_COMMAND, tools::APPLY_PATCH] {
        assert!(!tool_names.contains(&mutating_tool), "review request must hide {mutating_tool}; got {tool_names:?}");
    }
}

async fn record_review_request(
    temp: &TempDir,
    model: ModelId,
    provider_name: &'static str,
    session_id: &str,
) -> LLMRequest {
    let mut runner = Box::pin(make_runner_for_model(temp, VTCodeConfig::default(), session_id, model)).await;
    let allowlist = runner.review_tool_allowlist(&[tools::WILDCARD_ALL.to_string()]).await;
    runner.enable_full_auto(&allowlist).await;

    let provider = RecordingQueuedProvider::with_name(
        provider_name,
        vec![
            text_response("The review is complete."),
            text_response("The review is complete."),
        ],
    );
    let recorded = provider.clone();
    runner.provider_client = Box::new(provider);
    let _result = Box::pin(runner.execute_task(&task("Review task", "review-task"), &[]))
        .await
        .expect("task result");

    recorded
        .recorded_requests()
        .into_iter()
        .next()
        .expect("review provider request")
}

fn refusal_response(content: Option<&str>, category: &str) -> LLMResponse {
    LLMResponse {
        content: content.map(str::to_string),
        finish_reason: FinishReason::Refusal,
        reasoning_details: Some(vec![json!({"type": "stop_details", "category": category}).to_string()]),
        ..LLMResponse::default()
    }
}

mod evaluator_recovery;
mod evaluator_scorecard;
mod execution_modes;
mod harness_lifecycle;
mod headless_planning;
mod refusal_completion;
mod replan_tracker;
