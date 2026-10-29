use super::*;

#[test]
fn code_search_definitions_require_exact_name_ranges() {
    let fixture = code_search_fixture();
    let scope = scope::resolve_scope(fixture.workspace.path(), "src/widget.rs").expect("scope");
    let canonical = identity::canonicalize_existing_prefix(&fixture.workspace.path().join("src/widget.rs"));
    let source = fs::read_to_string(&canonical).expect("fixture source");
    let exact_end = source.find("}\n").expect("exact declaration end") + 1;
    let missing_name_start = source.find("Widget();").expect("body call");
    let file = DeclarationFileRecord {
        path: PathBuf::from("src/widget.rs"),
        language: AstGrepLanguage::Rust,
        complete: true,
        declarations: vec![
            DeclarationRecord {
                name: "Widget".to_string(),
                range: DeclarationRange { byte_start: 0, byte_end: exact_end },
            },
            DeclarationRecord {
                name: "Different".to_string(),
                range: DeclarationRange { byte_start: 0, byte_end: exact_end },
            },
            DeclarationRecord {
                name: "Widget".to_string(),
                range: DeclarationRange {
                    byte_start: missing_name_start,
                    byte_end: missing_name_start + "Widget()".len(),
                },
            },
        ],
    };
    let mut inventories = HashMap::new();
    let mut candidates = Vec::new();
    backends::process_declaration_file(
        &scope,
        &[AstGrepLanguage::Rust],
        "Widget",
        &file,
        true,
        &mut HashMap::new(),
        &mut inventories,
        &mut candidates,
    );

    assert_eq!(candidates.len(), 1, "only the exact definition is emitted");
    assert_eq!(candidates[0].result.line, Some(1));
    assert_eq!(candidates[0].result.column, Some(4));
    let inventory = inventories.get(&canonical).expect("inventory");
    assert!(!inventory.complete, "any failed exact range suppresses usage");
    assert_eq!(inventory.exact_name_ranges.len(), 1);

    let mut mixed = candidates;
    backends::classify_literal_candidates(
        &scope,
        &[AstGrepLanguage::Rust],
        vec![
            LiteralSearchCandidate {
                path: canonical.clone(),
                line: 1,
                column: 4,
                byte_start: 3,
                byte_end: 9,
                matched_text: "Widget".to_string(),
                snippet: "fn Widget() {".to_string(),
            },
            LiteralSearchCandidate {
                path: canonical,
                line: 2,
                column: 5,
                byte_start: missing_name_start,
                byte_end: missing_name_start + 6,
                matched_text: "Widget".to_string(),
                snippet: "Widget();".to_string(),
            },
        ],
        true,
        true,
        false,
        &mut backends::SourceTreeCache::default(),
        &inventories,
        &mut mixed,
    );
    let results = ranking::deduplicate_and_order(mixed);
    assert_eq!(
        results
            .iter()
            .filter(|result| result.result_type == CodeSearchResultType::Definition)
            .count(),
        1
    );
    assert!(
        results
            .iter()
            .any(|result| { result.result_type == CodeSearchResultType::Text && result.line == Some(2) })
    );
}
#[test]
fn code_search_scope_inventory_is_limited_to_requested_path() {
    let fixture = code_search_fixture();
    let direct = scope::resolve_scope(fixture.workspace.path(), "src/widget.rs").expect("file scope");
    assert_eq!(direct.allowed_files.len(), 1);
    assert!(direct.allowed_files.contains(&direct.requested_path));

    let subtree = scope::resolve_scope(fixture.workspace.path(), "src").expect("directory scope");
    assert!(
        subtree
            .allowed_files
            .iter()
            .all(|path| path.starts_with(&subtree.requested_path))
    );
    assert!(
        !subtree
            .allowed_files
            .contains(&identity::canonicalize_existing_prefix(&fixture.workspace.path().join("ignored.rs")))
    );
}
#[test]
fn code_search_deduplication_keeps_strongest_location() {
    let candidate = |result_type, backend_ordinal| ranking::RankedCandidate {
        result: CodeSearchResult {
            result_type,
            path: CompactStr::from("src/widget.rs"),
            line: Some(1),
            column: Some(4),
            name: None,
            snippet: Some(CompactStr::from("fn Widget() {}")),
        },
        backend_ordinal,
    };

    let results = ranking::deduplicate_and_order(vec![
        candidate(CodeSearchResultType::Text, 2),
        candidate(CodeSearchResultType::Usage, 1),
        candidate(CodeSearchResultType::Definition, 0),
    ]);

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].result_type, CodeSearchResultType::Definition);
}
#[test]
fn code_search_path_candidates_deduplicate_canonical_aliases_before_merge() {
    let fixture = code_search_fixture();
    let scope = scope::resolve_scope(fixture.workspace.path(), "src").expect("scope");
    let mut candidates = Vec::new();
    ranking::append_path_candidates(
        &scope,
        &scope.requested_path,
        [PathBuf::from("WidgetConfig.rs"), PathBuf::from("./WidgetConfig.rs")],
        &[AstGrepLanguage::Rust],
        &mut candidates,
    );

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].result.path, "src/WidgetConfig.rs");
}
