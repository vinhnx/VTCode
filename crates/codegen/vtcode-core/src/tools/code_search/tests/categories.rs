use super::*;

#[test]
fn code_search_file_type_aliases_share_canonical_mapping() {
    let request = request(json!({
        "query": "Widget",
        "file_types": [".h", "c", ".jsx", "javascript", ".mdx", "md", ".proto", "protobuf", "Dockerfile", "docker"]
    }))
    .normalise()
    .expect("aliases normalise");
    assert_eq!(request.filters.file_types, ["c", "javascript", "md", "proto", "dockerfile"]);
    for (path, language) in [
        ("include/widget.h", AstGrepLanguage::C),
        ("web/widget.jsx", AstGrepLanguage::JavaScript),
        ("docs/widget.mdx", AstGrepLanguage::Markdown),
        ("api/widget.proto", AstGrepLanguage::Protobuf),
        ("Dockerfile", AstGrepLanguage::Dockerfile),
    ] {
        assert_eq!(AstGrepLanguage::from_path(Path::new(path)), Some(language));
    }
}
#[cfg(unix)]
#[tokio::test]
async fn code_search_rejects_symlink_escape() {
    use std::os::unix::fs::symlink;

    let fixture = code_search_fixture();
    let outside = TempDir::new().expect("outside directory");
    fs::write(outside.path().join("Widget.rs"), "fn Widget() {}\n").expect("outside source");
    symlink(outside.path().join("Widget.rs"), fixture.workspace.path().join("escape.rs")).expect("escape symlink");
    assert!(
        execute(
            fixture.workspace.path(),
            request(json!({
                "query": "Widget",
                "path": "escape.rs",
                "result_types": ["text"]
            }))
        )
        .await
        .is_err()
    );
}
#[tokio::test]
#[serial]
async fn code_search_contains_definition_failure_by_requested_category() {
    let fixture = code_search_fixture();
    let _override = set_ast_grep_binary_override_for_tests(None);
    let definition_only = execute(
        fixture.workspace.path(),
        request(json!({
            "query": "Widget",
            "path": "src/widget.rs",
            "result_types": ["definition"]
        })),
    )
    .await;
    assert!(definition_only.is_err());

    let usage_only = execute(
        fixture.workspace.path(),
        request(json!({
            "query": "Widget",
            "path": "src/widget.rs",
            "result_types": ["usage"]
        })),
    )
    .await;
    assert!(usage_only.is_err());

    let mixed = execute(
        fixture.workspace.path(),
        request(json!({
            "query": "Widget",
            "path": "src/widget.rs",
            "result_types": ["definition", "text"]
        })),
    )
    .await
    .expect("text keeps mixed request successful");
    assert!(
        mixed
            .results
            .iter()
            .all(|result| result.result_type == CodeSearchResultType::Text)
    );
    assert!(mixed.hints.iter().any(|hint| hint.contains("definition")));
}
#[test]
fn code_search_incomplete_declaration_inventory_preserves_text() {
    let fixture = code_search_fixture();
    let scope = scope::resolve_scope(fixture.workspace.path(), "src/widget.rs").expect("scope");
    let canonical = identity::canonicalize_existing_prefix(&fixture.workspace.path().join("src/widget.rs"));
    let mut inventories = HashMap::new();
    inventories.insert(canonical, DeclarationInventory { complete: false, exact_name_ranges: Vec::new() });
    let mut candidates = Vec::new();
    backends::classify_literal_candidates(
        &scope,
        &[AstGrepLanguage::Rust],
        vec![LiteralSearchCandidate {
            path: fixture.workspace.path().join("src/widget.rs"),
            line: 1,
            column: 4,
            byte_start: 3,
            byte_end: 9,
            matched_text: "Widget".to_string(),
            snippet: "fn Widget() {\n".to_string(),
        }],
        true,
        true,
        false,
        &mut backends::SourceTreeCache::default(),
        &inventories,
        &mut candidates,
    );

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].result.result_type, CodeSearchResultType::Text);
}
#[test]
fn code_search_skips_empty_declaration_file_without_dropping_text() {
    // Invariant guard for the filter-first skip in `aggregate_search_results`:
    // when `usage_backend_needed` is false, a file with no matching
    // declarations is not read/parsed, yet its literal (text) hits must
    // still surface and no definition may be fabricated. The skip is
    // output-neutral by construction (empty declarations imply empty exact
    // name ranges); this pins that invariant against future refactors.
    let fixture = code_search_fixture();
    let scope = scope::resolve_scope(fixture.workspace.path(), "src/widget.rs").expect("scope");
    let canonical = identity::canonicalize_existing_prefix(&fixture.workspace.path().join("src/widget.rs"));
    let source = fs::read_to_string(&canonical).expect("fixture source");
    let call_start = source.find("Widget();").expect("call site");

    let literal_outcome = LiteralSearchOutcome {
        candidates: vec![LiteralSearchCandidate {
            path: fixture.workspace.path().join("src/widget.rs"),
            line: 2,
            column: 5,
            byte_start: call_start,
            byte_end: call_start + "Widget".len(),
            matched_text: "Widget".to_string(),
            snippet: "    Widget();".to_string(),
        }],
        truncated: false,
    };
    let declaration_outcome = DeclarationSearchOutcome {
        files: vec![DeclarationFileRecord {
            path: canonical,
            language: AstGrepLanguage::Rust,
            declarations: Vec::new(),
            complete: true,
        }],
        stream_complete: true,
        truncated: false,
    };

    let aggregation = aggregate_search_results(SearchAggregationInputs {
        scope,
        languages: vec![AstGrepLanguage::Rust],
        query: "Widget".to_string(),
        result_types: vec![CodeSearchResultType::Definition, CodeSearchResultType::Text],
        max_results: 100,
        definition_enabled: true,
        text_enabled: true,
        usage_enabled: false,
        usage_backend_needed: false,
        has_unsupported_files: false,
        requested_unsupported_file_types: false,
        literal_outcome: Ok(Some(literal_outcome)),
        declaration_outcome: Ok(Some(declaration_outcome)),
        path_outcome: Ok(None),
    })
    .expect("aggregation");

    assert!(
        aggregation
            .results
            .iter()
            .any(|result| result.result_type == CodeSearchResultType::Text),
        "empty-declaration file's literal hit must still surface as text"
    );
    assert!(
        !aggregation
            .results
            .iter()
            .any(|result| result.result_type == CodeSearchResultType::Definition),
        "an empty-declaration file cannot yield definitions"
    );
}
