#![allow(missing_docs)]

use anyhow::Result;
use serde_json::json;
use tempfile::TempDir;
use vtcode_core::config::constants::tools;
use vtcode_core::tools::ToolRegistry;

#[tokio::test]
async fn preflight_rejects_removed_unified_search_aliases() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    for alias in [
        tools::UNIFIED_SEARCH,
        tools::GREP_FILE,
        tools::LIST_FILES,
        tools::GET_ERRORS,
        tools::SEARCH_TOOLS,
        tools::GREP,
        "Search text",
        "structural search",
        "repo_browser.list_files",
        "repo_browser.grep_file",
        "fetch",
        "errors",
        "tool_discovery",
    ] {
        let err = registry
            .preflight_validate_call(
                alias,
                &json!({
                    "pattern": "ReasoningStage",
                    "path": ".",
                    "max_results": 10
                }),
            )
            .expect_err("removed unified_search alias should not resolve");
        assert!(
            err.to_string().contains("Unknown tool"),
            "{alias} should be rejected before schema validation: {err}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn separate_web_skill_and_discovery_names_do_not_route_to_unified_search() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    for (name, args) in [
        (tools::WEB_SEARCH, json!({"query": "rust ast-grep"})),
        (tools::WEB_FETCH, json!({"url": "https://example.com"})),
        (tools::FETCH_URL, json!({"url": "https://example.com"})),
        ("web", json!({"url": "https://example.com"})),
        (tools::LIST_SKILLS, json!({})),
        (tools::LOAD_SKILL, json!({"name": "example"})),
        (tools::MCP_SEARCH_TOOLS, json!({"query": "database"})),
    ] {
        match registry.preflight_validate_call(name, &args) {
            Ok(outcome) => assert_ne!(
                outcome.normalized_tool_name,
                tools::UNIFIED_SEARCH,
                "{name} must remain a separate affordance"
            ),
            Err(err) => assert!(
                err.to_string().contains("Unknown tool"),
                "{name} may be deferred, but must not fail through unified_search schema: {err}"
            ),
        }
    }
    Ok(())
}

#[tokio::test]
async fn code_search_rejects_every_former_public_field() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

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
        let mut payload = json!({"query": "ReasoningStage"});
        payload
            .as_object_mut()
            .expect("request object")
            .insert(field.to_string(), json!(true));
        let err = registry
            .preflight_validate_call(tools::CODE_SEARCH, &payload)
            .expect_err("former public field must fail schema validation");
        let text = err.to_string();
        assert!(text.contains("Invalid arguments for tool 'code_search'"));
        assert!(text.contains(field), "{field}: {text}");
    }
    Ok(())
}

#[tokio::test]
async fn code_search_validates_five_property_contract() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let valid = registry.preflight_validate_call(
        tools::CODE_SEARCH,
        &json!({
            "query": "ReasoningStage",
            "path": "src",
            "file_types": ["rust", ".rs"],
            "result_types": ["usage", "definition"],
            "max_results": 5
        }),
    )?;
    assert_eq!(valid.normalized_tool_name, tools::CODE_SEARCH);
    assert!(valid.readonly_classification);

    for invalid in [
        json!({}),
        json!({"query": ""}),
        json!({"query": "   "}),
        json!({"query": 42}),
        json!({"query": "Widget", "path": 42}),
        json!({"query": "Widget", "file_types": []}),
        json!({"query": "Widget", "file_types": "rust"}),
        json!({"query": "Widget", "file_types": ["unknown-language"]}),
        json!({"query": "Widget", "result_types": []}),
        json!({"query": "Widget", "result_types": ["reference"]}),
        json!({"query": "Widget", "result_types": "text"}),
        json!({"query": "Widget", "max_results": 0}),
        json!({"query": "Widget", "max_results": 101}),
        json!({"query": "Widget", "max_results": "20"}),
    ] {
        let error = registry
            .preflight_validate_call(tools::CODE_SEARCH, &invalid)
            .expect_err("invalid request must fail preflight");
        assert!(
            error
                .to_string()
                .contains("Invalid arguments for tool 'code_search'"),
            "{invalid}: {error}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn code_search_registry_executor_returns_normalised_typed_response() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let response = registry
        .execute_tool(
            tools::CODE_SEARCH,
            json!({
                "query": "  Widget  ",
                "file_types": [".rs", "rust"],
                "result_types": ["path", "definition", "path"]
            }),
        )
        .await?;

    let mut top_level_fields = response
        .as_object()
        .expect("response object")
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    top_level_fields.sort_unstable();
    assert_eq!(
        top_level_fields,
        [
            "filters",
            "hints",
            "query",
            "results",
            "returned",
            "truncated"
        ]
    );
    assert_eq!(response["query"], "Widget");
    assert_eq!(response["filters"]["path"], ".");
    assert_eq!(response["filters"]["file_types"], json!(["rust"]));
    assert_eq!(
        response["filters"]["result_types"],
        json!(["definition", "path"])
    );
    assert_eq!(response["filters"]["max_results"], 20);
    assert_eq!(response["results"], json!([]));
    assert_eq!(response["returned"], 0);
    assert_eq!(response["truncated"], false);
    assert_eq!(response["hints"], json!([]));
    Ok(())
}
