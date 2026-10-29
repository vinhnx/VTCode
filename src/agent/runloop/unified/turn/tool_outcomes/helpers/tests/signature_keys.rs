use super::*;

// --- read_normalized_signature_key tests ---

#[test]
fn read_normalized_signature_key_normalizes_file_operation_read_offset() {
    let args_a = json!({"action": "read", "path": "src/lib.rs", "offset": 0, "limit": 100});
    let args_b = json!({"action": "read", "path": "src/lib.rs", "offset": 50, "limit": 200});
    let key_a = read_normalized_signature_key("file_operation", &args_a);
    let key_b = read_normalized_signature_key("file_operation", &args_b);
    assert_eq!(key_a, key_b, "same file read with different offset/limit should produce the same normalized key");
}
#[test]
fn read_normalized_signature_key_preserves_encoding() {
    let utf8 = json!({"action": "read", "path": "src/lib.rs", "encoding": "utf8"});
    let base64 = json!({"action": "read", "path": "src/lib.rs", "encoding": "base64"});

    assert_ne!(
        read_normalized_signature_key("file_operation", &utf8),
        read_normalized_signature_key("file_operation", &base64),
        "different encodings produce different tool output and must not reuse one another"
    );
}
#[test]
fn read_normalized_signature_key_differentiates_different_paths() {
    let args_a = json!({"action": "read", "path": "src/lib.rs"});
    let args_b = json!({"action": "read", "path": "src/main.rs"});
    let key_a = read_normalized_signature_key("file_operation", &args_a);
    let key_b = read_normalized_signature_key("file_operation", &args_b);
    assert_ne!(key_a, key_b, "different paths must produce different keys");
}
#[test]
fn read_normalized_signature_key_includes_code_search_limit_and_normalises_filter_order() {
    let args_a = json!({
        "query": "Widget",
        "path": "src",
        "file_types": ["rust", "typescript"],
        "result_types": ["text", "definition"],
        "max_results": 10
    });
    let args_b = json!({
        "query": "Widget",
        "path": "src",
        "file_types": ["typescript", "rs"],
        "result_types": ["definition", "text"],
        "max_results": 100
    });
    let key_a = read_normalized_signature_key(tools::CODE_SEARCH, &args_a);
    let key_b = read_normalized_signature_key(tools::CODE_SEARCH, &args_b);
    assert_ne!(key_a, key_b, "different effective limits must not share one code-search replay identity");

    let args_default = json!({
        "query": " Widget ",
        "path": "src",
        "file_types": ["rs", "typescript"],
        "result_types": ["definition", "text"]
    });
    let args_explicit_default = json!({
        "query": "Widget",
        "path": "src",
        "file_types": ["typescript", "rust"],
        "result_types": ["text", "definition"],
        "max_results": 20
    });
    assert_eq!(
        read_normalized_signature_key(tools::CODE_SEARCH, &args_default),
        read_normalized_signature_key(tools::CODE_SEARCH, &args_explicit_default),
        "omitted and explicit default limits must share replay identity"
    );
}
#[test]
fn read_normalized_signature_key_preserves_mutation_for_write() {
    let args_a = json!({"path": "src/lib.rs", "content": "old"});
    let args_b = json!({"path": "src/lib.rs", "content": "new"});
    let key_a = read_normalized_signature_key("file_operation", &args_a);
    let key_b = read_normalized_signature_key("file_operation", &args_b);
    assert_ne!(key_a, key_b, "mutating writes must NOT be normalized away");
}
#[test]
fn find_duplicate_in_history_matches_normalized_read() {
    use vtcode_core::llm::provider as uni;

    // find_duplicate_in_history uses read_normalized_signature_key, which
    // strips offset/limit for file reads. A later unrelated Assistant batch
    // must not obscure the earlier matching call and result pair.

    // Verify normalization: same file + different offset/limit → same key
    let key_a = read_normalized_signature_key(
        tools::UNIFIED_FILE,
        &json!({"action":"read","path":"src/lib.rs","offset":0,"limit":100}),
    );
    let key_b = read_normalized_signature_key(
        tools::UNIFIED_FILE,
        &json!({"action":"read","path":"src/lib.rs","offset":50,"limit":500}),
    );
    assert_eq!(key_a, key_b, "same file read with different offset/limit should normalize to the same key");

    // Verify: different file → different key
    let key_c = read_normalized_signature_key(
        tools::UNIFIED_FILE,
        &json!({"action":"read","path":"src/main.rs","offset":0,"limit":100}),
    );
    assert_ne!(key_a, key_c, "different files must produce different normalized keys");

    // Verify: code-search result limits remain distinct while filter ordering normalises away.
    let s_key_a = read_normalized_signature_key(
        tools::CODE_SEARCH,
        &json!({"query":"Widget","path":"src","file_types":["rust","typescript"],"result_types":["text","definition"],"max_results":10}),
    );
    let s_key_b = read_normalized_signature_key(
        tools::CODE_SEARCH,
        &json!({"query":"Widget","path":"src","file_types":["typescript","rs"],"result_types":["definition","text"],"max_results":100}),
    );
    assert_ne!(s_key_a, s_key_b, "different effective limits must not share one code-search replay identity");

    // Verify: write NOT normalized
    let w_key_a = read_normalized_signature_key(
        tools::UNIFIED_FILE,
        &json!({"action":"write","path":"src/lib.rs","content":"old"}),
    );
    let w_key_b = read_normalized_signature_key(
        tools::UNIFIED_FILE,
        &json!({"action":"write","path":"src/lib.rs","content":"new"}),
    );
    assert_ne!(w_key_a, w_key_b, "writes must not be normalized away");

    // Verify: find_duplicate_in_history still works for EXACT match
    let mut history: Vec<uni::Message> = Vec::new();
    history.push(uni::Message::assistant_with_tools(
        "read".into(),
        vec![uni::ToolCall::function(
            "tc_exact".into(),
            tools::UNIFIED_FILE.into(),
            serde_json::to_string(&json!({"action":"read","path":"src/lib.rs","offset":0,"limit":100})).unwrap(),
        )],
    ));
    history.push(uni::Message {
        role: uni::MessageRole::Tool,
        content: uni::MessageContent::text("exact content".into()),
        tool_call_id: Some("tc_exact".into()),
        ..Default::default()
    });
    // Second pair (different file) so the scan finds A₀'s Tool after A₁:
    history.push(uni::Message::assistant_with_tools(
        "read other".into(),
        vec![uni::ToolCall::function(
            "tc_other".into(),
            tools::UNIFIED_FILE.into(),
            serde_json::to_string(&json!({"action":"read","path":"src/main.rs"})).unwrap(),
        )],
    ));
    history.push(uni::Message {
        role: uni::MessageRole::Tool,
        content: uni::MessageContent::text("other content".into()),
        tool_call_id: Some("tc_other".into()),
        ..Default::default()
    });

    let result = find_duplicate_in_history(
        &history,
        tools::UNIFIED_FILE,
        &json!({"action":"read","path":"src/lib.rs","offset":0,"limit":50}),
        Path::new("."),
    );
    assert_eq!(result.as_deref(), Some("exact content"));
}
#[test]
fn find_duplicate_in_history_respects_normalised_code_search_limit() {
    let original_args = json!({
        "query": "Widget",
        "path": "src",
        "file_types": ["rust", "typescript"],
        "result_types": ["text", "definition"],
        "max_results": 10
    });
    let history = vec![
        uni::Message::assistant_with_tools(
            "search".into(),
            vec![uni::ToolCall::function(
                "tc_search".into(),
                tools::CODE_SEARCH.into(),
                serde_json::to_string(&original_args).unwrap(),
            )],
        ),
        uni::Message {
            role: uni::MessageRole::Tool,
            content: uni::MessageContent::text("{\"results\":[]}".into()),
            tool_call_id: Some("tc_search".into()),
            ..Default::default()
        },
    ];

    let different_limit = find_duplicate_in_history(
        &history,
        tools::CODE_SEARCH,
        &json!({
            "query": "Widget",
            "path": "src",
            "file_types": ["typescript", "rs"],
            "result_types": ["definition", "text"],
            "max_results": 100
        }),
        Path::new("."),
    );

    assert_eq!(different_limit, None);

    let equivalent_default_history = vec![
        uni::Message::assistant_with_tools(
            "search".into(),
            vec![uni::ToolCall::function(
                "tc_default".into(),
                tools::CODE_SEARCH.into(),
                serde_json::to_string(&json!({
                    "query": "Widget",
                    "path": "src",
                    "max_results": 20
                }))
                .unwrap(),
            )],
        ),
        uni::Message {
            role: uni::MessageRole::Tool,
            content: uni::MessageContent::text("{\"results\":[1]}".into()),
            tool_call_id: Some("tc_default".into()),
            ..Default::default()
        },
    ];
    let reused = find_duplicate_in_history(
        &equivalent_default_history,
        tools::CODE_SEARCH,
        &json!({"query": " Widget ", "path": "src"}),
        Path::new("."),
    );
    assert_eq!(reused.as_deref(), Some("{\"results\":[1]}"));
}
