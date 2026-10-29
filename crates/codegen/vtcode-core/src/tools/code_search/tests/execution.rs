use super::*;

#[cfg(unix)]
#[tokio::test]
#[serial]
async fn code_search_fixture_composes_exact_classifications_deterministically() {
    let fixture = code_search_fixture();
    let _keep_fake_outline_alive = &fixture.fake_outline;
    let _override = set_ast_grep_binary_override_for_tests(Some(fixture.fake_outline_path.clone()));
    let payload = json!({
        "query": "Widget",
        "path": "src",
        "file_types": ["rust"],
        "max_results": 100
    });
    let first = execute(fixture.workspace.path(), request(payload.clone()))
        .await
        .expect("composed search");
    let second = execute(fixture.workspace.path(), request(payload))
        .await
        .expect("repeat composed search");
    assert_eq!(
        serde_json::to_value(&first).expect("first response"),
        serde_json::to_value(&second).expect("second response")
    );
    assert_eq!(first.results[0].result_type, CodeSearchResultType::Definition);
    assert!(
        first
            .results
            .iter()
            .any(|result| result.result_type == CodeSearchResultType::Usage && result.line == Some(2))
    );
    assert!(
        first
            .results
            .iter()
            .any(|result| result.result_type == CodeSearchResultType::Usage && result.line == Some(6))
    );
    assert!(first.results.iter().any(|result| {
        result.result_type == CodeSearchResultType::Text
            && result.snippet.as_deref().is_some_and(|line| line.contains("WidgetExtra"))
    }));
    assert!(
        first
            .results
            .iter()
            .any(|result| { result.result_type == CodeSearchResultType::Text && result.line == Some(9) })
    );
    assert!(
        first
            .results
            .iter()
            .any(|result| { result.result_type == CodeSearchResultType::Text && result.line == Some(11) })
    );
    assert!(
        first
            .results
            .iter()
            .any(|result| { result.result_type == CodeSearchResultType::Path && result.path == "src/WidgetConfig.rs" })
    );
    assert!(first.hints.iter().any(|hint| hint.contains("syntactic")));

    let lower_case_usage = execute(
        fixture.workspace.path(),
        request(json!({
            "query": "widget",
            "path": "src/widget.rs",
            "file_types": ["rust"],
            "result_types": ["usage"],
            "max_results": 100
        })),
    )
    .await
    .expect("lower-case usage search");
    assert!(!lower_case_usage.results.is_empty());
    assert!(
        lower_case_usage
            .results
            .iter()
            .filter(|result| result.result_type == CodeSearchResultType::Usage)
            .all(|result| result.name.as_deref() == Some("Widget"))
    );

    for result_type in ["definition", "usage", "text", "path"] {
        let response = execute(
            fixture.workspace.path(),
            request(json!({
                "query": "Widget",
                "path": "src",
                "file_types": ["rust"],
                "result_types": [result_type],
                "max_results": 100
            })),
        )
        .await
        .expect("result subset");
        assert!(response.results.iter().all(|result| {
            serde_json::to_value(result.result_type).expect("result type").as_str() == Some(result_type)
        }));
    }

    let unfiltered = execute(
        fixture.workspace.path(),
        request(json!({
            "query": "Widget",
            "path": "src",
            "result_types": ["usage", "text"],
            "max_results": 100
        })),
    )
    .await
    .expect("mixed-language usage search");
    assert!(
        unfiltered
            .results
            .iter()
            .any(|result| { result.result_type == CodeSearchResultType::Text && result.path == "src/widget.sh" })
    );
    assert!(unfiltered.hints.iter().any(|hint| hint.contains("file types")));
}
#[tokio::test]
async fn code_search_literal_smart_case_paths_and_policy_are_bounded() {
    let fixture = code_search_fixture();
    let lower = execute(
        fixture.workspace.path(),
        request(json!({
            "query": "widget",
            "path": "src/widget.rs",
            "result_types": ["text"],
            "max_results": 100
        })),
    )
    .await
    .expect("lower-case literal search");
    let mixed = execute(
        fixture.workspace.path(),
        request(json!({
            "query": "Widget",
            "path": "src/widget.rs",
            "result_types": ["text"],
            "max_results": 100
        })),
    )
    .await
    .expect("mixed-case literal search");
    assert!(lower.returned > mixed.returned);

    let punctuation = execute(
        fixture.workspace.path(),
        request(json!({
            "query": "Widget()",
            "path": "src/widget.rs",
            "result_types": ["text"]
        })),
    )
    .await
    .expect("regex punctuation is literal");
    assert!(!punctuation.results.is_empty());

    let truncated = execute(
        fixture.workspace.path(),
        request(json!({
            "query": "Widget",
            "path": "src/widget.rs",
            "result_types": ["text"],
            "max_results": 1
        })),
    )
    .await
    .expect("bounded result");
    assert_eq!(truncated.returned, 1);
    assert!(truncated.truncated);
    assert!(truncated.hints[0].contains("Narrow path"));
    assert!(
        truncated
            .results
            .iter()
            .all(|result| { result.snippet.as_ref().is_none_or(|snippet| snippet.len() <= SNIPPET_BYTE_CAP) })
    );

    let long_snippet = execute(
        fixture.workspace.path(),
        request(json!({
            "query": "Widget",
            "path": "src/long.rs",
            "result_types": ["text"]
        })),
    )
    .await
    .expect("long snippet search");
    let snippet = long_snippet.results[0].snippet.as_ref().expect("source snippet");
    assert!(snippet.len() <= SNIPPET_BYTE_CAP);
    assert!(snippet.len() > 230);
    assert!(std::str::from_utf8(snippet.as_bytes()).is_ok());

    for rejected_path in ["../outside", ".env", "ignored.rs"] {
        assert!(
            execute(
                fixture.workspace.path(),
                request(json!({
                    "query": "Widget",
                    "path": rejected_path,
                    "result_types": ["text"]
                }))
            )
            .await
            .is_err(),
            "{rejected_path} must be rejected"
        );
    }
}
#[tokio::test]
async fn code_search_direct_file_path_search_never_returns_siblings() {
    let fixture = code_search_fixture();
    let response = execute(
        fixture.workspace.path(),
        request(json!({
            "query": "widget",
            "path": "src/widget.rs",
            "result_types": ["path"]
        })),
    )
    .await
    .expect("direct file path search");

    assert_eq!(response.returned, 1);
    assert_eq!(response.results[0].path, "src/widget.rs");
    assert!(response.results.iter().all(|result| result.path == "src/widget.rs"));
}
#[tokio::test]
async fn code_search_leading_dash_text_query_succeeds_on_single_file() {
    // Session session-vtcode-20261015T083154Z_961005-54191: text-only
    // `path=README.md` queries starting with `- ` failed with "all
    // requested categories unavailable" because ripgrep parsed the leading
    // dash as flags. Asymmetric pair: dash-prefixed vs plain query must
    // both succeed on the same single file.
    let workspace = TempDir::new().expect("workspace");
    fs::create_dir_all(workspace.path().join(".git")).expect("git marker");
    fs::write(workspace.path().join("README.md"), "# Title\n\n- [Sessions](#sessions)\n\n## Sessions\n")
        .expect("readme fixture");

    let dashed = execute(
        workspace.path(),
        request(json!({
            "query": "- [Sessions](#sessions)",
            "path": "README.md",
            "file_types": ["md"],
            "result_types": ["text"]
        })),
    )
    .await
    .expect("leading-dash text search must succeed");
    assert_eq!(dashed.returned, 1);
    assert_eq!(dashed.results[0].result_type, CodeSearchResultType::Text);

    let plain = execute(
        workspace.path(),
        request(json!({
            "query": "Sessions",
            "path": "README.md",
            "result_types": ["text"]
        })),
    )
    .await
    .expect("plain text search must succeed");
    assert!(plain.returned >= 1);
}
#[tokio::test]
async fn code_search_bash_usage_is_successfully_unsupported() {
    let fixture = code_search_fixture();
    let response = execute(
        fixture.workspace.path(),
        request(json!({
            "query": "Widget",
            "path": "src/widget.sh",
            "result_types": ["usage"]
        })),
    )
    .await
    .expect("unsupported Bash usage is a successful empty component");
    assert!(response.results.is_empty());
    assert_eq!(response.returned, 0);
    assert!(!response.truncated);
    assert_eq!(response.hints.len(), 1);
    assert!(response.hints[0].contains("file types"));
}
#[tokio::test]
#[serial]
async fn code_search_bash_definition_remains_available() {
    let fixture = code_search_fixture();
    let _fake_outline_dir = &fixture.fake_outline;
    let _override = set_ast_grep_binary_override_for_tests(Some(fixture.fake_outline_path.clone()));
    let response = execute(
        fixture.workspace.path(),
        request(json!({
            "query": "Widget",
            "path": "src/widget.sh",
            "result_types": ["definition"]
        })),
    )
    .await
    .expect("Bash definition search");

    assert_eq!(response.returned, 1);
    assert_eq!(response.results[0].result_type, CodeSearchResultType::Definition);
    assert_eq!(response.results[0].name.as_deref(), Some("Widget"));
}
#[tokio::test]
async fn code_search_usage_only_requires_an_eligible_supported_inventory() {
    let workspace = TempDir::new().expect("workspace");
    fs::create_dir_all(workspace.path().join("empty")).expect("empty directory");
    fs::write(workspace.path().join("notes.txt"), "Widget\n").expect("unsupported fixture");

    for path in ["empty", "notes.txt"] {
        let outcome = execute(
            workspace.path(),
            request(json!({
                "query": "Widget",
                "path": path,
                "result_types": ["usage"]
            })),
        )
        .await;
        assert!(outcome.is_err(), "{path} has no supported inventory");
    }

    let unsupported = execute(
        workspace.path(),
        request(json!({
            "query": "Widget",
            "path": "empty",
            "file_types": ["bash"],
            "result_types": ["usage"]
        })),
    )
    .await
    .expect("explicit unsupported usage language remains successful");
    assert!(unsupported.results.is_empty());
    assert!(unsupported.hints[0].contains("file types"));
}
