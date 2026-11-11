use super::*;

#[test]
fn code_search_unavailable_error_retains_bounded_backend_context() {
    let fixture = code_search_fixture();
    let scope = scope::resolve_scope(fixture.workspace.path(), "src/widget.rs").unwrap();
    let Err(error) = aggregate_search_results(SearchAggregationInputs {
        scope,
        languages: vec![],
        result_types: vec![CodeSearchResultType::Text],
        max_results: 5,
        definition_enabled: false,
        text_enabled: true,
        usage_enabled: false,
        usage_backend_needed: false,
        has_unsupported_files: false,
        requested_unsupported_file_types: false,
        literal_outcome: Err(anyhow::anyhow!("backend refused: {}", "é".repeat(1000))),
        declaration_outcome: Ok(None),
        path_outcome: Ok(None),
    }) else {
        panic!("expected an unavailable category");
    };
    let text = error.to_string();
    assert!(text.contains("literal: backend refused:"));
    assert!(text.contains("requested: text"));
    assert!(text.chars().count() < 600);
}
#[test]
fn result_identity_includes_effective_max_results() {
    let one = normalised_identity(&serde_json::json!({
        "query": "Widget",
        "max_results": 1
    }));
    let hundred = normalised_identity(&serde_json::json!({
        "query": "Widget",
        "max_results": 100
    }));
    assert_ne!(one, hundred);
}
#[test]
fn result_identity_normalises_omitted_max_results_to_default() {
    let omitted = normalised_identity(&serde_json::json!({"query": " Widget "}));
    let explicit = normalised_identity(&serde_json::json!({
        "query": "Widget",
        "max_results": DEFAULT_MAX_RESULTS
    }));
    assert_eq!(omitted, explicit);
}
#[test]
fn loop_identity_deliberately_ignores_max_results() {
    let one = normalised_loop_identity(&serde_json::json!({
        "query": "Widget",
        "max_results": 1
    }));
    let hundred = normalised_loop_identity(&serde_json::json!({
        "query": "Widget",
        "max_results": 100
    }));
    assert_eq!(one, hundred);
}
#[test]
fn search_identity_normalises_lexical_path_aliases() {
    let plain = normalised_loop_identity(&serde_json::json!({
        "query": "Widget",
        "path": "README.md",
    }));
    let aliased = normalised_loop_identity(&serde_json::json!({
        "query": "Widget",
        "path": "./docs/../README.md",
    }));

    assert_eq!(plain, aliased);
}
#[test]
fn search_identity_preserves_escaping_parent_components() {
    assert_eq!(normalised_path("./docs/../README.md"), "README.md");
    assert_eq!(normalised_path("../README.md"), "../README.md");

    let in_workspace = normalised_identity(&serde_json::json!({
        "query": "Widget",
        "path": "README.md",
    }));
    let escaping = normalised_identity(&serde_json::json!({
        "query": "Widget",
        "path": "../README.md",
    }));
    assert_ne!(in_workspace, escaping);
}
#[test]
fn code_search_defaults_normalise_to_locked_contract() {
    let request = request(json!({"query": "  Widget  "}))
        .normalise()
        .expect("request should normalise");

    assert_eq!(request.query, "Widget");
    assert_eq!(request.filters.path, ".");
    assert!(request.filters.file_types.is_empty());
    assert_eq!(request.filters.result_types, CodeSearchResultType::ALL);
    assert_eq!(request.filters.max_results, 20);
    assert_eq!(backend_candidate_cap(1), 4);
    assert_eq!(backend_candidate_cap(50), 200);
    assert_eq!(backend_candidate_cap(100), 200);
}
#[test]
fn code_search_filters_normalise_and_deduplicate() {
    let request = request(json!({
        "query": "Widget",
        "path": " src ",
        "file_types": [" rust ", ".rs", ".h", "C"],
        "result_types": ["path", "definition", "path"],
        "max_results": 7
    }))
    .normalise()
    .expect("request should normalise");

    assert_eq!(request.filters.path, "src");
    assert_eq!(request.filters.file_types, ["rust", "c"]);
    assert_eq!(request.filters.result_types, [CodeSearchResultType::Definition, CodeSearchResultType::Path]);
    assert_eq!(request.filters.max_results, 7);
}
#[test]
fn replay_scope_overlap_is_symmetric_and_component_aware() {
    let workspace = TempDir::new().expect("workspace");
    fs::create_dir_all(workspace.path().join("src/module")).expect("source fixture");
    let args = json!({"query": "Widget", "path": "src/module"});
    let absolute_child = workspace.path().join("src/module/widget.rs");
    let canonical_child = identity::canonicalize_existing_prefix(workspace.path()).join("src/module/widget.rs");

    for mutation in [
        PathBuf::from("src/module/widget.rs"),
        absolute_child,
        canonical_child,
        PathBuf::from("src"),
    ] {
        assert!(
            scope_contains_mutated_path(&args, &mutation, workspace.path()),
            "expected overlap for {}",
            mutation.display()
        );
    }
    assert!(!scope_contains_mutated_path(&args, Path::new("src/module_two/widget.rs"), workspace.path(),));
    assert!(!scope_contains_mutated_path(&args, Path::new("tests/widget.rs"), workspace.path(),));
}
#[cfg(unix)]
#[test]
fn replay_scope_overlap_resolves_symlinked_workspace_roots() {
    use std::os::unix::fs::symlink;

    let real_workspace = TempDir::new().expect("real workspace");
    fs::create_dir_all(real_workspace.path().join("src")).expect("source fixture");
    let link_parent = TempDir::new().expect("link parent");
    let linked_workspace = link_parent.path().join("workspace-link");
    symlink(real_workspace.path(), &linked_workspace).expect("workspace symlink");
    let args = json!({"query": "Widget", "path": "src"});

    assert!(scope_contains_mutated_path(&args, &linked_workspace.join("src/widget.rs"), &linked_workspace,));
    assert!(scope_contains_mutated_path(&args, &real_workspace.path().join("src/widget.rs"), &linked_workspace,));
}
#[test]
fn complete_stream_does_not_override_explicitly_incomplete_inventory() {
    let workspace = TempDir::new().expect("workspace");
    let source = workspace.path().join("widget.rs");
    fs::write(&source, "fn Widget() {}\n").expect("source fixture");
    let canonical_root = identity::canonicalize_existing_prefix(workspace.path());
    let canonical_source = identity::canonicalize_existing_prefix(&source);
    let scope = ResolvedSearchScope {
        workspace_root: canonical_root.clone(),
        requested_path: canonical_root,
        requested_is_file: false,
        allowed_files: HashSet::from([canonical_source.clone()]),
    };
    let inventories =
        HashMap::from([(canonical_source, DeclarationInventory { complete: false, exact_name_ranges: Vec::new() })]);

    assert!(!scope::has_complete_supported_inventory(&scope, &[CodeLanguage::Rust], true, &inventories,));
}
#[test]
fn code_search_semantic_validation_rejects_invalid_values() {
    for invalid in [
        json!({"query": "   "}),
        json!({"query": "Widget", "path": " "}),
        json!({"query": "Widget", "file_types": []}),
        json!({"query": "Widget", "file_types": ["unknown-language"]}),
        json!({"query": "Widget", "result_types": []}),
        json!({"query": "Widget", "max_results": 0}),
        json!({"query": "Widget", "max_results": 101}),
    ] {
        let error = request(invalid).normalise().expect_err("invalid value must fail");
        assert!(error.to_string().contains("code_search"));
    }
}
#[test]
fn code_search_typed_boundary_rejects_old_and_unknown_fields() {
    for field in [
        "action",
        "workflow",
        "pattern",
        "kind",
        "config_path",
        "filter",
        "lang",
        "selector",
        "strictness",
        "view",
        "items",
        "type",
        "match",
        "pub_members",
        "follow",
        "debug_query",
        "globs",
        "skip_snapshot_tests",
        "context_lines",
        "severities",
        "no_ignore",
        "threads",
        "format",
        "report_style",
        "before_lines",
        "after_lines",
        "builtin_rules",
    ] {
        let mut payload = json!({"query": "Widget"});
        payload
            .as_object_mut()
            .expect("request object")
            .insert(field.to_string(), json!(true));
        let error =
            serde_json::from_value::<CodeSearchRequest>(payload).expect_err("former field must fail deserialisation");
        assert!(error.to_string().contains("unknown field"), "{field}: {error}");
    }
}
#[test]
fn code_search_response_omits_optional_result_fields() {
    let value = serde_json::to_value(CodeSearchResponse {
        query: CompactStr::from("Widget"),
        filters: CodeSearchFilters {
            path: CompactStr::from("."),
            file_types: Vec::new(),
            result_types: CodeSearchResultType::ALL.to_vec(),
            max_results: 20,
        },
        results: vec![CodeSearchResult {
            result_type: CodeSearchResultType::Path,
            path: CompactStr::from("src/widget.rs"),
            line: None,
            column: None,
            name: None,
            snippet: None,
        }],
        returned: 1,
        truncated: false,
        hints: Vec::new(),
    })
    .expect("response should serialise");

    let result = value["results"][0].as_object().expect("result object");
    assert_eq!(result.len(), 2);
    assert_eq!(result["result_type"], "path");
    assert_eq!(result["path"], "src/widget.rs");
}
#[test]
fn code_search_snippets_use_exact_utf8_safe_byte_cap() {
    let ascii = normalised_snippet(&format!("Widget {}", "x".repeat(300)));
    assert_eq!(ascii.len(), SNIPPET_BYTE_CAP);

    let unicode = normalised_snippet(&format!("Widget {}", "こ".repeat(200)));
    assert!(unicode.len() <= SNIPPET_BYTE_CAP);
    assert!(unicode.len() > SNIPPET_BYTE_CAP - 4);
    assert!(std::str::from_utf8(unicode.as_bytes()).is_ok());
}
