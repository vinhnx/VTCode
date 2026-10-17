use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;
use serde_json::json;

#[tokio::test]
async fn pre_execution_responses_consume_streamed_identity_without_an_emitter() {
    use crate::agent::runloop::unified::run_loop_context::StreamedToolCallItem;

    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    ctx.harness_emitter = None;
    ctx.harness_state
        .remember_streamed_tool_call_items(["cached", "rejected"].map(|id| {
            (
                id.to_owned(),
                StreamedToolCallItem {
                    item_id: format!("streamed-{id}"),
                    tool_name: "read_file".to_owned(),
                },
            )
        }));
    ctx.push_reused_tool_response("cached", "read_file", &json!({"path":"sample.txt"}), "cached text".to_owned());
    assert!(ctx.harness_state.take_streamed_tool_call_item_id("cached").is_none());
    // Consuming the cached call must leave the separate rejection available.
    let remaining = ctx.harness_state.take_all_streamed_tool_call_item_ids();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].0, "rejected");
    assert_eq!(remaining[0].1.item_id, "streamed-rejected");
    ctx.harness_state.remember_streamed_tool_call_items(remaining);
    ctx.emit_rejected_tool_call_item("rejected", Some("read_file"), None, "permission denied");
    assert!(ctx.harness_state.take_all_streamed_tool_call_item_ids().is_empty());
    assert_eq!(ctx.harness_state.tool_calls, 0);
    let response = ctx.working_history.last().unwrap();
    assert_eq!(response.tool_call_id.as_deref(), Some("cached"));
    assert_eq!(response.content.as_text(), "cached text");
}

#[tokio::test]
async fn reused_responses_preserve_terminal_evidence_without_streamed_starts() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("events.jsonl");
    let emitter = HarnessEventEmitter::new(path.clone()).unwrap();
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    ctx.harness_emitter = Some(&emitter);
    for (id, cmd, output, expected_status, expected_text) in [
        (
            "cached_ok",
            "sed -n '1p' file.txt",
            json!({"output":"asymmetric cached text", "exit_code":0}),
            "completed",
            Some("asymmetric cached text"),
        ),
        (
            "cached_spool",
            "cat file.txt",
            json!({"output":"retained in spool", "exit_code":0, "spool_path":"cached.txt"}),
            "completed",
            Some(""),
        ),
        ("cached_empty", "grep absent file.txt", json!({"output":"", "exit_code":1}), "completed", None),
        (
            "cached_failure",
            "grep --invalid file.txt",
            json!({"stderr":"invalid option", "exit_code":2}),
            "failed",
            Some("invalid option"),
        ),
        (
            "cached_plain",
            "cat file.txt",
            json!("plain cached output"),
            "completed",
            Some("plain cached output"),
        ),
    ] {
        ctx.push_reused_tool_response(id, "exec_command", &json!({"cmd":cmd}), output.to_string());
        let events = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        let matching = events
            .iter()
            .map(|record| &record["event"])
            .filter(|event| event["item"]["tool_call_id"] == id)
            .collect::<Vec<_>>();
        assert_eq!(matching.iter().filter(|event| event["type"] == "item.started").count(), 2);
        let completed = matching
            .iter()
            .filter(|event| event["type"] == "item.completed")
            .collect::<Vec<_>>();
        assert_eq!(completed.len(), 2);
        let terminal = completed.iter().find(|event| event["item"]["type"] == "tool_output").unwrap();
        assert_eq!(terminal["item"]["status"], expected_status);
        assert_eq!(terminal["item"]["exit_code"], output.get("exit_code").cloned().unwrap_or_default());
        assert_eq!(terminal["item"]["spool_path"], output.get("spool_path").cloned().unwrap_or_default());
        if let Some(expected_text) = expected_text {
            assert_eq!(terminal["item"]["output"], expected_text);
        }
        assert_eq!(ctx.harness_state.tool_calls, 0, "reuse must not charge a fresh execution");
    }
    assert!(ctx.harness_state.take_all_streamed_tool_call_item_ids().is_empty());
}
