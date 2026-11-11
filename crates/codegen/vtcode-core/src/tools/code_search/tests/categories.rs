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
        ("include/widget.h", CodeLanguage::C),
        ("web/widget.jsx", CodeLanguage::JavaScript),
        ("docs/widget.mdx", CodeLanguage::Markdown),
        ("api/widget.proto", CodeLanguage::Protobuf),
        ("Dockerfile", CodeLanguage::Dockerfile),
    ] {
        assert_eq!(CodeLanguage::from_path(Path::new(path)), Some(language));
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
async fn code_search_reports_definitions_and_usages_for_supported_languages() {
    let fixture = code_search_fixture();

    let definitions = execute(
        fixture.workspace.path(),
        request(json!({
            "query": "Widget",
            "path": "src/widget.rs",
            "result_types": ["definition"]
        })),
    )
    .await
    .expect("definition search succeeds");
    assert!(
        definitions
            .results
            .iter()
            .any(|result| result.result_type == CodeSearchResultType::Definition),
        "the Rust function declaration is reported as a definition"
    );

    let usages = execute(
        fixture.workspace.path(),
        request(json!({
            "query": "Widget",
            "path": "src/widget.rs",
            "result_types": ["usage"]
        })),
    )
    .await
    .expect("usage search succeeds");
    assert!(
        usages
            .results
            .iter()
            .any(|result| result.result_type == CodeSearchResultType::Usage),
        "the call site is reported as a usage"
    );
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
        &[CodeLanguage::Rust],
        vec![LiteralSearchCandidate {
            path: fixture.workspace.path().join("src/widget.rs"),
            line: 1,
            column: 4,
            byte_start: 3,
            byte_end: 9,
            matched_text: "Widget".to_string(),
            snippet: "fn Widget() {\n".to_string(),
            word_bounded: true,
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
            word_bounded: true,
        }],
        truncated: false,
    };
    let declaration_outcome = DeclarationSearchOutcome {
        files: vec![DeclarationFileRecord {
            path: canonical,
            language: CodeLanguage::Rust,
            declarations: Vec::new(),
            complete: true,
        }],
        stream_complete: true,
        truncated: false,
    };

    let aggregation = aggregate_search_results(SearchAggregationInputs {
        scope,
        languages: vec![CodeLanguage::Rust],
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
