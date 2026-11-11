use super::*;

#[tokio::test]
#[serial]
async fn code_search_columns_count_bytes_for_definitions_and_usages() {
    let workspace = TempDir::new().expect("workspace");
    fs::create_dir_all(workspace.path().join("src")).expect("src directory");
    fs::create_dir_all(workspace.path().join(".git")).expect("git marker");
    // The usage line carries a multi-byte character before the symbol, so a
    // character count and a byte count disagree.
    let source = "fn Widget() {}\nfn main() {\n    let x = \"こ\"; let Widget = 1;\n}\n";
    fs::write(workspace.path().join("src/uni.rs"), source).expect("fixture");

    let response = execute(
        workspace.path(),
        request(json!({
            "query": "Widget",
            "path": "src",
            "file_types": ["rust"],
            "max_results": 100
        })),
    )
    .await
    .expect("search runs");

    let definition = response
        .results
        .iter()
        .find(|result| result.result_type == CodeSearchResultType::Definition)
        .expect("a definition result");
    assert_eq!(definition.line, Some(1));
    assert_eq!(definition.column, Some(4), "the declaration name starts at byte 4");

    let usage = response
        .results
        .iter()
        .find(|result| result.result_type == CodeSearchResultType::Usage)
        .expect("a usage result");
    let line = source.lines().nth(usage.line.expect("line") - 1).expect("line text");
    let offset = line.find("Widget").expect("usage position");
    let byte_column = offset + 1;
    let char_column = line[..offset].chars().count() + 1;
    assert_ne!(byte_column, char_column, "the fixture must tell bytes and characters apart");
    assert_eq!(usage.column, Some(byte_column), "the column counts bytes");
}

#[test]
fn code_search_definitions_require_exact_name_ranges() {
    let fixture = code_search_fixture();
    let scope = scope::resolve_scope(fixture.workspace.path(), "src/widget.rs").expect("scope");
    let canonical = identity::canonicalize_existing_prefix(&fixture.workspace.path().join("src/widget.rs"));
    let source = fs::read_to_string(&canonical).expect("fixture source");
    // The declaration name starts after `fn `, so the exact name range covers
    // only the identifier, not the whole declaration.
    let name_start = source.find("Widget").expect("declaration name");
    let name_end = name_start + "Widget".len();
    let file = DeclarationFileRecord {
        path: PathBuf::from("src/widget.rs"),
        language: CodeLanguage::Rust,
        complete: true,
        declarations: vec![DeclarationRecord {
            name: "Widget".to_string(),
            range: DeclarationRange { byte_start: name_start, byte_end: name_end },
            line: 1,
            column: 4,
        }],
    };
    let mut inventories = HashMap::new();
    let mut candidates = Vec::new();
    backends::process_declaration_file(
        &scope,
        &[CodeLanguage::Rust],
        &file,
        true,
        &mut HashMap::new(),
        &mut inventories,
        &mut candidates,
    );

    assert_eq!(candidates.len(), 1, "the declaration of the queried symbol is emitted");
    assert_eq!(candidates[0].result.line, Some(1));
    assert_eq!(candidates[0].result.column, Some(4));
    let inventory = inventories.get(&canonical).expect("inventory");
    assert_eq!(inventory.exact_name_ranges.len(), 1, "the exact name range is recorded once");

    let mut mixed = candidates;
    backends::classify_literal_candidates(
        &scope,
        &[CodeLanguage::Rust],
        vec![
            LiteralSearchCandidate {
                path: canonical.clone(),
                line: 1,
                column: 4,
                byte_start: name_start,
                byte_end: name_end,
                matched_text: "Widget".to_string(),
                snippet: "fn Widget() {".to_string(),
                word_bounded: true,
            },
            LiteralSearchCandidate {
                path: canonical,
                line: 2,
                column: 5,
                byte_start: source.find("Widget();").expect("body call"),
                byte_end: source.find("Widget();").expect("body call") + 6,
                matched_text: "Widget".to_string(),
                snippet: "Widget();".to_string(),
                word_bounded: true,
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
            .any(|result| { result.result_type == CodeSearchResultType::Usage && result.line == Some(2) }),
        "a word-bounded occurrence outside the declaration is a usage"
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
        &[CodeLanguage::Rust],
        &mut candidates,
    );

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].result.path, "src/WidgetConfig.rs");
}
