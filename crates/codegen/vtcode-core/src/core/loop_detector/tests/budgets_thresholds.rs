use super::*;

// --- Global readonly budget tests ---

#[test]
fn global_readonly_budget_fires_at_limit() {
    let mut detector = LoopDetector::with_max_repeated_calls(100);
    let mut saw_hard_stop = false;

    // Make MAX_TOTAL_READONLY_CALLS calls with different args to avoid per-tool detection
    for i in 0..MAX_TOTAL_READONLY_CALLS {
        let args = json!({"query": format!("pattern_{i}"), "path": "src/"});
        if let Some(msg) = detector.record_call(tools::CODE_SEARCH, &args)
            && msg.contains("global read-only budget")
        {
            saw_hard_stop = true;
            break;
        }
    }

    assert!(saw_hard_stop, "Global readonly budget should fire at {MAX_TOTAL_READONLY_CALLS}");
    assert!(detector.is_hard_limit_exceeded(tools::CODE_SEARCH));
}
#[test]
fn global_readonly_budget_counts_across_different_tools() {
    let mut detector = LoopDetector::with_max_repeated_calls(100);
    let mut hard_stop_count = 0;

    // Alternate between code_search and file_operation to evade per-tool limits.
    for i in 0..MAX_TOTAL_READONLY_CALLS + 5 {
        if i % 2 == 0 {
            let args = json!({"query": format!("p_{i}"), "path": "src/"});
            if let Some(msg) = detector.record_call(tools::CODE_SEARCH, &args)
                && msg.contains("global read-only budget")
            {
                hard_stop_count += 1;
            }
        } else {
            let args = json!({"action": "read", "path": format!("src/file_{i}.rs")});
            if let Some(msg) = detector.record_call(tools::UNIFIED_FILE, &args)
                && msg.contains("global read-only budget")
            {
                hard_stop_count += 1;
            }
        }
    }

    assert!(hard_stop_count > 0, "Global budget should fire when alternating tools");
    assert_eq!(detector.total_readonly_calls(), MAX_TOTAL_READONLY_CALLS + 5);
}
#[test]
fn exhausted_global_readonly_budget_blocks_every_tool_and_says_so() {
    let mut detector = LoopDetector::with_max_repeated_calls(100);
    for i in 0..MAX_TOTAL_READONLY_CALLS {
        let args = json!({"query": format!("p_{i}"), "path": "src/"});
        detector.record_call(tools::CODE_SEARCH, &args);
    }

    let edit = json!({"path": "src/new.rs", "content": "fn main() {}"});
    let msg = detector
        .record_call(tools::WRITE_FILE, &edit)
        .expect("an edit after the budget is exhausted is also stopped");
    assert!(msg.starts_with(HARD_STOP_PREFIX), "{msg}");
    assert!(
        msg.contains("so every further tool call this turn is blocked, including edits and commands."),
        "{msg}"
    );
    assert!(!msg.contains("further read-only calls are blocked"), "{msg}");
}
#[test]
fn global_readonly_budget_resets_on_reset() {
    let mut detector = LoopDetector::with_max_repeated_calls(100);

    // Make some readonly calls
    for i in 0..10 {
        let args = json!({"query": format!("p_{i}"), "path": "src/"});
        detector.record_call(tools::CODE_SEARCH, &args);
    }
    assert_eq!(detector.total_readonly_calls(), 10);

    detector.reset();
    assert_eq!(detector.total_readonly_calls(), 0);
}
#[test]
fn global_readonly_budget_not_incremented_by_mutating_tools() {
    let mut detector = LoopDetector::with_max_repeated_calls(100);

    // Make mutating calls
    for i in 0..5 {
        let args = json!({"path": format!("src/file_{i}.rs"), "content": "fn main() {}"});
        detector.record_call(tools::WRITE_FILE, &args);
    }

    assert_eq!(detector.total_readonly_calls(), 0);
}
#[test]
fn lowered_navigation_loop_thresholds() {
    let mut detector = LoopDetector::with_max_repeated_calls(100);

    // Make 3 read-only calls (below threshold)
    for i in 0..3 {
        let args = json!({"query": format!("p_{i}"), "path": "src/"});
        assert!(detector.record_call(tools::CODE_SEARCH, &args).is_none(), "Call {i} should not trigger warning");
    }

    // 4th call should trigger navigation loop warning (streak hits 4)
    let args = json!({"query": "p_4", "path": "src/"});
    let warning = detector.record_call(tools::CODE_SEARCH, &args);
    assert!(warning.is_some(), "Navigation loop warning should fire at streak 4");
    assert!(warning.unwrap().contains("Navigation loop detected"));
}
#[test]
fn navigation_hard_stop_at_lowered_threshold() {
    let mut detector = LoopDetector::with_max_repeated_calls(100);

    // Make 7 read-only calls (new hard stop threshold)
    for i in 0..6 {
        let args = json!({"query": format!("p_{i}"), "path": "src/"});
        detector.record_call(tools::CODE_SEARCH, &args);
    }

    // 7th call should trigger HARD STOP
    let args = json!({"query": "p_7", "path": "src/"});
    let warning = detector.record_call(tools::CODE_SEARCH, &args);
    assert!(warning.is_some(), "Navigation hard stop should fire at streak 7");
    let msg = warning.unwrap();
    assert!(msg.contains(HARD_STOP_PREFIX), "Expected hard stop: {msg}");
    assert!(detector.is_hard_limit_exceeded(tools::CODE_SEARCH));
}
#[test]
fn subagent_navigation_hard_stop_at_5() {
    let mut detector = LoopDetector::with_max_repeated_calls(100);
    detector.set_subagent_mode(true);

    // Make 4 read-only calls (subagent warning at 3, but no hard stop yet)
    for i in 0..4 {
        let args = json!({"query": format!("p_{i}"), "path": "src/"});
        detector.record_call(tools::CODE_SEARCH, &args);
    }

    // 5th call should trigger HARD STOP for subagent
    let args = json!({"query": "p_5", "path": "src/"});
    let warning = detector.record_call(tools::CODE_SEARCH, &args);
    assert!(warning.is_some(), "Subagent navigation hard stop should fire at streak 5");
    let msg = warning.unwrap();
    assert!(msg.contains(HARD_STOP_PREFIX), "Expected hard stop: {msg}");
}
#[test]
fn subagent_readonly_budget_at_20() {
    let mut detector = LoopDetector::with_max_repeated_calls(100);
    detector.set_subagent_mode(true);

    // Make 20 read-only calls with different args to avoid per-tool detection
    for i in 0..20 {
        let args = json!({"query": format!("unique_{i}"), "path": "src/"});
        detector.record_call(tools::CODE_SEARCH, &args);
    }

    // 21st call should trigger global budget HARD STOP
    let args = json!({"query": "final", "path": "src/"});
    let warning = detector.record_call(tools::CODE_SEARCH, &args);
    assert!(warning.is_some(), "Subagent global read-only budget should fire at 20");
    let msg = warning.unwrap();
    assert!(msg.contains("global read-only budget is exhausted"), "Expected budget exhaustion: {msg}");
    assert!(msg.contains("limit: 20"), "Expected limit 20 in message: {msg}");
}
#[test]
fn main_agent_readonly_budget_still_30() {
    let mut detector = LoopDetector::with_max_repeated_calls(100);
    // NOT setting subagent mode -- default is false

    // Make 30 read-only calls
    for i in 0..30 {
        let args = json!({"query": format!("unique_{i}"), "path": "src/"});
        detector.record_call(tools::CODE_SEARCH, &args);
    }

    // 31st call should trigger global budget HARD STOP
    let args = json!({"query": "final", "path": "src/"});
    let warning = detector.record_call(tools::CODE_SEARCH, &args);
    assert!(warning.is_some(), "Main agent global read-only budget should fire at 30");
    let msg = warning.unwrap();
    assert!(msg.contains("limit: 30"), "Expected limit 30 in message: {msg}");
}
