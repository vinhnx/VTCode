use super::*;

#[test]
fn dedup_repeated_file_reads_rewrites_only_older_exact_matches() {
    let history = vec![
        assistant_with_tool_call(read_file_tool_call("call-1", "src/lib.rs")),
        Message::tool_response_with_origin(
            "call-1".to_string(),
            json!({
                "file_path": "src/lib.rs",
                "start_line": 1,
                "end_line": 40,
                "result": "older contents"
            })
            .to_string(),
            tool_names::READ_FILE.to_string(),
        ),
        assistant_with_tool_call(file_operation_read_tool_call("call-2", "src/lib.rs")),
        Message::tool_response(
            "call-2".to_string(),
            json!({
                "path": "src/lib.rs",
                "start_line": 1,
                "end_line": 40,
                "result": "newer contents"
            })
            .to_string(),
        ),
    ];

    let deduped = dedup_repeated_file_reads_for_local_compaction(&history);

    let older_payload: serde_json::Value =
        serde_json::from_str(deduped[1].content.as_text().as_ref()).expect("json payload");
    assert_eq!(older_payload.get("deduped_read").and_then(serde_json::Value::as_bool), Some(true));
    assert_eq!(older_payload.get("note").and_then(serde_json::Value::as_str), Some(DEDUPED_FILE_READ_NOTE));
    assert_eq!(older_payload.get("file_path").and_then(serde_json::Value::as_str), Some("src/lib.rs"));
    assert!(deduped[3].content.as_text().contains("newer contents"));
    assert!(!deduped[3].content.as_text().contains("deduped_read"));
}

#[test]
fn dedup_repeated_file_reads_keeps_different_slices_and_chunked_reads() {
    let different_slice_history = vec![
        assistant_with_tool_call(read_file_tool_call("call-1", "src/lib.rs")),
        Message::tool_response_with_origin(
            "call-1".to_string(),
            json!({
                "file_path": "src/lib.rs",
                "start_line": 1,
                "end_line": 20,
                "result": "slice one"
            })
            .to_string(),
            tool_names::READ_FILE.to_string(),
        ),
        assistant_with_tool_call(read_file_tool_call("call-2", "src/lib.rs")),
        Message::tool_response_with_origin(
            "call-2".to_string(),
            json!({
                "file_path": "src/lib.rs",
                "start_line": 21,
                "end_line": 40,
                "result": "slice two"
            })
            .to_string(),
            tool_names::READ_FILE.to_string(),
        ),
    ];
    let chunked_history = vec![
        assistant_with_tool_call(read_file_tool_call("call-3", "src/lib.rs")),
        Message::tool_response_with_origin(
            "call-3".to_string(),
            json!({
                "file_path": "src/lib.rs",
                "start_line": 1,
                "end_line": 40,
                "result": "first chunk",
                "spool_chunked": true,
                "has_more": true
            })
            .to_string(),
            tool_names::READ_FILE.to_string(),
        ),
        assistant_with_tool_call(read_file_tool_call("call-4", "src/lib.rs")),
        Message::tool_response_with_origin(
            "call-4".to_string(),
            json!({
                "file_path": "src/lib.rs",
                "start_line": 1,
                "end_line": 40,
                "result": "second chunk",
                "spool_chunked": true,
                "has_more": false
            })
            .to_string(),
            tool_names::READ_FILE.to_string(),
        ),
    ];

    assert_eq!(dedup_repeated_file_reads_for_local_compaction(&different_slice_history), different_slice_history);
    assert_eq!(dedup_repeated_file_reads_for_local_compaction(&chunked_history), chunked_history);
}
