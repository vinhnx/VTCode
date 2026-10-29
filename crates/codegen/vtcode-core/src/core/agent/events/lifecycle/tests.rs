#[test]
fn explanation_file_attribution_ignores_noops_failures_and_unrelated_reads() {
    let args = serde_json::json!({"path":"a.rs", "content":"same"});
    let noop =
        serde_json::json!({"success":true,"diff":[{"path":"a.rs","operation":"updated","is_empty":true,"content":""}]});
    assert!(file_change_completed_event("call", "write_file", &args, &noop).is_none());
    let changed = serde_json::json!({"success":true,"diff":[{"path":"a.rs","operation":"updated","content":"--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new","additions":1,"deletions":1},{"path":"b.rs","operation":"updated","is_empty":true}]});
    let event = file_change_completed_event("call", "write_file", &args, &changed).unwrap();
    let ThreadEvent::ItemCompleted(e) = event else {
        panic!("expected completed file change")
    };
    let ThreadItemDetails::FileChange(change) = e.item.details else {
        panic!("expected file change")
    };
    assert_eq!(change.changes.len(), 1);
    assert_eq!(change.changes[0].path, "a.rs");
    assert_eq!(change.additions, Some(1));
    assert_eq!(change.diff_incomplete, None);
    let incomplete = serde_json::json!({"success":true,"diff":[{"path":"a.rs","operation":"updated","content":"+new","additions":1,"deletions":0,"truncated":true},{"path":"b.rs","operation":"updated","skipped":true}]});
    let ThreadEvent::ItemCompleted(event) =
        file_change_completed_event("second", "write_file", &args, &incomplete).unwrap()
    else {
        panic!("file event")
    };
    let ThreadItemDetails::FileChange(change) = event.item.details else {
        panic!("file details")
    };
    assert_eq!(change.diff_incomplete, Some(true));
    assert_eq!(change.additions, None, "partial totals must remain unavailable");
    assert!(file_change_completed_event("call", "read_file", &args, &changed).is_none());
    assert!(
        file_change_completed_event(
            "call",
            "write_file",
            &args,
            &serde_json::json!({"success":false,"modified_files":["a.rs"]})
        )
        .is_none()
    );
}
use serde_json::json;

use super::*;

#[test]
fn tool_started_event_omits_arguments_when_absent() {
    let event = tool_started_event("item".to_string(), "shell", None, Some("call_1"));
    let ThreadEvent::ItemStarted(ItemStartedEvent { item }) = event else {
        panic!("expected started item");
    };
    let ThreadItemDetails::ToolInvocation(details) = item.details else {
        panic!("expected tool invocation");
    };
    assert!(details.arguments.is_none());
    assert_eq!(details.tool_name, "shell");
}

#[test]
fn tool_output_updated_event_streams_in_progress_output() {
    let event = tool_output_updated_event("item".to_string(), Some("call_1"), "abc");
    let ThreadEvent::ItemUpdated(ItemUpdatedEvent { item }) = event else {
        panic!("expected updated item");
    };
    let ThreadItemDetails::ToolOutput(details) = item.details else {
        panic!("expected tool output");
    };
    assert_eq!(details.call_id, "item");
    assert_eq!(details.tool_call_id.as_deref(), Some("call_1"));
    assert_eq!(details.output, "abc");
    assert_eq!(details.status, ToolCallStatus::InProgress);
}

#[test]
fn tool_output_payload_preserves_spool_reference() {
    let payload = tool_output_payload_from_value(&json!({
        "spool_path": ".vtcode/context/tool_outputs/run-1.txt",
        "output": "ignored"
    }));

    assert_eq!(payload.aggregated_output, "");
    assert_eq!(payload.spool_path.as_deref(), Some(".vtcode/context/tool_outputs/run-1.txt"));
}

#[test]
fn tool_output_payload_preserves_spooled_metadata_with_reference() {
    let payload = tool_output_payload_from_value(&json!({
        "spool_path": ".vtcode/context/tool_outputs/run-1.txt",
        "preview": "bounded preview",
        "spooled_bytes": 42,
        "stderr_preview": "diagnostic preview",
        "generated_files": ["src/generated.rs"],
        "next_action": "Inspect the generated file."
    }));

    assert_eq!(payload.spool_path.as_deref(), Some(".vtcode/context/tool_outputs/run-1.txt"));
    assert!(payload.aggregated_output.contains("bounded preview"));
    assert!(payload.aggregated_output.contains("\"spooled_bytes\": 42"));
    assert!(payload.aggregated_output.contains("diagnostic preview"));
    assert!(payload.aggregated_output.contains("src/generated.rs"));
    assert!(payload.aggregated_output.contains("Inspect the generated file."));
}

#[test]
fn tool_output_payload_includes_apply_patch_diff_content() {
    let payload = tool_output_payload_from_value(&json!({
        "success": true,
        "applied": ["[1/1] Updated file: README.md (1 chunk)"],
        "modified_files": ["README.md"],
        "diff": [{
            "path": "README.md",
            "content": "diff --git a/README.md b/README.md\n-before\n+after\n",
            "skipped": false
        }]
    }));

    assert!(payload.aggregated_output.contains("diff --git a/README.md b/README.md"));
    assert!(payload.aggregated_output.contains("-before"));
    assert!(payload.aggregated_output.contains("+after"));
}

#[test]
fn tool_output_payload_normalizes_legacy_write_diff_preview() {
    let payload = tool_output_payload_from_value(&json!({
        "success": true,
        "path": "README.md",
        "diff_preview": {
            "content": "diff --git a/README.md b/README.md\n-before\n+after\n",
            "skipped": false
        }
    }));

    assert!(payload.aggregated_output.contains("diff preview for README.md"));
    assert!(payload.aggregated_output.contains("-before"));
    assert!(payload.aggregated_output.contains("+after"));
}

#[test]
fn tool_output_payload_preserves_diff_metadata_without_repeating_preview() {
    let payload = tool_output_payload_from_value(&json!({
        "success": true,
        "applied": ["Updated README.md"],
        "modified_files": ["README.md"],
        "diff": [{
            "path": "README.md",
            "content": "diff --git a/README.md b/README.md\n-before\n+after\n",
            "skipped": false
        }],
        "next_action": "Run the documentation checks."
    }));

    assert!(payload.aggregated_output.contains("Updated README.md"));
    assert!(payload.aggregated_output.contains("README.md"));
    assert!(payload.aggregated_output.contains("Run the documentation checks."));
    assert_eq!(payload.aggregated_output.matches("diff --git a/README.md b/README.md").count(), 1);
}

#[test]
fn tool_output_payload_summarizes_list_results() {
    let payload = tool_output_payload_from_value(&json!({
        "items": [
            {"name": "app.rs", "path": "vtcode-tui/src/app.rs", "type": "file"},
            {"name": "core_tui", "path": "vtcode-tui/src/core_tui", "type": "directory"},
            {"name": "lib.rs", "path": "vtcode-tui/src/lib.rs", "type": "file"}
        ],
        "count": 3,
        "total": 11
    }));

    assert_eq!(payload.spool_path, None);
    assert!(payload.aggregated_output.contains("Listed 11 items"));
    assert!(payload.aggregated_output.contains("2 files, 1 directory"));
    assert!(payload.aggregated_output.contains("vtcode-tui/src/app.rs"));
}

#[test]
fn tool_output_payload_combines_list_summary_with_message() {
    let payload = tool_output_payload_from_value(&json!({
        "items": [
            {"name": "app.rs", "path": "vtcode-tui/src/app.rs", "type": "file"},
            {"name": "core_tui", "path": "vtcode-tui/src/core_tui", "type": "directory"}
        ],
        "count": 2,
        "total": 2,
        "message": "[+3 more items]"
    }));

    assert!(payload.aggregated_output.contains("Listed 2 items"));
    assert!(payload.aggregated_output.contains("[+3 more items]"));
}

#[test]
fn tool_output_payload_summarizes_match_results() {
    let payload = tool_output_payload_from_value(&json!({
        "matches": [
            {"path": "src/main.rs", "line_number": 12},
            {"file": "src/lib.rs", "line_number": 9}
        ],
        "total_match_count": 7
    }));

    assert_eq!(payload.spool_path, None);
    assert!(payload.aggregated_output.contains("Found 7 matches"));
    assert!(payload.aggregated_output.contains("src/main.rs"));
    assert!(payload.aggregated_output.contains("src/lib.rs"));
}

#[test]
fn tool_output_payload_summarizes_nested_match_paths() {
    let payload = tool_output_payload_from_value(&json!({
        "matches": [
            {
                "type": "match",
                "data": {
                    "path": {"text": "vtcode-tui/src/core_tui/runner/mod.rs"},
                    "line_number": 27,
                    "lines": {"text": "runloop\n"}
                }
            }
        ],
        "total_match_count": 1
    }));

    assert!(payload.aggregated_output.contains("Found 1 match"));
    assert!(payload.aggregated_output.contains("vtcode-tui/src/core_tui/runner/mod.rs"));
}

#[test]
fn tool_output_payload_reports_empty_match_set() {
    let payload = tool_output_payload_from_value(&json!({
        "matches": [],
        "path": "crates/codegen/vtcode-core/src"
    }));

    assert_eq!(payload.aggregated_output, "No matches found");
    assert_eq!(payload.spool_path, None);
}

#[test]
fn tool_output_payload_summarizes_code_search_results() {
    // Shape produced by the `code_search` tool (`CodeSearchResult`).
    let payload = tool_output_payload_from_value(&json!({
        "query": "startup timing",
        "filters": {"path": "src", "file_types": null, "result_types": null, "max_results": 10},
        "results": [
            {"result_type": "definition", "path": "src/main.rs", "line": 42},
            {"result_type": "usage", "path": "src/startup/mod.rs", "line": 7},
            {"result_type": "usage", "path": "src/main.rs", "line": 42}
        ],
        "returned": 3
    }));

    assert_eq!(payload.spool_path, None);
    assert!(payload.aggregated_output.contains("Returned 3 results"));
    assert!(payload.aggregated_output.contains("for `startup timing`"));
    assert!(payload.aggregated_output.contains("src/main.rs:42"));
    assert!(payload.aggregated_output.contains("src/startup/mod.rs:7"));
    // Duplicated path:line sampled once only.
    assert_eq!(payload.aggregated_output.matches("src/main.rs:42").count(), 1);
}

#[test]
fn tool_output_payload_reports_empty_code_search_results() {
    let payload = tool_output_payload_from_value(&json!({
        "query": "nonexistent_symbol",
        "results": [],
        "returned": 0
    }));

    assert_eq!(payload.aggregated_output, "No results found for `nonexistent_symbol`");
}

#[test]
fn tool_output_payload_includes_structured_recovery_guidance() {
    let payload = tool_output_payload_from_value(&json!({
        "matches": [],
        "path": "src/agent",
        "hint": "Pattern looks like a code fragment.",
        "next_action": "Retry with a larger parseable pattern."
    }));

    assert!(payload.aggregated_output.contains("No matches found"));
    assert!(payload.aggregated_output.contains("Pattern looks like a code fragment."));
    assert!(payload.aggregated_output.contains("Retry with a larger parseable pattern."));
}

#[test]
fn tool_output_payload_preserves_distinct_stream_aliases_once() {
    let payload = tool_output_payload_from_value(&json!({
        "output": "merged stdout\nmerged stderr",
        "stdout": "merged stdout",
        "stderr": "merged stderr"
    }));

    assert_eq!(payload.aggregated_output, "merged stdout\nmerged stderr");
}

#[test]
fn tool_output_payload_preserves_equal_named_streams_without_merged_output() {
    let payload = tool_output_payload_from_value(&json!({
        "stdout": "same output",
        "stderr": "same output"
    }));

    assert!(payload.aggregated_output.contains("[stdout]\nsame output"));
    assert!(payload.aggregated_output.contains("[stderr]\nsame output"));
    assert_eq!(payload.aggregated_output.matches("same output").count(), 2);
}

#[test]
fn tool_output_payload_preserves_subset_named_streams_without_merged_output() {
    let payload = tool_output_payload_from_value(&json!({
        "stdout": "shared line\nstdout only",
        "stderr": "shared line"
    }));

    assert!(payload.aggregated_output.contains("[stdout]\nshared line\nstdout only"));
    assert!(payload.aggregated_output.contains("[stderr]\nshared line"));
    assert_eq!(payload.aggregated_output.matches("shared line").count(), 2);
}

#[test]
fn tool_output_payload_preserves_overlapping_named_streams_with_merged_output() {
    let payload = tool_output_payload_from_value(&json!({
        "output": "shared line\nmerged only",
        "stdout": "shared line",
        "stderr": "shared line"
    }));

    assert!(payload.aggregated_output.contains("[stdout]\nshared line"));
    assert!(payload.aggregated_output.contains("[stderr]\nshared line"));
    assert!(payload.aggregated_output.contains("shared line\nmerged only"));
}

#[test]
fn tool_output_payload_preserves_structured_metadata_with_streams() {
    let payload = tool_output_payload_from_value(&json!({
        "stdout": "command output",
        "json_result": {"answer": 42},
        "generated_files": {"files": ["src/generated.rs"]},
        "next_action": "Review the generated file."
    }));

    assert!(payload.aggregated_output.contains("command output"));
    assert!(payload.aggregated_output.contains("Structured output:"));
    assert!(payload.aggregated_output.contains("src/generated.rs"));
    assert!(payload.aggregated_output.contains("\"answer\": 42"));
    assert!(payload.aggregated_output.contains("Review the generated file."));
}

#[test]
fn tool_output_payload_preserves_structured_only_result_metadata() {
    let payload = tool_output_payload_from_value(&json!({
        "json_result": {"answer": 42},
        "metadata_flag": false,
        "metadata_count": 0,
    }));

    assert!(payload.aggregated_output.contains("Structured output:"));
    assert!(payload.aggregated_output.contains("\"answer\": 42"));
    assert!(payload.aggregated_output.contains("\"metadata_flag\": false"));
    assert!(payload.aggregated_output.contains("\"metadata_count\": 0"));
}

#[test]
fn tool_output_payload_preserves_structured_error_details() {
    let payload = tool_output_payload_from_value(&json!({
        "error": {
            "message": "command failed",
            "code": "E_COMMAND",
            "retryable": true,
        }
    }));

    assert!(payload.aggregated_output.contains("command failed"));
    assert!(payload.aggregated_output.contains("\"code\": \"E_COMMAND\""));
    assert!(payload.aggregated_output.contains("\"retryable\": true"));
}

#[test]
fn tool_output_payload_preserves_streams_alongside_diff_preview() {
    let payload = tool_output_payload_from_value(&json!({
        "stdout": "apply completed",
        "stderr": "warning: generated file was already present",
        "diff": [{
            "path": "README.md",
            "content": "diff --git a/README.md b/README.md\n-before\n+after\n",
            "skipped": false,
        }]
    }));

    assert!(payload.aggregated_output.contains("diff --git a/README.md b/README.md"));
    assert!(payload.aggregated_output.contains("[stdout]\napply completed"));
    assert!(
        payload
            .aggregated_output
            .contains("[stderr]\nwarning: generated file was already present")
    );
}

#[test]
fn tool_output_payload_preserves_non_object_results() {
    assert_eq!(tool_output_payload_from_value(&json!("  plain result\n")).aggregated_output, "  plain result\n");
    assert_eq!(tool_output_payload_from_value(&json!(42)).aggregated_output, "42");
    assert_eq!(tool_output_payload_from_value(&json!(null)).aggregated_output, "");
}

#[test]
fn tool_invocation_completed_event_embeds_outcome() {
    let event = tool_invocation_completed_event(
        "tool_1".to_string(),
        "exec_command",
        Some(&json!({"cmd": "pwd"})),
        Some("call_1"),
        ToolCallStatus::Failed,
        ToolOutcome::HookDenied,
    );
    let ThreadEvent::ItemCompleted(ItemCompletedEvent { item }) = event else {
        panic!("expected completed item");
    };
    let ThreadItemDetails::ToolInvocation(details) = item.details else {
        panic!("expected tool invocation");
    };
    assert_eq!(details.status, ToolCallStatus::Failed);
    assert_eq!(details.outcome, Some(ToolOutcome::HookDenied));
}

#[test]
fn complete_tool_call_infers_outcome_from_status() {
    let mut emitter = SharedLifecycleEmitter::default();
    let call_id = "call_1".to_string();
    emitter.start_tool_call(&call_id, Some("exec_command".to_string()), None);
    emitter.sync_tool_call_arguments(&call_id, "{\"cmd\":\"pwd\"}", Some("exec_command".to_string()), None);
    emitter.complete_tool_call(&call_id, ToolCallStatus::Completed, None);
    let events = emitter.drain_events();
    // events[0] = tool_started, events[1] = tool_invocation_updated, events[2] = tool_invocation_completed
    let ThreadEvent::ItemCompleted(ItemCompletedEvent { item }) = &events[2] else {
        panic!("expected completed item at index 2, got {:?}", events[0]);
    };
    let ThreadItemDetails::ToolInvocation(details) = &item.details else {
        panic!("expected tool invocation");
    };
    assert_eq!(details.outcome, Some(ToolOutcome::Success));
}

#[test]
fn append_tool_call_delta_throttles_intermediate_updates() {
    let mut emitter = SharedLifecycleEmitter::default();
    let call_id = "call_t".to_string();
    emitter.start_tool_call(&call_id, Some("exec_command".to_string()), None);
    // Clear the ItemStarted event so we only count delta-driven ItemUpdated events.
    let _ = emitter.drain_events();

    // Send 600 one-byte deltas. The first delta always emits (update_events == 0).
    // Subsequent deltas only emit after MIN_TOOL_ARG_UPDATE_BYTES (512) new bytes.
    // Expected ItemUpdated events: at 1 byte (first) and at 513 bytes (threshold met).
    for _ in 0..600 {
        emitter.append_tool_call_delta(&call_id, "x", None, None);
    }
    let events = emitter.drain_events();
    let item_updated_count = events.iter().filter(|e| matches!(e, ThreadEvent::ItemUpdated(_))).count();
    assert_eq!(
        item_updated_count, 2,
        "expected 2 throttled ItemUpdated events for 600 1-byte deltas, got {item_updated_count}"
    );

    // The complete event must still carry the full accumulated arguments.
    emitter.complete_tool_call(&call_id, ToolCallStatus::Completed, None);
    let events = emitter.drain_events();
    let completed = events
        .iter()
        .find_map(|e| {
            if let ThreadEvent::ItemCompleted(ItemCompletedEvent { item }) = e {
                Some(item)
            } else {
                None
            }
        })
        .expect("should have a completed event");
    let ThreadItemDetails::ToolInvocation(details) = &completed.details else {
        panic!("expected tool invocation in completed event");
    };
    assert!(
        details.arguments.as_ref().is_some_and(|a| a.to_string().contains("xxx")),
        "completed event should carry the full accumulated arguments"
    );
}

#[test]
fn append_tool_call_delta_caps_intermediate_update_events() {
    let mut emitter = SharedLifecycleEmitter::default();
    let call_id = "call_c".to_string();
    emitter.start_tool_call(&call_id, Some("exec_command".to_string()), None);
    let _ = emitter.drain_events();

    // Send 5000 one-byte deltas. With MIN_TOOL_ARG_UPDATE_BYTES=512 and
    // MAX_TOOL_ARG_UPDATE_EVENTS=8, emissions stop after 8 updates even
    // though the threshold keeps being met.
    for _ in 0..5000 {
        emitter.append_tool_call_delta(&call_id, "x", None, None);
    }
    let events = emitter.drain_events();
    let item_updated_count = events.iter().filter(|e| matches!(e, ThreadEvent::ItemUpdated(_))).count();
    assert_eq!(
        item_updated_count, MAX_TOOL_ARG_UPDATE_EVENTS,
        "intermediate updates should be capped at MAX_TOOL_ARG_UPDATE_EVENTS, got {item_updated_count}"
    );
}

#[test]
fn flush_open_tool_call_arguments_emits_full_args_for_throttled_small_calls() {
    let mut emitter = SharedLifecycleEmitter::default();
    let call_id = "call_flush".to_string();
    emitter.start_tool_call(&call_id, Some("shell".to_string()), None);
    let _ = emitter.drain_events();

    // Stream a small JSON tool call in two deltas. The first delta emits
    // eagerly (incomplete JSON); the second is below the byte threshold
    // and is throttled out.
    emitter.append_tool_call_delta(&call_id, "{\"cmd\":\"ec", None, None);
    emitter.append_tool_call_delta(&call_id, "ho hi\"}", None, None);
    let intermediate = emitter.drain_events();
    let intermediate_updated = intermediate.iter().filter(|e| matches!(e, ThreadEvent::ItemUpdated(_))).count();
    assert_eq!(intermediate_updated, 1, "first delta emits one eager update");

    // Flush should emit a final update with the full, valid-JSON arguments.
    emitter.flush_open_tool_call_arguments();
    let flushed = emitter.drain_events();
    let updated = flushed
        .iter()
        .filter_map(|e| {
            if let ThreadEvent::ItemUpdated(ItemUpdatedEvent { item }) = e {
                Some(&item.details)
            } else {
                None
            }
        })
        .next_back();
    let ThreadItemDetails::ToolInvocation(details) = updated.expect("flush should emit an update") else {
        panic!("expected tool invocation update");
    };
    assert_eq!(
        details.arguments.as_ref().and_then(|a| a.get("cmd")).and_then(|c| c.as_str()),
        Some("echo hi"),
        "flush should carry the full accumulated arguments"
    );
    // Tool call should still be open (no completed event).
    assert!(flushed.iter().all(|e| !matches!(e, ThreadEvent::ItemCompleted(_))));
}

#[test]
fn flush_open_tool_call_arguments_skips_redundant_flush() {
    let mut emitter = SharedLifecycleEmitter::default();
    let call_id = "call_skip".to_string();
    emitter.start_tool_call(&call_id, Some("shell".to_string()), None);
    let _ = emitter.drain_events();

    // A single large delta (>= MIN_TOOL_ARG_UPDATE_BYTES) emits with the
    // full arguments, so the last_emitted_args_len already equals the
    // accumulated length. A subsequent flush should be a no-op.
    let big = "x".repeat(MIN_TOOL_ARG_UPDATE_BYTES);
    emitter.append_tool_call_delta(&call_id, &big, None, None);
    let _ = emitter.drain_events();

    emitter.flush_open_tool_call_arguments();
    let flushed = emitter.drain_events();
    assert!(flushed.is_empty(), "flush should skip when the last update already carried the full arguments");
}

#[test]
fn tool_output_payload_maps_app_disclosure_to_previews() {
    let payload = tool_output_payload_from_value(&json!({
        "output": "tool ran",
        "uiDisclosure": {
            "ui://demo/app.html": {
                "uri": "ui://demo/app.html",
                "mimeType": "text/html;profile=mcp-app",
                "bytes": 4312,
                "blocks": 2,
                "ui": {
                    "connectDomains": ["https://api.example.com"],
                    "resourceDomains": [],
                    "frameDomains": [],
                    "baseUriDomains": [],
                    "permissions": ["camera"],
                    "domain": null,
                },
            },
        },
    }));

    assert_eq!(payload.app_previews.len(), 1);
    let preview = &payload.app_previews[0];
    assert_eq!(preview.uri, "ui://demo/app.html");
    assert_eq!(preview.mime_type.as_deref(), Some("text/html;profile=mcp-app"));
    assert_eq!(preview.bytes, 4312);
    assert_eq!(preview.blocks, 2);
    let ui = preview.ui.as_ref().expect("ui posture must be mapped");
    assert_eq!(ui.connect_domains, vec!["https://api.example.com".to_string()]);
    assert_eq!(ui.permissions, vec!["camera".to_string()]);
    assert!(ui.resource_domains.is_empty());
}

#[test]
fn tool_output_payload_skips_malformed_disclosure_entries() {
    let payload = tool_output_payload_from_value(&json!({
        "output": "tool ran",
        "uiDisclosure": {
            "ui://demo/good.html": {
                "uri": "ui://demo/good.html",
                "bytes": 128,
                "blocks": 1,
            },
            "ui://demo/bad.html": {"bytes": "not-a-number"},
        },
    }));

    assert_eq!(payload.app_previews.len(), 1);
    assert_eq!(payload.app_previews[0].uri, "ui://demo/good.html");
}

#[test]
fn tool_output_payload_without_disclosure_has_no_previews() {
    let payload = tool_output_payload_from_value(&json!({"output": "plain result"}));
    assert!(payload.app_previews.is_empty());

    let payload = tool_output_payload_from_value(&json!({"output": "x", "uiDisclosure": "not-an-object"}));
    assert!(payload.app_previews.is_empty());
}

#[test]
fn tool_output_payload_maps_suppression_marker_to_refused_uris() {
    let payload = tool_output_payload_from_value(&json!({
        "output": "tool ran",
        "uiSuppressed": ["ui://b/app.html", "ui://a/app.html"],
    }));

    assert_eq!(payload.suppressed_apps, ["ui://b/app.html", "ui://a/app.html"]);
    assert!(payload.app_previews.is_empty(), "suppressed results carry no previews");
    assert!(
        !payload.aggregated_output.contains("uiSuppressed"),
        "marker must not leak into aggregated text: {}",
        payload.aggregated_output
    );
}

#[test]
fn tool_output_payload_rejects_malformed_suppression_markers() {
    let cases = [
        (json!("not-an-array"), Vec::<&str>::new()),
        (json!({"ui://a/app.html": true}), Vec::<&str>::new()),
        (json!(["ui://ok/app.html", 42, Value::Null]), vec!["ui://ok/app.html"]),
        (json!(["u0", "u1", "u2", "u3", "u4", "u5"]), vec!["u0", "u1", "u2", "u3"]),
    ];
    for (marker, expected) in cases {
        let payload = tool_output_payload_from_value(&json!({"output": "x", "uiSuppressed": marker}));
        assert_eq!(payload.suppressed_apps, expected, "marker {marker} must map strictly");
    }
}

#[test]
fn tool_output_payload_excludes_app_keys_from_structured_metadata() {
    let payload = tool_output_payload_from_value(&json!({
        "output": "tool ran",
        "uiDisclosure": {"ui://demo/app.html": {"uri": "ui://demo/app.html", "bytes": 8, "blocks": 1}},
    }));
    assert_eq!(payload.app_previews.len(), 1);
    assert!(
        !payload.aggregated_output.contains("uiDisclosure"),
        "disclosure has first-class previews; raw JSON must not repeat: {}",
        payload.aggregated_output
    );
}
