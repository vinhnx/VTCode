use super::*;
use crate::llm::provider::{Message, MessageRole};
/// Helper: Create test messages
fn make_tool_call(call_id: &str, tool_name: &str) -> Message {
    Message::assistant_with_tools(
        "".to_string(),
        vec![crate::llm::provider::ToolCall::function(
            call_id.to_string(),
            tool_name.to_string(),
            "{}".to_string(),
        )],
    )
}

fn make_tool_response(call_id: &str, content: &str) -> Message {
    Message::tool_response(call_id.to_string(), content.to_string())
}

/// Test: Valid history with matched calls and outputs
#[test]
fn test_validate_history_valid_matched_pairs() {
    let mut messages = vec![
        make_tool_call("call_1", "list_files"),
        make_tool_response("call_1", "file1.rs\nfile2.rs"),
    ];

    let report = validate_history_invariants(&messages);
    assert!(report.is_valid(), "Valid paired call/output should pass");
    assert!(report.missing_outputs.is_empty());
    assert!(report.orphan_outputs.is_empty());

    // Normalize should be a no-op
    normalize_history(&mut messages);
    assert_eq!(messages.len(), 2);
}

/// Test: Missing output (tool call without response)
#[test]
fn test_validate_history_missing_output() {
    let messages = vec![make_tool_call("call_1", "list_files")];

    let report = validate_history_invariants(&messages);
    assert!(!report.is_valid());
    assert_eq!(report.missing_outputs.len(), 1);
    assert_eq!(report.missing_outputs[0].call_id.as_str(), "call_1");
    assert!(report.orphan_outputs.is_empty());
}

/// Test: Orphan output (response without corresponding call)
#[test]
fn test_validate_history_orphan_output() {
    let messages = vec![make_tool_response("orphan_call", "Some result")];

    let report = validate_history_invariants(&messages);
    assert!(!report.is_valid());
    assert!(report.missing_outputs.is_empty());
    assert_eq!(report.orphan_outputs.len(), 1);
    assert_eq!(report.orphan_outputs[0].as_str(), "orphan_call");
}

/// Test: ensure_call_outputs_present creates synthetic outputs
#[test]
fn test_ensure_call_outputs_present() {
    let mut messages = vec![make_tool_call("call_1", "list_files")];
    let initial_len = messages.len();

    ensure_call_outputs_present(&mut messages);

    assert_eq!(messages.len(), initial_len + 1);
    let last_msg = &messages[initial_len];
    assert_eq!(last_msg.tool_call_id, Some("call_1".to_string()));
    assert!(last_msg.content.as_text().contains("canceled"));

    let report = validate_history_invariants(&messages);
    assert!(report.is_valid());
}

/// Test: remove_orphan_outputs filters out orphaned responses
#[test]
fn test_remove_orphan_outputs() {
    let mut messages = vec![
        make_tool_call("call_1", "list_files"),
        make_tool_response("call_1", "valid result"),
        make_tool_response("orphan_call", "orphan result"),
    ];

    let initial_len = messages.len();
    remove_orphan_outputs(&mut messages);

    assert_eq!(messages.len(), initial_len - 1);
    assert!(
        messages
            .iter()
            .any(|msg| msg.tool_call_id.as_ref().is_some_and(|id| id == "call_1"))
    );
    assert!(
        !messages
            .iter()
            .any(|msg| { msg.tool_call_id.as_ref().is_some_and(|id| id == "orphan_call") })
    );

    let report = validate_history_invariants(&messages);
    assert!(report.is_valid());
}

/// Test: normalize() applies both fixes (synthetic output + orphan removal)
#[test]
fn test_normalize_combined_fixes() {
    let mut messages = vec![
        make_tool_call("call_1", "read_file"),
        make_tool_call("call_2", "write_file"),
        make_tool_response("call_2", "written"),
        make_tool_response("orphan", "orphan result"),
    ];

    normalize_history(&mut messages);

    let report = validate_history_invariants(&messages);
    assert!(report.is_valid());
    assert!(
        messages
            .iter()
            .any(|msg| msg.tool_call_id.as_ref().is_some_and(|id| id == "call_1"))
    );
    assert!(
        !messages
            .iter()
            .any(|msg| msg.tool_call_id.as_ref().is_some_and(|id| id == "orphan"))
    );
}

#[test]
fn request_normalization_shared_reuses_clean_history_arc() {
    let messages = Arc::new(vec![Message::user("clean history".to_string())]);

    let normalized = normalize_history_for_request_shared(Arc::clone(&messages));

    assert!(Arc::ptr_eq(&messages, &normalized));
}

#[test]
fn request_normalization_reused_ids_match_causally_preceding_batches() {
    let messages = Arc::new(vec![
        make_tool_response("reused", "early result"),
        make_tool_call("reused", "read_file"),
        make_tool_response("reused", "first result"),
        make_tool_call("reused", "write_file"),
        Message::user("interleaved".to_string()),
        make_tool_response("reused", "second result"),
        make_tool_response("reused", "duplicate result"),
    ]);
    let normalized = normalize_history_for_request_shared(Arc::clone(&messages));
    assert_eq!(
        normalized.as_ref(),
        &vec![
            make_tool_call("reused", "read_file"),
            make_tool_response("reused", "first result"),
            make_tool_call("reused", "write_file"),
            make_tool_response("reused", "second result"),
            Message::user("interleaved".to_string()),
        ]
    );
    assert_eq!(messages.len(), 7);
    assert_eq!(messages[0].content.as_text(), "early result");
}

#[test]
fn request_normalization_shared_repairs_asymmetric_batch_without_mutating_source() {
    let messages = Arc::new(vec![
        Message::assistant_with_tools(
            "three calls".to_string(),
            ["call_a", "call_b", "call_c"]
                .into_iter()
                .map(|id| {
                    crate::llm::provider::ToolCall::function(id.to_string(), "read_file".to_string(), "{}".to_string())
                })
                .collect(),
        ),
        Message::user("interleaved request".to_string()),
        make_tool_response("call_b", "result B"),
        make_tool_response("call_a", "result A"),
        make_tool_response("call_b", "duplicate B"),
    ]);
    let source_before = messages.as_ref().clone();

    let normalized = normalize_history_for_request_shared(Arc::clone(&messages));

    assert!(!Arc::ptr_eq(&messages, &normalized));
    assert_eq!(normalized.len(), 5);
    assert_eq!(normalized[0], source_before[0]);
    assert_eq!(normalized[1], make_tool_response("call_b", "result B"));
    assert_eq!(normalized[2], make_tool_response("call_a", "result A"));
    assert_eq!(normalized[3].role, MessageRole::Tool);
    assert_eq!(normalized[3].tool_call_id.as_deref(), Some("call_c"));
    assert_eq!(
        normalized[3].content.as_text(),
        "canceled: no tool result was recorded; this bounded placeholder preserves the tool-call protocol."
    );
    assert_eq!(normalized[4], Message::user("interleaved request".to_string()));
    assert_eq!(messages.as_ref(), &source_before);
    let repeated = normalize_history_for_request_shared(Arc::clone(&normalized));
    assert!(Arc::ptr_eq(&normalized, &repeated));
}

#[test]
fn request_normalization_groups_split_results_after_intervening_messages() {
    let messages = vec![
        Message::assistant_with_tools(
            "".to_string(),
            vec![
                crate::llm::provider::ToolCall::function(
                    "call_1".to_string(),
                    "read_file".to_string(),
                    "{}".to_string(),
                ),
                crate::llm::provider::ToolCall::function(
                    "call_2".to_string(),
                    "read_file".to_string(),
                    "{}".to_string(),
                ),
            ],
        ),
        Message::system("intervening system note".to_string()),
        make_tool_response("call_2", "result two"),
        Message::user("intervening user note".to_string()),
        make_tool_response("call_1", "result one"),
        Message::assistant("done".to_string()),
    ];

    let normalized = normalize_history_for_request(&messages);

    assert_eq!(normalized[0].role, MessageRole::Assistant);
    assert_eq!(normalized[1].tool_call_id.as_deref(), Some("call_2"));
    assert_eq!(normalized[1].content.as_text(), "result two");
    assert_eq!(normalized[2].tool_call_id.as_deref(), Some("call_1"));
    assert_eq!(normalized[2].content.as_text(), "result one");
    assert_eq!(normalized[3].role, MessageRole::System);
    assert_eq!(normalized[4].role, MessageRole::User);
    assert_eq!(normalized[5].role, MessageRole::Assistant);
}

#[test]
fn request_normalization_drops_early_orphan_duplicate_and_synthesizes_missing_results() {
    let messages = vec![
        make_tool_response("call_1", "causally early"),
        make_tool_response("orphan", "orphaned"),
        make_tool_call("call_1", "read_file"),
        make_tool_call("call_2", "read_file"),
        make_tool_response("call_1", "valid"),
        make_tool_response("call_1", "duplicate"),
    ];

    let normalized = normalize_history_for_request(&messages);

    assert_eq!(normalized.len(), 4);
    assert_eq!(normalized[0].role, MessageRole::Assistant);
    assert_eq!(normalized[0].tool_calls.as_ref().map(Vec::len), Some(1));
    assert_eq!(normalized[1].tool_call_id.as_deref(), Some("call_1"));
    assert_eq!(normalized[1].content.as_text(), "valid");
    assert_eq!(normalized[2].role, MessageRole::Assistant);
    assert_eq!(normalized[3].tool_call_id.as_deref(), Some("call_2"));
    assert!(normalized[3].content.as_text().contains("bounded placeholder"));
}

#[test]
fn request_normalization_is_idempotent_and_does_not_mutate_durable_history() {
    let messages = vec![
        make_tool_call("call_1", "read_file"),
        Message::system("intervening".to_string()),
        make_tool_response("call_1", "result"),
    ];
    let durable_before = messages.clone();

    assert!(request_history_needs_normalization(&messages));
    let normalized = normalize_history_for_request(&messages);
    assert!(!request_history_needs_normalization(&normalized));
    assert_eq!(normalize_history_for_request(&normalized), normalized);
    assert_eq!(messages, durable_before);
}

/// Test: recover_history_from_crash handles both missing and orphan outputs
#[test]
fn test_recover_from_crash() {
    let mut messages = vec![
        make_tool_call("crashed_call", "dangerous_op"),
        make_tool_response("old_call", "stale result"),
    ];

    recover_history_from_crash(&mut messages);

    let report = validate_history_invariants(&messages);
    assert!(report.is_valid());
    assert!(
        messages
            .iter()
            .any(|msg| { msg.tool_call_id.as_ref().is_some_and(|id| id == "crashed_call") })
    );
    assert!(
        !messages
            .iter()
            .any(|msg| msg.tool_call_id.as_ref().is_some_and(|id| id == "old_call"))
    );
}

/// Test: HistoryValidationReport summary messages
#[test]
fn test_validation_report_summary() {
    let valid = HistoryValidationReport::default();
    assert_eq!(valid.summary(), "History invariants are valid");
    assert!(valid.is_valid());

    let invalid = HistoryValidationReport {
        missing_outputs: vec![
            MissingOutput {
                call_id: ToolCallId::new("call_1"),
                tool_name: "tool_a".into(),
            },
            MissingOutput {
                call_id: ToolCallId::new("call_2"),
                tool_name: "tool_b".into(),
            },
        ],
        orphan_outputs: vec![ToolCallId::new("orphan_1")],
    };
    assert_eq!(invalid.summary(), "2 missing outputs, 1 orphan outputs");
    assert!(!invalid.is_valid());
}

/// Test: Multiple tool calls with selective missing outputs
#[test]
fn test_multiple_calls_partial_outputs() {
    let _messages: Vec<Message> = (1..=3)
        .flat_map(|i| {
            vec![
                make_tool_call(&format!("call_{i}"), &format!("tool_{i}")),
                if i != 2 {
                    make_tool_response(&format!("call_{i}"), &format!("result_{i}"))
                } else {
                    // Simulate a gap: we don't add a response for call_2 here directly,
                    // but we need to build messages differently.
                    // Instead, build manually below.
                    Message::tool_response("placeholder".into(), "".into())
                },
            ]
        })
        .collect();
    // Redo: explicit construction
    let mut messages = vec![
        make_tool_call("call_1", "tool_1"),
        make_tool_response("call_1", "result_1"),
        make_tool_call("call_2", "tool_2"),
        make_tool_call("call_3", "tool_3"),
        make_tool_response("call_3", "result_3"),
    ];

    let report = validate_history_invariants(&messages);
    assert!(!report.is_valid());
    assert_eq!(report.missing_outputs.len(), 1);
    assert_eq!(report.missing_outputs[0].call_id.as_str(), "call_2");

    normalize_history(&mut messages);
    assert!(validate_history_invariants(&messages).is_valid());
}

/// Test: OutputStatus enum conversion
#[test]
fn test_output_status_as_str() {
    assert_eq!(OutputStatus::Success.as_str(), "success");
    assert_eq!(OutputStatus::Failed.as_str(), "failed");
    assert_eq!(OutputStatus::Canceled.as_str(), "canceled");
    assert_eq!(OutputStatus::Timeout.as_str(), "timeout");
}

/// Test: find_safe_split_point maintains call/output pairs
#[test]
fn test_find_safe_split_point() {
    let messages = vec![
        Message::user("User 1".into()),           // 0
        make_tool_call("call_a", "tool_a"),       // 1
        make_tool_response("call_a", "Result A"), // 2
        make_tool_call("call_b", "tool_b"),       // 3
        make_tool_response("call_b", "Result B"), // 4
    ];
    let conversation_len = 5;

    // Split at 3 means keeping 3,4. But response at 2 needs call at 1 -> must split at 2.
    let safe = safe_history_split_point(&messages, conversation_len, 3);
    assert_eq!(safe, 2, "Should move split to include Call A");

    // Split at 4 is safe: call_b (3) and response_b (4) are both kept.
    let safe2 = safe_history_split_point(&messages, conversation_len, 4);
    assert_eq!(safe2, 4, "Should stay at 4 as it is safe");
}

#[test]
fn test_summarize_list_formatting() {
    assert_eq!(summarize_list(&[]), "none");
    assert_eq!(summarize_list(&["a".into()]), "a");
    assert_eq!(summarize_list(&["a".into(), "b".into()]), "a, b");
    let many: Vec<String> = (1..=7).map(|i| format!("item{i}")).collect();
    let result = summarize_list(&many);
    assert!(result.contains("item1, item2, item3, item4, item5"));
    assert!(result.contains("[+2 more]"));
}

fn bulky_tool_history(count: usize, body_chars: usize) -> Vec<Message> {
    let mut messages = vec![Message::user("start".to_string())];
    for i in 0..count {
        let call_id = format!("call_{i}");
        messages.push(make_tool_call(&call_id, "read_file"));
        messages.push(make_tool_response(&call_id, &"x".repeat(body_chars)));
    }
    messages.push(Message::assistant("done".to_string()));
    messages
}

fn is_cleared_stub(message: &Message) -> bool {
    message.content.as_text().contains("\"cleared\":\"tool_result\"")
}

#[test]
fn clear_old_tool_results_noop_below_trigger() {
    let messages = bulky_tool_history(3, 200);
    let cleared = clear_old_tool_results(&messages, u64::MAX, 1, 1, false);
    assert_eq!(cleared.len(), messages.len());
    assert!(
        cleared
            .iter()
            .filter(|m| m.role == MessageRole::Tool)
            .all(|m| !is_cleared_stub(m))
    );
}

#[test]
fn clear_old_tool_results_keeps_newest_and_stubs_all_older() {
    // Four large tool results; keep 1. Every non-kept result is stubbed so
    // re-shaping full durable history cannot leave a growing tail.
    let messages = bulky_tool_history(4, 4_000);
    let cleared = clear_old_tool_results(&messages, 1, 1, 1, false);
    let tool_msgs: Vec<&Message> = cleared.iter().filter(|m| m.role == MessageRole::Tool).collect();
    assert_eq!(tool_msgs.len(), 4);
    assert!(is_cleared_stub(tool_msgs[0]), "oldest result is cleared");
    assert!(is_cleared_stub(tool_msgs[1]));
    assert!(is_cleared_stub(tool_msgs[2]));
    assert!(!is_cleared_stub(tool_msgs[3]), "newest result stays intact");
    // Protocol pairing survives.
    for (original, rewritten) in messages.iter().zip(cleared.iter()) {
        assert_eq!(original.role, rewritten.role);
        assert_eq!(original.tool_call_id, rewritten.tool_call_id);
    }
}

#[test]
fn clear_old_tool_results_respects_keep_tool_uses() {
    let messages = bulky_tool_history(5, 3_000);
    let cleared = clear_old_tool_results(&messages, 1, 3, 1, false);
    let tool_msgs: Vec<&Message> = cleared.iter().filter(|m| m.role == MessageRole::Tool).collect();
    assert_eq!(tool_msgs.len(), 5);
    assert!(!is_cleared_stub(tool_msgs[2]));
    assert!(!is_cleared_stub(tool_msgs[3]));
    assert!(!is_cleared_stub(tool_msgs[4]));
}

#[test]
fn clear_old_tool_results_optional_clear_tool_inputs() {
    let mut messages = bulky_tool_history(3, 3_000);
    // Attach a recognizable argument payload on the first call.
    if let Some(call) = messages
        .get_mut(1)
        .and_then(|m| m.tool_calls.as_mut())
        .and_then(|calls| calls.first_mut())
        .and_then(|call| call.function.as_mut())
    {
        call.arguments = serde_json::json!({"path": "secret-path", "padding": "x".repeat(1024)}).to_string();
    }

    let cleared = clear_old_tool_results(&messages, 1, 1, 1, true);
    let first_call_args = cleared
        .get(1)
        .and_then(|m| m.tool_calls.as_ref())
        .and_then(|calls| calls.first())
        .and_then(|call| call.function.as_ref())
        .map(|function| function.arguments.as_str())
        .unwrap_or_default();
    assert!(!first_call_args.contains("secret-path"), "cleared call arguments must drop the original payload");
    assert!(
        first_call_args.contains("\"cleared\":\"tool_input\""),
        "cleared arguments must be a JSON placeholder, got {first_call_args:?}"
    );
    assert!(
        serde_json::from_str::<serde_json::Value>(first_call_args).is_ok(),
        "cleared arguments must stay valid JSON for the wire"
    );
    // Newest retained call keeps its arguments.
    let last_call_args = cleared
        .get(5)
        .and_then(|m| m.tool_calls.as_ref())
        .and_then(|calls| calls.first())
        .and_then(|call| call.function.as_ref())
        .map(|function| function.arguments.as_str())
        .unwrap_or_default();
    assert_eq!(last_call_args, "{}");
}

#[test]
fn clear_old_tool_results_default_config_clears_paired_inputs() {
    // Regression: `clear_tool_inputs` now defaults to true, so the common
    // request-shaping path must drop stale apply_patch/write_file bodies
    // together with the stubbed results.
    let default_clear_tool_inputs = vtcode_config::core::agent::ToolResultClearingConfig::default().clear_tool_inputs;
    assert!(default_clear_tool_inputs, "config default must clear tool inputs");

    let mut messages = bulky_tool_history(3, 3_000);
    if let Some(call) = messages
        .get_mut(1)
        .and_then(|m| m.tool_calls.as_mut())
        .and_then(|calls| calls.first_mut())
        .and_then(|call| call.function.as_mut())
    {
        call.arguments =
            serde_json::json!({"input": format!("*** Begin Patch\n{}\n*** End Patch", "x".repeat(1024))}).to_string();
    }

    let cleared = clear_old_tool_results(&messages, 1, 1, 1, default_clear_tool_inputs);
    let cleared_args = cleared
        .get(1)
        .and_then(|m| m.tool_calls.as_ref())
        .and_then(|calls| calls.first())
        .and_then(|call| call.function.as_ref())
        .map(|function| function.arguments.as_str())
        .unwrap_or_default();
    assert!(
        !cleared_args.contains("Begin Patch"),
        "default-on clearing must drop patch bodies, got {cleared_args:?}"
    );
    assert_eq!(cleared_args, CLEARED_TOOL_INPUT_PLACEHOLDER);

    let kept_args = cleared
        .get(5)
        .and_then(|m| m.tool_calls.as_ref())
        .and_then(|calls| calls.first())
        .and_then(|call| call.function.as_ref())
        .map(|function| function.arguments.as_str())
        .unwrap_or_default();
    assert_eq!(kept_args, "{}", "kept call arguments must stay intact");
}

#[test]
fn clear_old_tool_results_is_idempotent_on_stubs() {
    let messages = bulky_tool_history(4, 3_000);
    let once = clear_old_tool_results(&messages, 1, 1, 1_000, false);
    let twice = clear_old_tool_results(&once, 1, 1, 1_000, false);
    let stubs_once = once.iter().filter(|m| is_cleared_stub(m)).count();
    let stubs_twice = twice.iter().filter(|m| is_cleared_stub(m)).count();
    assert_eq!(stubs_once, stubs_twice);
}

#[test]
fn clear_old_tool_results_leaves_durable_history_unchanged() {
    let messages = bulky_tool_history(3, 2_500);
    let snapshot: Vec<String> = messages.iter().map(|m| m.content.as_text().into_owned()).collect();
    let request_messages = clear_old_tool_results(&messages, 1, 1, 1, false);
    let after: Vec<String> = messages.iter().map(|m| m.content.as_text().into_owned()).collect();
    assert_eq!(snapshot, after, "durable history must not be mutated");
    assert!(
        request_messages
            .iter()
            .filter(|m| m.role == MessageRole::Tool)
            .any(is_cleared_stub),
        "request messages should carry stubs"
    );
}

#[test]
fn local_tool_result_clearing_gate_mirrors_native_edits() {
    use super::should_apply_local_tool_result_clearing as gate;
    assert!(gate("zai", false, true), "non-Anthropic uses local clearing");
    assert!(gate("openai", false, true), "OpenAI uses local clearing");
    assert!(gate("anthropic", false, true), "Anthropic without edits falls back to local");
    assert!(!gate("anthropic", true, true), "Anthropic with context_edits uses native clear_tool_uses only");
    assert!(!gate("zai", false, false), "disabled config never clears");
}

#[test]
fn clear_old_tool_results_stub_is_valid_json_for_non_ascii_tool_names() {
    let mut messages = bulky_tool_history(3, 2_500);
    // Index 2 is the first tool response (call_0).
    messages[2].origin_tool = Some("读取文件".to_string());
    messages[2].tool_call_id = Some("call_ünïcode".to_string());
    let cleared = clear_old_tool_results(&messages, 1, 1, 1, false);
    let stub = cleared
        .iter()
        .filter(|m| is_cleared_stub(m))
        .find(|m| m.content.as_text().contains("call_ünïcode"))
        .expect("stub for the renamed call");
    let parsed: serde_json::Value =
        serde_json::from_str(stub.content.as_text().as_ref()).expect("stub must be valid JSON");
    assert_eq!(parsed["tool"], "读取文件");
    assert_eq!(parsed["tool_call_id"], "call_ünïcode");
}

#[test]
fn clear_old_tool_results_clear_tool_inputs_covers_freeform_text() {
    let mut messages = bulky_tool_history(3, 2_500);
    // Freeform custom tool payload lives in `text`, not function.arguments.
    if let Some(call) = messages
        .get_mut(1)
        .and_then(|m| m.tool_calls.as_mut())
        .and_then(|calls| calls.first_mut())
    {
        call.text = Some("raw freeform payload".repeat(100));
        call.thought_signature = Some("sig".to_string());
    }
    let cleared = clear_old_tool_results(&messages, 1, 1, 1, true);
    let call = cleared
        .get(1)
        .and_then(|m| m.tool_calls.as_ref())
        .and_then(|calls| calls.first())
        .expect("call preserved");
    assert_eq!(call.text.as_deref(), Some(CLEARED_TOOL_INPUT_PLACEHOLDER));
    assert!(call.thought_signature.is_none());
}
