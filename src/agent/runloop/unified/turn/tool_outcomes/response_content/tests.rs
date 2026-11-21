use serde_json::json;

use super::*;
use crate::agent::runloop::unified::run_loop_context::{HarnessTurnState, TurnId, TurnRunId};
use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;

fn big_read_output() -> serde_json::Value {
    json!({ "content_kind": "text", "path": "src/cli/mod.rs" })
}

#[test]
fn whole_file_read_over_threshold_is_summarized() {
    let args = json!({ "action": "read", "path": "src/cli/mod.rs" });
    let feature = tool_output_summary_feature(tool_names::UNIFIED_FILE, &args, &big_read_output(), 10_000);
    assert!(matches!(feature, Some(LightweightFeature::LargeReadSummary)));
}

#[test]
fn bounded_read_is_not_summarized_even_when_large() {
    // Reproduces the turn_640 waste: a ranged read got summarized, forcing
    // a wasteful raw re-read of the identical `offset`/`limit` slice.
    let args = json!({ "action": "read", "path": "src/cli/mod.rs", "offset": 81, "limit": 189 });
    let feature = tool_output_summary_feature(tool_names::UNIFIED_FILE, &args, &big_read_output(), 10_000);
    assert!(feature.is_none(), "bounded reads must be returned verbatim, not summarized");
}

#[test]
fn bounded_read_via_start_line_is_not_summarized() {
    let args = json!({ "action": "read", "path": "src/cli/mod.rs", "start_line": 10, "end_line": 40 });
    let feature = tool_output_summary_feature(tool_names::UNIFIED_FILE, &args, &big_read_output(), 10_000);
    assert!(feature.is_none());
}

#[test]
fn spooled_bounded_read_is_still_summarized() {
    // Genuinely huge output offloaded to disk must always be summarized,
    // even when the read was scoped, so context is not blown out.
    let args = json!({ "action": "read", "path": "big.log", "offset": 0, "limit": 500 });
    let output = json!({
        "content_kind": "text",
        "path": "big.log",
        "spool_path": ".vtcode/context/tool_outputs/big.log"
    });
    let feature = tool_output_summary_feature(tool_names::UNIFIED_FILE, &args, &output, 200_000);
    assert!(matches!(feature, Some(LightweightFeature::LargeReadSummary)));
}

#[test]
fn small_whole_file_read_is_not_summarized() {
    let args = json!({ "action": "read", "path": "small.rs" });
    let feature = tool_output_summary_feature(tool_names::UNIFIED_FILE, &args, &big_read_output(), 500);
    assert!(feature.is_none());
}

#[test]
fn apply_patch_modified_files_remain_internal() {
    let compacted = compact_model_tool_payload(json!({
        "success": true,
        "applied": [{"path": "src/widget.rs", "operation": "update"}],
        "modified_files": ["/workspace/src/widget.rs"]
    }));

    assert_eq!(compacted, json!({"applied": [{"path": "src/widget.rs", "operation": "update"}]}));
}

#[test]
fn prepared_spooled_response_records_spool_metadata_without_visible_bytes() {
    let output = json!({
        "spool_path": ".vtcode/context/tool_outputs/command.log",
        "spooled_bytes": 12_345,
        "preview": "bounded command preview",
        "exit_code": 1,
        "failure_diagnostics": "command completed with status 1"
    });
    let content = maybe_inline_spooled_with_preview(tool_names::UNIFIED_EXEC, &output);
    let mut harness_state = HarnessTurnState::new(
        TurnRunId("response-content-run".to_string()),
        TurnId("response-content-turn".to_string()),
        120,
        2,
    );

    record_prepared_tool_response_metrics(&mut harness_state, &output, &content);

    let diagnostics = harness_state.snapshot_turn_diagnostics(Default::default(), 0);
    assert_eq!(diagnostics.spooled_results, 1);
    assert_eq!(diagnostics.raw_spooled_bytes, 12_345);
    assert_eq!(diagnostics.model_visible_output_bytes, 0);
}

#[test]
fn generic_reference_only_spooled_response_preserves_spool_metadata_and_preview() {
    let output = json!({
        "spool_path": ".vtcode/context/tool_outputs/command.log",
        "spooled_bytes": 38_912,
        "spool_line_count": 377,
        "preview": "bounded command preview",
        "output": "large output that was written to the spool"
    });

    let content = maybe_inline_spooled_with_preview(tool_names::UNIFIED_EXEC, &output);
    let shaped: serde_json::Value = serde_json::from_str(&content).expect("shaped response should be JSON");
    assert_eq!(shaped["spool_path"], output["spool_path"]);
    assert_eq!(shaped["spooled_bytes"], 38_912);
    assert_eq!(shaped["spool_line_count"], 377);
    assert_eq!(shaped["preview"], "bounded command preview");
    assert!(shaped.get("output").is_none());
}

#[tokio::test]
async fn successful_prepared_tool_response_counts_visible_bytes_once_after_history_push() {
    let output = json!({
        "spool_path": ".vtcode/context/tool_outputs/command.log",
        "spooled_bytes": 12_345,
        "preview": "bounded command preview",
        "exit_code": 1,
        "failure_diagnostics": "command completed with status 1"
    });
    let mut backing = TestTurnProcessingBacking::new().await;

    let diagnostics = {
        let mut ctx = backing.turn_processing_context();
        let args = json!({ "cmd": "printf test", "raw": true });
        let content = prepare_tool_response_content(&mut ctx, tool_names::UNIFIED_EXEC, &args, &output).await;
        let expected_visible_bytes = content.len() as u64;

        ctx.push_tool_response("call_1", Some(tool_names::UNIFIED_EXEC), content);

        let diagnostics = ctx.harness_state.snapshot_turn_diagnostics(Default::default(), 0);
        assert_eq!(diagnostics.model_visible_output_bytes, expected_visible_bytes);
        diagnostics
    };

    assert_eq!(diagnostics.spooled_results, 1);
    assert_eq!(diagnostics.raw_spooled_bytes, 12_345);
}

#[tokio::test]
async fn same_call_tool_response_overwrite_replaces_visible_byte_accounting() {
    let mut backing = TestTurnProcessingBacking::new().await;

    let diagnostics = {
        let mut ctx = backing.turn_processing_context();
        let first_content = r#"{"output":"first"}"#.to_string();
        let final_content = r#"{"output":"latest result"}"#.to_string();

        ctx.push_tool_response("call_1", Some(tool_names::READ_FILE), first_content);
        ctx.push_tool_response("call_1", Some(tool_names::READ_FILE), final_content.clone());

        let tool_messages: Vec<&vtcode_core::llm::provider::Message> = ctx
            .working_history
            .iter()
            .filter(|message| matches!(message.role, vtcode_core::llm::provider::MessageRole::Tool))
            .collect();
        assert_eq!(tool_messages.len(), 1, "same-call overwrite should replace the in-place history entry");
        assert_eq!(tool_messages[0].content.as_text_borrowed(), Some(final_content.as_str()));

        let diagnostics = ctx.harness_state.snapshot_turn_diagnostics(Default::default(), 0);
        assert_eq!(
            diagnostics.model_visible_output_bytes,
            final_content.len() as u64,
            "visible-byte diagnostics should match the final history content exactly once"
        );
        diagnostics
    };

    assert_eq!(diagnostics.spooled_results, 0);
    assert_eq!(diagnostics.raw_spooled_bytes, 0);
}

#[tokio::test]
async fn raw_tool_response_applies_reduce_tool_result_hard_caps() {
    // Interactive path must share the headless AgentRunner's reduce caps
    // even when raw=true skips summarization (the previous leak).
    let mut backing = TestTurnProcessingBacking::new().await;
    let stdout = (0..2200).map(|_| "a").collect::<Vec<_>>().join("\n");
    let output = json!({ "stdout": stdout, "exit_code": 0 });
    let args = json!({ "cmd": "gen", "raw": true });

    let mut ctx = backing.turn_processing_context();
    let content = prepare_tool_response_content(&mut ctx, tool_names::UNIFIED_EXEC, &args, &output).await;

    let shaped: serde_json::Value = serde_json::from_str(&content).expect("model content should be JSON");
    assert_eq!(shaped["is_truncated"], json!(true));
    let visible = shaped["stdout"].as_str().expect("stdout remains a string");
    assert!(
        visible.lines().count() < 2200,
        "raw=true must still apply reduce_tool_result line caps, got {} lines",
        visible.lines().count()
    );
}

#[test]
fn compact_continuation_preserves_extras_asymmetrically() {
    // Asymmetric: continue carries a wait hint extra, read carries a path-extra.
    // Both must compact identity keys yet keep their own extras verbatim.
    let continued = compact_next_continue_args(&json!({
        "session_id": "run-continue-1",
        "next_hint": "wait-again"
    }));
    assert_eq!(continued["s"], json!("run-continue-1"));
    assert!(continued.get("session_id").is_none());
    assert_eq!(continued["next_hint"], json!("wait-again"));

    let read = compact_next_read_args(&json!({
        "path": "a.log",
        "offset": 81,
        "limit": 40,
        "spool_chunked": true
    }));
    assert_eq!(read["p"], json!("a.log"));
    assert_eq!(read["o"], json!(81));
    assert!(read.get("path").is_none());
    assert_eq!(read["spool_chunked"], json!(true));
}

#[test]
fn compact_continuation_passes_through_unparsable() {
    // Boundary: non-object and missing-identity inputs must round-trip untouched,
    // never produce an empty compact object the harness would misread.
    let scalar = json!("not-an-object");
    assert_eq!(compact_next_continue_args(&scalar), scalar);
    let missing = json!({"unrelated": 1});
    assert_eq!(compact_next_continue_args(&missing), missing);
    assert_eq!(compact_next_read_args(&missing), missing);
}
