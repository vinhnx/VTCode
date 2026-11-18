use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use futures::stream::FuturesUnordered;
use tempfile::TempDir;
use vtcode_core::config::constants::tools;
use vtcode_core::tools::registry::ToolRegistry;

use super::{PreparedToolCall, ValidatedToolCall, interrupt_parallel_group, planned_execution_group_stats};
use crate::agent::runloop::unified::turn::context::{PreparedAssistantToolCall, TurnHandlerOutcome, TurnLoopResult};

#[tokio::test]
async fn retained_navigation_notice_follows_tool_result_and_leaves_tools_available() {
    use crate::agent::runloop::unified::tool_pipeline::{ToolExecutionStatus, ToolPipelineOutcome};
    use crate::agent::runloop::unified::turn::tool_outcomes::helpers::{LoopTracker, update_repetition_tracker};

    let mut backing = super::super::tests::TestContextBacking::new().await;
    let mut ctx = backing.turn_processing_context();
    let mut tracker = LoopTracker::new();
    let args = serde_json::json!({"cmd":"sed -n '2,3p' README.md"});
    let result = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({"output":"second\nthird\n", "exit_code":0}),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    for _ in 0..2 {
        update_repetition_tracker(&mut tracker, &result, tools::EXEC_COMMAND, &args);
    }
    ctx.working_history.push(vtcode_core::llm::provider::Message::tool_response(
        "call-read".to_string(),
        "completed output".to_string(),
    ));
    super::flush_loop_notices(&mut ctx, &mut tracker);
    let last = ctx.working_history.last().expect("coaching after result");
    assert!(last.content.as_text().contains("only text evidence already gathered"));
    assert!(last.content.as_text().contains("Tools remain available"));
    super::flush_loop_notices(&mut ctx, &mut tracker);
    assert_eq!(
        ctx.working_history
            .iter()
            .filter(|m| m.content.as_text().contains("only text evidence already gathered"))
            .count(),
        1
    );
}

fn validated_call<'a>(
    call_id: &'a str,
    tool_name: &str,
    readonly_classification: bool,
    parallel_safe_after_preflight: bool,
    effective_args: serde_json::Value,
) -> ValidatedToolCall<'a> {
    let raw_tool_call = vtcode_core::llm::provider::ToolCall::function(
        call_id.to_string(),
        tool_name.to_string(),
        serde_json::to_string(&effective_args).expect("serialize args"),
    );
    ValidatedToolCall {
        tool_call: Box::leak(Box::new(PreparedAssistantToolCall::new(raw_tool_call))),
        prepared: PreparedToolCall {
            canonical_name: tool_name.to_string(),
            readonly_classification,
            parallel_safe_after_preflight,
            effective_args: effective_args.clone(),
            fallback_recommendation: None,
            already_preflighted: true,
        },
    }
}

#[test]
fn build_execution_groups_batches_contiguous_parallel_safe_reads() {
    let stats = planned_execution_group_stats(
        &[
            validated_call("call_1", tools::CODE_SEARCH, true, true, serde_json::json!({"query":"turn loop"})),
            validated_call("call_2", tools::READ_FILE, true, true, serde_json::json!({"path":"src/main.rs"})),
        ],
        true,
    );

    assert_eq!(stats, (1, 1, 2));
}

#[tokio::test]
async fn prevalidated_call_is_rejected_when_prior_group_exhausted_wall_clock() {
    let mut backing = super::super::tests::TestContextBacking::new().await;
    let mut ctx = backing.turn_processing_context();
    let call = validated_call("later-call", tools::READ_FILE, true, true, serde_json::json!({"path":"sample.txt"}));
    ctx.harness_state.tool_wall_clock_elapsed = std::time::Duration::from_secs(59);
    assert!(!super::reject_expired_wall_clock(&mut ctx, &call));
    ctx.harness_state.tool_wall_clock_elapsed += std::time::Duration::from_secs(1);
    assert!(super::reject_expired_wall_clock(&mut ctx, &call));
    assert!(ctx.working_history.iter().any(|message| {
        message.tool_call_id.as_deref() == Some("later-call") && message.content.as_text().contains("wall clock budget")
    }));
}

#[test]
fn build_execution_groups_preserves_order_around_mutating_calls() {
    let stats = planned_execution_group_stats(
        &[
            validated_call("call_1", tools::CODE_SEARCH, true, true, serde_json::json!({"query":"alpha"})),
            validated_call(
                "call_2",
                tools::UNIFIED_EXEC,
                false,
                false,
                serde_json::json!({"action":"run","command":["cargo","check"]}),
            ),
            validated_call("call_3", tools::CODE_SEARCH, true, true, serde_json::json!({"query":"omega"})),
        ],
        true,
    );

    assert_eq!(stats, (3, 0, 1));
}

#[test]
fn build_execution_groups_batches_duplicate_parallel_tool_names() {
    let stats = planned_execution_group_stats(
        &[
            validated_call("call_1", tools::CODE_SEARCH, true, true, serde_json::json!({"query":"alpha"})),
            validated_call("call_2", tools::CODE_SEARCH, true, true, serde_json::json!({"query":"beta"})),
        ],
        true,
    );

    assert_eq!(stats, (1, 1, 2));
}

#[test]
fn build_execution_groups_falls_back_to_serial_when_parallel_disabled() {
    let stats = planned_execution_group_stats(
        &[
            validated_call("call_1", tools::CODE_SEARCH, true, true, serde_json::json!({"query":"alpha"})),
            validated_call("call_2", tools::CODE_SEARCH, true, true, serde_json::json!({"query":"beta"})),
        ],
        false,
    );

    assert_eq!(stats, (2, 0, 1));
}

#[test]
fn build_execution_groups_keeps_non_parallel_safe_reads_serial() {
    let stats = planned_execution_group_stats(
        &[
            validated_call("call_1", tools::LIST_PTY_SESSIONS, true, false, serde_json::json!({})),
            validated_call("call_2", tools::CODE_SEARCH, true, true, serde_json::json!({"query":"beta"})),
        ],
        true,
    );

    assert_eq!(stats, (2, 0, 1));
}

#[test]
fn build_execution_groups_respects_post_preflight_parallel_safety() {
    let raw_tool_call = vtcode_core::llm::provider::ToolCall::function(
        "call_remapped".to_string(),
        tools::UNIFIED_FILE.to_string(),
        serde_json::json!({"path":"src/main.rs"}).to_string(),
    );
    let remapped = ValidatedToolCall {
        tool_call: Box::leak(Box::new(PreparedAssistantToolCall::new(raw_tool_call))),
        prepared: PreparedToolCall {
            canonical_name: tools::UNIFIED_EXEC.to_string(),
            readonly_classification: true,
            parallel_safe_after_preflight: false,
            effective_args: serde_json::json!({"action":"run","command":"git status"}),
            fallback_recommendation: None,
            already_preflighted: true,
        },
    };

    let stats = planned_execution_group_stats(
        &[
            remapped,
            validated_call("call_read", tools::CODE_SEARCH, true, true, serde_json::json!({"query":"beta"})),
        ],
        true,
    );

    assert_eq!(stats, (2, 0, 1));
}

#[tokio::test]
async fn interrupt_parallel_group_drains_pending_futures() {
    let temp_dir = TempDir::new().expect("create temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let completions = Arc::new(AtomicUsize::new(0));
    let mut futures = FuturesUnordered::new();

    for _ in 0..2 {
        let completions = Arc::clone(&completions);
        futures.push(async move {
            completions.fetch_add(1, Ordering::SeqCst);
        });
    }

    let outcome = interrupt_parallel_group(
        &registry,
        &mut futures,
        false,
        TurnLoopResult::Cancelled,
        "test interruption cleanup",
    )
    .await;

    assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Cancelled)));
    assert_eq!(completions.load(Ordering::SeqCst), 2);
    assert!(futures.is_empty());
}
