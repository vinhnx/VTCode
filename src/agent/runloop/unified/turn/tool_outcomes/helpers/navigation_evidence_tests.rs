use super::*;
use serde_json::json;
use vtcode_core::config::constants::tools;

fn success(output: serde_json::Value) -> ToolPipelineOutcome {
    ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output,
        stdout: None,
        modified_files: vec![],
        command_success: true,
    })
}

fn search_row(line: usize, text: &str) -> serde_json::Value {
    json!({"result_type":"text", "path":"README.md", "line":line, "snippet":text})
}

#[test]
fn concentrated_novel_queries_get_coaching_without_counting_as_churn() {
    let mut tracker = LoopTracker::new();
    for index in 0..12 {
        update_repetition_tracker(
            &mut tracker,
            &success(json!({"results":[search_row(index + 1, &format!("new fact {index}"))]})),
            tools::CODE_SEARCH,
            &json!({"query":format!("q{index}"),"path":"README.md"}),
        );
        assert_eq!(tracker.take_focused_search_notice(), index == 5);
        assert!(!tracker.take_redundant_navigation_notice());
        assert_eq!(tracker.low_signal_tool_calls, 0);
        assert_eq!(tracker.max_low_signal_count(), 0);
    }
    let mut diverse = LoopTracker::new();
    for index in 0..12 {
        update_repetition_tracker(
            &mut diverse,
            &success(
                json!({"results":[{"result_type":"text","path":format!("file{index}.rs"),"line":1,"snippet":"new source"}]}),
            ),
            tools::CODE_SEARCH,
            &json!({"query":"new source"}),
        );
        assert!(!diverse.take_focused_search_notice());
    }
}

#[test]
fn different_queries_with_retained_rows_are_redundant_but_new_rows_are_productive() {
    let mut tracker = LoopTracker::new();
    for (query, rows, redundant) in [
        ("Overview", vec![search_row(10, "## Overview")], false),
        ("#", vec![search_row(10, "## Overview")], true),
        ("Contents", vec![search_row(10, "## Overview"), search_row(30, "## Contents")], false),
        ("headings", vec![search_row(30, "## Contents")], true),
        ("changed", vec![search_row(10, "## Updated overview")], false),
    ] {
        let previous = tracker.low_signal_tool_calls;
        update_repetition_tracker(
            &mut tracker,
            &success(json!({"results":rows})),
            tools::CODE_SEARCH,
            &json!({"query":query, "path":"README.md"}),
        );
        assert_eq!(tracker.low_signal_tool_calls - previous, u32::from(redundant));
        assert!(!tracker.verification_is_pending());
        assert_eq!(tracker.fix_edits_remaining, 0);
    }
    assert!(tracker.take_redundant_navigation_notice());
    tracker.reset_after_balancer_recovery();
    let previous = tracker.low_signal_tool_calls;
    update_repetition_tracker(
        &mut tracker,
        &success(json!({"results":[search_row(30,"## Contents")]})),
        tools::CODE_SEARCH,
        &json!({"query":"another query"}),
    );
    assert_eq!(tracker.low_signal_tool_calls, previous + 1, "retained evidence stays redundant after synthesis");
    assert!(!tracker.take_redundant_navigation_notice(), "coaching stays once per turn");
}

#[test]
fn overlapping_sed_pages_only_count_as_redundant_when_all_lines_were_seen() {
    let mut tracker = LoopTracker::new();
    for (command, output, redundant) in [
        ("sed -n '30,32p' .vtcode/context/tool_outputs/check.txt", "alpha\nbeta\ngamma\n", false),
        ("sed -n '31,32p' .vtcode/context/tool_outputs/check.txt", "beta\ngamma\n", true),
        ("sed -n '32,33p' .vtcode/context/tool_outputs/check.txt", "gamma\ndelta\n", false),
        ("sed -n '30,33p' .vtcode/context/tool_outputs/check.txt", "alpha\nbeta\ngamma\ndelta\n", true),
    ] {
        let previous = tracker.low_signal_tool_calls;
        update_repetition_tracker(
            &mut tracker,
            &success(json!({"output":output,"exit_code":0})),
            tools::EXEC_COMMAND,
            &json!({"cmd":command}),
        );
        assert_eq!(tracker.low_signal_tool_calls - previous, u32::from(redundant));
    }
    let previous = tracker.low_signal_tool_calls;
    update_repetition_tracker(
        &mut tracker,
        &success(json!({"output":"beta\ngamma\n","exit_code":0})),
        tools::EXEC_COMMAND,
        &json!({"cmd":"sed -n '31,32p' .vtcode/context/tool_outputs/check.txt", "workdir":"other"}),
    );
    assert_eq!(tracker.low_signal_tool_calls, previous, "a different cwd is different evidence");
}

#[test]
fn running_sed_chunks_do_not_establish_positioned_evidence() {
    let args = json!({"cmd":"sed -n '10,12p' README.md"});
    for output in [
        json!({"output":"alpha\n", "session_id":"running"}),
        json!({"output":"alpha\n", "exit_code":null}),
    ] {
        let mut tracker = LoopTracker::new();
        for _ in 0..4 {
            update_repetition_tracker(&mut tracker, &success(output.clone()), tools::EXEC_COMMAND, &args);
        }
        assert!(!tracker.take_redundant_navigation_notice());
        assert_eq!(tracker.low_signal_tool_calls, 0);
        update_repetition_tracker(
            &mut tracker,
            &success(json!({"output":"alpha\n", "exit_code":0})),
            tools::EXEC_COMMAND,
            &args,
        );
        assert!(!tracker.take_redundant_navigation_notice(), "terminal output must supply fresh evidence");
        assert_eq!(tracker.low_signal_tool_calls, 0);
    }
}

#[test]
fn incomplete_failed_or_unpositioned_results_cannot_establish_redundant_evidence() {
    for output in [
        json!({"results":[search_row(10,"Overview")], "truncated":true}),
        json!({"results":[search_row(10,"Overview")], "error":"failed"}),
        json!({"results":[{"path":"README.md","snippet":"Overview"}]}),
        json!({"results":[search_row(10,"Overview"), {"result_type":"definition"}]}),
    ] {
        let mut tracker = LoopTracker::new();
        update_repetition_tracker(&mut tracker, &success(output), tools::CODE_SEARCH, &json!({"query":"Overview"}));
        update_repetition_tracker(
            &mut tracker,
            &success(json!({"results":[search_row(10,"Overview")]})),
            tools::CODE_SEARCH,
            &json!({"query":"another"}),
        );
        assert!(!tracker.take_redundant_navigation_notice());
    }
    for status in [
        ToolExecutionStatus::Cancelled,
        ToolExecutionStatus::Failure {
            error: vtcode_core::tools::registry::ToolExecutionError::policy_violation(tools::CODE_SEARCH, "denied"),
        },
    ] {
        let mut tracker = LoopTracker::new();
        update_repetition_tracker(
            &mut tracker,
            &ToolPipelineOutcome::from_status(status),
            tools::CODE_SEARCH,
            &json!({"query":"Overview"}),
        );
        assert!(!tracker.take_redundant_navigation_notice());
        assert_eq!(tracker.low_signal_tool_calls, 0);
    }
}

#[test]
fn successful_docs_edit_invalidates_retained_search_evidence() {
    let mut tracker = LoopTracker::new();
    let hit = success(json!({"results":[search_row(10,"## Overview")]}));
    let args = json!({"query":"Overview", "path":"README.md"});
    update_repetition_tracker(&mut tracker, &hit, tools::CODE_SEARCH, &args);
    update_repetition_tracker(
        &mut tracker,
        &success(json!({"success":true})),
        tools::APPLY_PATCH,
        &json!({"input":"*** Begin Patch\n*** Update File: README.md\n@@\n-old\n+new\n*** End Patch\n"}),
    );
    update_repetition_tracker(&mut tracker, &hit, tools::CODE_SEARCH, &args);
    assert_eq!(tracker.low_signal_tool_calls, 0);
    assert!(!tracker.take_redundant_navigation_notice());
}

#[test]
fn context_compaction_discards_evidence_without_replenishing_notices_or_verifier_state() {
    let mut tracker = LoopTracker::with_verification_snapshot((true, 1));
    let signatures = vec!["retained line".to_string()];
    assert!(!tracker.record_navigation_evidence(&signatures));
    assert!(tracker.record_navigation_evidence(&signatures));
    assert!(tracker.take_redundant_navigation_notice());
    tracker.clear_navigation_evidence();
    assert!(tracker.verification_is_pending());
    assert_eq!(tracker.fix_edits_remaining, 1);
    assert!(!tracker.record_navigation_evidence(&signatures), "cleared context needs fresh evidence");
    assert!(tracker.record_navigation_evidence(&signatures));
    assert!(!tracker.take_redundant_navigation_notice(), "compaction cannot replenish coaching");
}

#[test]
fn bounded_evidence_ledger_keeps_unrecorded_rows_productive() {
    let mut tracker = LoopTracker::new();
    let signatures: Vec<_> = (0..4100).map(|n| format!("row{n}")).collect();
    assert!(!tracker.record_navigation_evidence(&signatures));
    assert!(tracker.record_navigation_evidence(&signatures[..4096]));
    assert!(!tracker.record_navigation_evidence(&signatures[4096..]));
}

#[test]
fn balancer_recovery_preserves_retained_evidence_but_resets_churn() {
    // Synthesis leaves context available, so it must not make an unchanged
    // read novel. Only churn counters reset before the bounded execution phase.
    let mut tracker = LoopTracker::new();
    let signatures = vec!["retained line".to_string()];
    assert!(!tracker.record_navigation_evidence(&signatures));
    assert!(tracker.record_navigation_evidence(&signatures));
    tracker.reset_after_balancer_recovery();
    assert!(tracker.record_navigation_evidence(&signatures), "post-recovery resume must remember retained rows");
}
