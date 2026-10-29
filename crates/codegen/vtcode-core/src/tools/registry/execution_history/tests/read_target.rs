use super::*;

#[test]
fn find_recent_successful_by_read_target_matches_same_path_different_offset() {
    let history = ToolExecutionHistory::new(10);

    // Record 1: read src/lib.rs with offset=0
    history.add_record(ToolExecutionRecord::success(
        tools::UNIFIED_FILE.to_string(),
        tools::UNIFIED_FILE.to_string(),
        false,
        None,
        json!({"action":"read","path":"src/lib.rs","offset":0,"limit":100}),
        json!({"content":"file content"}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    // Record 2: read src/main.rs (different file)
    history.add_record(ToolExecutionRecord::success(
        tools::UNIFIED_FILE.to_string(),
        tools::UNIFIED_FILE.to_string(),
        false,
        None,
        json!({"action":"read","path":"src/main.rs","offset":0,"limit":100}),
        json!({"content":"main content"}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    // Query: same path, different offset — should NOT match (issue #680:
    // a different offset means the model is asking for a different slice
    // of the file, so it needs fresh content, not a cached stub).
    let result = history.find_recent_successful_by_read_target(
        tools::UNIFIED_FILE,
        &json!({"action":"read","path":"src/lib.rs","offset":500,"limit":200}),
        Duration::from_secs(600),
    );
    assert!(result.is_none(), "different offset should not match same path");

    // Query: different path, same pagination — should match record 2
    let result2 = history.find_recent_successful_by_read_target(
        tools::UNIFIED_FILE,
        &json!({"action":"read","path":"src/main.rs","offset":0,"limit":100}),
        Duration::from_secs(600),
    );
    assert!(result2.is_some());
    assert_eq!(result2.unwrap(), json!({"content":"main content"}));

    // Query: non-existent path — should return None
    let result3 = history.find_recent_successful_by_read_target(
        tools::UNIFIED_FILE,
        &json!({"action":"read","path":"src/missing.rs"}),
        Duration::from_secs(600),
    );
    assert!(result3.is_none());

    // Query: write action — should return None (not read-only)
    let result4 = history.find_recent_successful_by_read_target(
        tools::UNIFIED_FILE,
        &json!({"action":"write","path":"src/lib.rs","content":"new"}),
        Duration::from_secs(600),
    );
    assert!(result4.is_none(), "write action should not match read records");
}
#[test]
fn find_recent_successful_by_read_target_extent_matters() {
    let history = ToolExecutionHistory::new(10);

    // Record: read AGENTS.md, offset=0, limit=200
    history.add_record(ToolExecutionRecord::success(
        tools::UNIFIED_FILE.to_string(),
        tools::UNIFIED_FILE.to_string(),
        false,
        None,
        json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200}),
        json!({"output":"full file content line 1\nline2\n..."}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    // Query: same path, same offset, larger limit → should NOT match
    // (issue #680: the model asked for more lines than the cache has)
    let result = history.find_recent_successful_by_read_target(
        tools::UNIFIED_FILE,
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":220}),
        Duration::from_secs(600),
    );
    assert!(result.is_none(), "larger limit should not match same path");

    // Query: same path, same offset, same limit → should match (genuine repeat)
    let result = history.find_recent_successful_by_read_target(
        tools::UNIFIED_FILE,
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200}),
        Duration::from_secs(600),
    );
    assert!(result.is_some(), "same path and same limit should match");

    // Query: same path, same offset, smaller limit → should match (subset)
    let result = history.find_recent_successful_by_read_target(
        tools::UNIFIED_FILE,
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":100}),
        Duration::from_secs(600),
    );
    assert!(result.is_some(), "smaller limit is a subset of cached extent");
}
#[test]
fn find_recent_successful_by_read_target_no_limit_uses_default() {
    let history = ToolExecutionHistory::new(10);

    // Record: read AGENTS.md with no explicit limit or offset (defaults)
    history.add_record(ToolExecutionRecord::success(
        tools::UNIFIED_FILE.to_string(),
        tools::UNIFIED_FILE.to_string(),
        false,
        None,
        json!({"action":"read","path":"AGENTS.md"}),
        json!({"output":"default content"}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    // Query: same path, also no explicit limit/offset → should match (both use defaults)
    let result = history.find_recent_successful_by_read_target(
        tools::UNIFIED_FILE,
        &json!({"action":"read","path":"AGENTS.md"}),
        Duration::from_secs(600),
    );
    assert!(result.is_some(), "both using default offset/limit should match");

    // Query: same path, default offset but explicit limit → should NOT match
    // (one has explicit pagination, other doesn't — can't compare)
    let result = history.find_recent_successful_by_read_target(
        tools::UNIFIED_FILE,
        &json!({"action":"read","path":"AGENTS.md","limit":200}),
        Duration::from_secs(600),
    );
    assert!(result.is_none(), "mixed default/explicit limit should not match");
}
#[test]
fn find_recent_successful_by_read_target_raw_shape_matters() {
    let history = ToolExecutionHistory::new(10);

    // Record: non-raw read can be summarized for the model, so it must not
    // satisfy a later raw=true query that asks for exact content.
    history.add_record(ToolExecutionRecord::success(
        tools::UNIFIED_FILE.to_string(),
        tools::UNIFIED_FILE.to_string(),
        false,
        None,
        json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200}),
        json!({"summary":"summarized guidance","summarized_for_model":true}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    let result = history.find_recent_successful_by_read_target(
        tools::UNIFIED_FILE,
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200,"raw":true}),
        Duration::from_secs(600),
    );
    assert!(result.is_none(), "non-raw summarized read should not satisfy raw=true query");

    // Record: raw=true read can satisfy the same raw=true shape.
    history.add_record(ToolExecutionRecord::success(
        tools::UNIFIED_FILE.to_string(),
        tools::UNIFIED_FILE.to_string(),
        false,
        None,
        json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200,"raw":true}),
        json!({"output":"exact file content"}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    let result = history.find_recent_successful_by_read_target(
        tools::UNIFIED_FILE,
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200,"raw":true}),
        Duration::from_secs(600),
    );
    assert_eq!(result, Some(json!({"output":"exact file content"})));
}
#[test]
fn find_recent_successful_by_read_target_validates_aliases_pagination_and_encoding() {
    let history = ToolExecutionHistory::new(10);
    let cached_args = json!({
        "action": "read",
        "path": "src/lib.rs",
        "offset_lines": 1,
        "page_size_lines": 100,
        "page": 2,
        "per_page": 50,
        "encoding": "utf8"
    });
    let cached_result = json!({"content": "cached"});
    history.add_record(ToolExecutionRecord::success(
        tools::UNIFIED_FILE.to_string(),
        tools::UNIFIED_FILE.to_string(),
        false,
        None,
        cached_args,
        cached_result.clone(),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    assert_eq!(
        history.find_recent_successful_by_read_target(
            tools::UNIFIED_FILE,
            &json!({
                "action": "read",
                "path": "src/lib.rs",
                "offset_lines": 1,
                "page_size_lines": 50,
                "page": 2,
                "per_page": 50,
                "encoding": "utf8"
            }),
            Duration::from_secs(600),
        ),
        Some(cached_result)
    );
    assert!(
        history
            .find_recent_successful_by_read_target(
                tools::UNIFIED_FILE,
                &json!({
                    "action": "read",
                    "path": "src/lib.rs",
                    "offset_lines": 1,
                    "page_size_lines": 50,
                    "page": 3,
                    "per_page": 50,
                    "encoding": "utf8"
                }),
                Duration::from_secs(600),
            )
            .is_none()
    );
    assert!(
        history
            .find_recent_successful_by_read_target(
                tools::UNIFIED_FILE,
                &json!({
                    "action": "read",
                    "path": "src/lib.rs",
                    "offset_lines": 1,
                    "page_size_lines": 50,
                    "page": 2,
                    "per_page": 50,
                    "encoding": "base64"
                }),
                Duration::from_secs(600),
            )
            .is_none()
    );
    assert!(
        history
            .find_recent_successful_by_read_target(
                tools::UNIFIED_FILE,
                &json!({
                    "action": "read",
                    "path": "src/lib.rs",
                    "offset_lines": "invalid",
                    "page_size_lines": 50,
                    "page": 2,
                    "per_page": 50,
                    "encoding": "utf8"
                }),
                Duration::from_secs(600),
            )
            .is_none()
    );
}
