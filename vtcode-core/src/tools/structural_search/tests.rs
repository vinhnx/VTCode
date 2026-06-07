use super::{
    AstGrepByteOffset, AstGrepLabel, AstGrepMatch, AstGrepMetaVar, AstGrepMetaVariables,
    AstGrepPoint, AstGrepRange, AstGrepRewriteMatch, AstGrepScanFinding, StructuralSearchRequest,
    StructuralWorkflow, build_query_result, execute_structural_search, extract_custom_languages,
    extract_language_globs, extract_language_injections, format_ast_grep_failure, normalize_match,
    normalize_rewrite_match, normalize_scan_finding, parse_compact_matches,
    preflight_parseable_pattern, sanitize_pattern_for_tree_sitter,
};
use crate::tools::ast_grep_binary::AST_GREP_INSTALL_COMMAND;
use crate::tools::editing::patch::set_ast_grep_binary_override_for_tests;
use serde_json::json;
use serial_test::serial;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;

fn request() -> StructuralSearchRequest {
    StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "fn $NAME() {}",
        "path": "src",
        "max_results": 2
    }))
    .expect("valid request")
}

fn write_fake_sg(script_body: &str) -> (TempDir, PathBuf) {
    let script_dir = TempDir::new().expect("script tempdir");
    let script_path = script_dir.path().join("sg");
    fs::write(&script_path, script_body).expect("write fake sg");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&script_path).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script_path, perms).expect("chmod");
    }
    (script_dir, script_path)
}

#[test]
fn normalize_match_emits_vtcode_shape() {
    let match_value = normalize_match(AstGrepMatch {
        file: "src/lib.rs".to_string(),
        text: "fn alpha() {}".to_string(),
        lines: Some("12: fn alpha() {}".to_string()),
        language: Some("Rust".to_string()),
        range: AstGrepRange {
            start: AstGrepPoint {
                line: 12,
                column: 0,
            },
            end: AstGrepPoint {
                line: 12,
                column: 13,
            },
            byte_offset: None,
        },
        meta_variables: None,
    });

    assert_eq!(match_value["file"], "src/lib.rs");
    assert_eq!(match_value["line_number"], 12);
    assert_eq!(match_value["text"], "fn alpha() {}");
    assert_eq!(match_value["lines"], "12: fn alpha() {}");
    assert_eq!(match_value["language"], "Rust");
    assert_eq!(match_value["range"]["start"]["column"], 0);
    assert_eq!(match_value["range"]["end"]["column"], 13);
}

#[test]
fn build_query_result_truncates_matches() {
    let result = build_query_result(
        &request(),
        "src",
        vec![
            AstGrepMatch {
                file: "src/lib.rs".to_string(),
                text: "fn alpha() {}".to_string(),
                lines: None,
                language: Some("Rust".to_string()),
                range: AstGrepRange {
                    start: AstGrepPoint {
                        line: 10,
                        column: 0,
                    },
                    end: AstGrepPoint {
                        line: 10,
                        column: 13,
                    },
                    byte_offset: None,
                },
                meta_variables: None,
            },
            AstGrepMatch {
                file: "src/lib.rs".to_string(),
                text: "fn beta() {}".to_string(),
                lines: None,
                language: Some("Rust".to_string()),
                range: AstGrepRange {
                    start: AstGrepPoint {
                        line: 20,
                        column: 0,
                    },
                    end: AstGrepPoint {
                        line: 20,
                        column: 12,
                    },
                    byte_offset: None,
                },
                meta_variables: None,
            },
            AstGrepMatch {
                file: "src/lib.rs".to_string(),
                text: "fn gamma() {}".to_string(),
                lines: None,
                language: Some("Rust".to_string()),
                range: AstGrepRange {
                    start: AstGrepPoint {
                        line: 30,
                        column: 0,
                    },
                    end: AstGrepPoint {
                        line: 30,
                        column: 13,
                    },
                    byte_offset: None,
                },
                meta_variables: None,
            },
        ],
    );

    assert_eq!(result["backend"], "ast-grep");
    assert_eq!(result["pattern"], "fn $NAME() {}");
    assert_eq!(result["path"], "src");
    assert_eq!(result["matches"].as_array().expect("matches").len(), 2);
    assert_eq!(result["truncated"], true);
}

#[test]
fn structural_request_defaults_workflow_to_query() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "fn $NAME() {}"
    }))
    .expect("valid request");

    assert_eq!(request.workflow, StructuralWorkflow::Query);
}

#[test]
fn structural_request_requires_pattern_or_kind_for_query() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "   "
    }))
    .expect_err("pattern or kind required");

    assert!(
        err.to_string()
            .contains("requires a non-empty `pattern` or `kind`")
    );
}

#[test]
fn structural_request_accepts_kind_without_pattern() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "kind": "function_item",
        "lang": "rust"
    }))
    .expect("kind-only request should be valid");

    assert_eq!(request.workflow, StructuralWorkflow::Query);
    assert_eq!(request.kind(), Some("function_item"));
    assert!(request.pattern().is_none());
}

#[test]
fn structural_request_accepts_kind_with_pattern() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "fn $NAME() {}",
        "kind": "function_item",
        "lang": "rust"
    }))
    .expect("kind+pattern request should be valid");

    assert_eq!(request.kind(), Some("function_item"));
    assert_eq!(request.pattern(), Some("fn $NAME() {}"));
}

#[test]
fn structural_request_accepts_esquery_compound_kind() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "kind": "call_expression > identifier",
        "lang": "javascript"
    }))
    .expect("ESQuery compound kind should be valid");

    assert_eq!(request.kind(), Some("call_expression > identifier"));
}

#[test]
fn structural_request_rejects_kind_for_scan() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "scan",
        "kind": "function_item"
    }))
    .expect_err("scan rejects kind");

    assert!(err.to_string().contains("workflow='scan'"));
    assert!(err.to_string().contains("does not accept `kind`"));
}

#[test]
fn structural_request_rejects_kind_for_test() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "test",
        "kind": "function_item"
    }))
    .expect_err("test rejects kind");

    assert!(err.to_string().contains("workflow='test'"));
    assert!(err.to_string().contains("does not accept `kind`"));
}

#[test]
fn structural_request_allows_scan_without_pattern() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "scan"
    }))
    .expect("scan should not require a pattern");

    assert_eq!(request.workflow, StructuralWorkflow::Scan);
    assert_eq!(request.requested_path(), ".");
    assert_eq!(request.requested_config_path(), "sgconfig.yml");
}

#[test]
fn structural_request_allows_test_without_pattern() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "test"
    }))
    .expect("test should not require a pattern");

    assert_eq!(request.workflow, StructuralWorkflow::Test);
    assert_eq!(request.requested_config_path(), "sgconfig.yml");
}

#[test]
fn structural_request_requires_lang_for_debug_query() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "fn $NAME() {}",
        "debug_query": "ast"
    }))
    .expect_err("lang required");

    assert!(err.to_string().contains(
        "Inference only works for unambiguous file paths or single-language positive globs"
    ));
}

#[test]
fn structural_request_canonicalizes_known_lang_alias() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "fn $NAME() {}",
        "lang": "ts"
    }))
    .expect("valid request");

    assert_eq!(request.lang.as_deref(), Some("typescript"));
}

#[test]
fn structural_request_canonicalizes_additional_ast_grep_aliases() {
    let go_request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "fn $NAME() {}",
        "lang": "golang"
    }))
    .expect("valid request");
    assert_eq!(go_request.lang.as_deref(), Some("go"));

    let js_request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "foo(<Bar />)",
        "lang": "jsx"
    }))
    .expect("valid request");
    assert_eq!(js_request.lang.as_deref(), Some("javascript"));
}

#[test]
fn structural_request_infers_lang_from_unambiguous_path() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "fn $NAME() {}",
        "path": "src/lib.rs",
        "debug_query": "ast"
    }))
    .expect("path inference should satisfy debug query");

    assert_eq!(request.lang.as_deref(), Some("rust"));
}

#[test]
fn structural_request_infers_lang_from_additional_supported_extensions() {
    let js_request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "export default $VALUE",
        "path": "web/app.mjs",
        "debug_query": "ast"
    }))
    .expect("path inference should satisfy debug query");
    assert_eq!(js_request.lang.as_deref(), Some("javascript"));

    let ts_request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "export const $VALUE = 1",
        "path": "web/app.cts",
        "debug_query": "ast"
    }))
    .expect("path inference should satisfy debug query");
    assert_eq!(ts_request.lang.as_deref(), Some("typescript"));
}

#[test]
fn structural_request_infers_lang_from_unambiguous_globs() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "fn $NAME() {}",
        "globs": ["**/*.rs", "!target/**"],
        "debug_query": "ast"
    }))
    .expect("glob inference should satisfy debug query");

    assert_eq!(request.lang.as_deref(), Some("rust"));
}

#[test]
fn structural_request_accepts_rewrite_field_for_rewrite_workflow() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "rewrite",
        "pattern": "fn $NAME() {}",
        "rewrite": "fn renamed() {}"
    }))
    .expect("rewrite field accepted for rewrite workflow");

    assert_eq!(request.workflow, StructuralWorkflow::Rewrite);
    assert_eq!(request.rewrite_text(), Some("fn renamed() {}"));
}

#[test]
fn structural_request_rewrite_workflow_requires_pattern_and_rewrite() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "rewrite",
        "rewrite": "fn renamed() {}"
    }))
    .expect_err("rewrite workflow requires pattern");

    assert!(err.to_string().contains("requires a non-empty `pattern`"));

    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "rewrite",
        "pattern": "fn $NAME() {}"
    }))
    .expect_err("rewrite workflow requires rewrite");

    assert!(err.to_string().contains("requires a non-empty `rewrite`"));
}

#[test]
fn structural_request_rejects_raw_run_only_flags() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "fn $NAME() {}",
        "stdin": true
    }))
    .expect_err("raw run-only flags should be rejected");

    assert!(err.to_string().contains("remove `stdin`"));
}

#[test]
fn structural_request_rejects_hyphenated_raw_run_only_flags() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "fn $NAME() {}",
        "no-ignore": "hidden"
    }))
    .expect_err("hyphenated raw run-only flags should be rejected");

    assert!(err.to_string().contains("remove `no_ignore`"));
}

#[test]
fn structural_request_rejects_scan_output_format_flags() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "scan",
        "format": "sarif"
    }))
    .expect_err("scan-only output flags should be rejected");

    assert!(err.to_string().contains("remove `format`"));
}

#[test]
fn structural_request_rejects_scan_severity_override_flags() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "scan",
        "error": true
    }))
    .expect_err("scan severity overrides should be rejected");

    assert!(err.to_string().contains("remove `error`"));
}

#[test]
fn structural_request_rejects_test_only_snapshot_flags() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "test",
        "snapshot_dir": "__snapshots__"
    }))
    .expect_err("test-only snapshot flags should be rejected");

    assert!(err.to_string().contains("remove `snapshot_dir`"));
}

#[test]
fn structural_request_rejects_test_only_include_off_flag() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "test",
        "include_off": true
    }))
    .expect_err("test include-off flag should be rejected");

    assert!(err.to_string().contains("remove `include_off`"));
}

#[test]
fn structural_request_rejects_new_command_flags() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "fn $NAME() {}",
        "base_dir": "."
    }))
    .expect_err("new-command flags should be rejected");

    assert!(err.to_string().contains("remove `base_dir`"));
}

#[test]
fn structural_request_rejects_query_only_fields_for_scan() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "scan",
        "lang": "rust"
    }))
    .expect_err("scan rejects query-only fields");

    assert!(err.to_string().contains("workflow='scan'"));
    assert!(err.to_string().contains("does not accept `lang`"));
}

#[test]
fn structural_request_rejects_scan_only_fields_for_query() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "fn $NAME() {}",
        "config_path": "sgconfig.yml"
    }))
    .expect_err("query rejects config path");

    assert!(err.to_string().contains("workflow='query'"));
    assert!(err.to_string().contains("does not accept `config_path`"));
}

#[test]
fn structural_request_rejects_query_only_fields_for_test() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "test",
        "selector": "function_item"
    }))
    .expect_err("test rejects query-only fields");

    assert!(err.to_string().contains("workflow='test'"));
    assert!(err.to_string().contains("does not accept `selector`"));
}

#[test]
fn structural_request_rejects_scan_only_fields_for_test() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "test",
        "globs": ["**/*.rs"]
    }))
    .expect_err("test rejects globs");

    assert!(err.to_string().contains("workflow='test'"));
    assert!(err.to_string().contains("does not accept `globs`"));
}

#[test]
fn sanitize_pattern_for_tree_sitter_rewrites_ast_grep_metavariables() {
    let (sanitized, contains_metavariables) =
        sanitize_pattern_for_tree_sitter("fn $NAME($$ARGS) { $BODY }");

    assert!(contains_metavariables);
    assert_eq!(sanitized, "fn placeholder(placeholders) { placeholder }");
}

#[test]
fn structural_pattern_preflight_accepts_supported_metavariable_patterns() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "fn $NAME($$ARGS) {}",
        "lang": "rust"
    }))
    .expect("valid request");

    assert!(
        preflight_parseable_pattern(&request)
            .expect("metavariable pattern should preflight")
            .is_none()
    );
}

#[test]
fn structural_pattern_preflight_guides_result_type_fragments() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "Result<$T>",
        "lang": "rust"
    }))
    .expect("valid request");

    let hint = preflight_parseable_pattern(&request)
        .expect("fragment hint should be returned")
        .expect("expected guidance");
    assert!(hint.contains("Result return-type queries"), "{hint}");
    assert!(
        hint.contains("fn $NAME($$ARGS) -> Result<$T> { $$BODY }"),
        "{hint}"
    );
    assert!(
        hint.contains("Do not retry the same fragment with grep"),
        "{hint}"
    );
}

#[test]
fn structural_pattern_preflight_guides_return_arrow_fragments() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "-> Result<$T>",
        "lang": "rust"
    }))
    .expect("valid request");

    let hint = preflight_parseable_pattern(&request)
        .expect("fragment hint should be returned")
        .expect("expected guidance");
    assert!(hint.contains("Result return-type queries"), "{hint}");
    assert!(
        hint.contains("fn $NAME($$ARGS) -> Result<$T> { $$BODY }"),
        "{hint}"
    );
}

#[tokio::test]
#[serial]
async fn structural_search_reports_missing_ast_grep() {
    let temp = TempDir::new().expect("workspace tempdir");
    let _override = set_ast_grep_binary_override_for_tests(None);

    let err = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "pattern": "fn $NAME() {}",
            "path": "."
        }),
    )
    .await
    .expect_err("missing ast-grep");

    let text = err.to_string();
    assert!(text.contains("ast-grep"));
    assert!(text.contains(AST_GREP_INSTALL_COMMAND));
}

#[tokio::test]
#[serial]
async fn structural_search_preflight_rejects_invalid_literal_pattern_before_ast_grep_runs() {
    let temp = TempDir::new().expect("workspace tempdir");
    let invoked_marker = temp.path().join("sg_invoked");
    let script = format!(
        "#!/bin/sh\ntouch \"{}\"\nprintf '[]'\n",
        invoked_marker.display()
    );
    let (_script_dir, script_path) = write_fake_sg(&script);

    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let err = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "pattern": "fn alpha( {}",
            "lang": "rust",
            "path": "."
        }),
    )
    .await
    .expect_err("invalid literal pattern should fail in preflight");

    let text = err.to_string();
    assert!(
        text.contains("structural pattern preflight failed"),
        "{text}"
    );
    assert!(text.contains("valid parseable code"), "{text}");
    assert!(!invoked_marker.exists(), "ast-grep should not be invoked");
}

#[tokio::test]
#[serial]
async fn structural_search_returns_fragment_guidance_without_running_ast_grep() {
    let temp = TempDir::new().expect("workspace tempdir");
    let invoked_marker = temp.path().join("sg_invoked");
    let script = format!(
        "#!/bin/sh\ntouch \"{}\"\nprintf '[]'\n",
        invoked_marker.display()
    );
    let (_script_dir, script_path) = write_fake_sg(&script);

    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let result = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "pattern": "Result<$T>",
            "lang": "rust",
            "path": "."
        }),
    )
    .await
    .expect("fragment guidance should be returned");

    assert_eq!(result["matches"], json!([]));
    assert_eq!(result["is_recoverable"], json!(true));
    assert!(
        result["next_action"]
            .as_str()
            .expect("next_action")
            .contains("larger parseable pattern")
    );
    let hint = result["hint"].as_str().expect("hint");
    assert!(hint.contains("Result return-type queries"), "{hint}");
    assert!(
        hint.contains("fn $NAME($$ARGS) -> Result<$T> { $$BODY }"),
        "{hint}"
    );
    assert!(
        hint.contains("Retry `unified_search` with `action='structural'`"),
        "{hint}"
    );
    assert!(!hint.contains("load_skill"), "{hint}");
    assert!(!invoked_marker.exists(), "ast-grep should not be invoked");
}

#[tokio::test]
#[serial]
async fn structural_search_arrow_fragment_guidance_prefers_direct_retry() {
    let temp = TempDir::new().expect("workspace tempdir");
    let invoked_marker = temp.path().join("sg_invoked");
    let script = format!(
        "#!/bin/sh\ntouch \"{}\"\nprintf '[]'\n",
        invoked_marker.display()
    );
    let (_script_dir, script_path) = write_fake_sg(&script);

    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let result = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "pattern": "-> Result<$T>",
            "lang": "rust",
            "path": "."
        }),
    )
    .await
    .expect("fragment guidance should be returned");

    assert_eq!(result["is_recoverable"], json!(true));
    let hint = result["hint"].as_str().expect("hint");
    assert!(hint.contains("Result return-type queries"), "{hint}");
    assert!(
        hint.contains("Retry `unified_search` with `action='structural'`"),
        "{hint}"
    );
    assert!(!hint.contains("load_skill"), "{hint}");
    assert!(!invoked_marker.exists(), "ast-grep should not be invoked");
}

#[tokio::test]
#[serial]
async fn structural_search_remaps_legacy_crates_paths_to_workspace_crates() {
    let temp = TempDir::new().expect("workspace tempdir");
    let crate_src = temp.path().join("vtcode-core").join("src");
    fs::create_dir_all(&crate_src).expect("create remapped crate src");
    let args_path = temp.path().join("sg_args.txt");
    let script = format!(
        "#!/bin/sh\nprintf '%s\n' \"$@\" > \"{}\"\nprintf '[]'\n",
        args_path.display()
    );
    let (_script_dir, script_path) = write_fake_sg(&script);

    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let legacy_path = temp.path().join("crates").join("vtcode-core").join("src");
    let result = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "pattern": "fn $NAME() {}",
            "lang": "rust",
            "path": legacy_path.to_string_lossy().to_string()
        }),
    )
    .await
    .expect("search should remap legacy crates path");

    assert_eq!(result["path"], json!("vtcode-core/src"));
    let args = fs::read_to_string(args_path).expect("read sg args");
    assert!(args.lines().any(|line| line == "vtcode-core/src"), "{args}");
}

#[tokio::test]
#[serial]
async fn structural_search_passes_leading_dash_patterns_with_equals_syntax() {
    let temp = TempDir::new().expect("workspace tempdir");
    let args_path = temp.path().join("sg_args.txt");
    let script = format!(
        "#!/bin/sh\nprintf '%s\n' \"$@\" > \"{}\"\nprintf '[]'\n",
        args_path.display()
    );
    let (_script_dir, script_path) = write_fake_sg(&script);

    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let result = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "pattern": "const X: i32 = -1;",
            "lang": "rust",
            "path": "."
        }),
    )
    .await
    .expect("search should run");

    assert_eq!(result["matches"], json!([]));
    let args = fs::read_to_string(args_path).expect("read sg args");
    assert!(
        args.lines()
            .any(|line| line == "--pattern=const X: i32 = -1;")
    );
}

#[tokio::test]
#[serial]
async fn structural_search_treats_exit_code_one_as_no_matches() {
    let temp = TempDir::new().expect("workspace tempdir");
    let script = "#!/bin/sh\nprintf '[]'\nexit 1\n";
    let (_script_dir, script_path) = write_fake_sg(script);

    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let result = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "pattern": "fn $NAME() {}",
            "lang": "rust",
            "path": "."
        }),
    )
    .await
    .expect("no-match exit should be normalized");

    assert_eq!(result["matches"], json!([]));
    assert_eq!(result["truncated"], false);
}

#[tokio::test]
#[serial]
async fn structural_search_passes_selector_and_strictness_flags() {
    let temp = TempDir::new().expect("workspace tempdir");
    let args_path = temp.path().join("sg_args.txt");
    let script = format!(
        "#!/bin/sh\nprintf '%s\n' \"$@\" > \"{}\"\nprintf '[]'\n",
        args_path.display()
    );
    let (_script_dir, script_path) = write_fake_sg(&script);

    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let result = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "pattern": "console.log($$$ARGS)",
            "lang": "javascript",
            "selector": "expression_statement",
            "strictness": "signature",
            "path": "."
        }),
    )
    .await
    .expect("search should run");

    assert_eq!(result["matches"], json!([]));
    let args = fs::read_to_string(args_path).expect("read sg args");
    assert!(args.lines().any(|line| line == "--lang"));
    assert!(args.lines().any(|line| line == "javascript"));
    assert!(args.lines().any(|line| line == "--selector"));
    assert!(args.lines().any(|line| line == "expression_statement"));
    assert!(args.lines().any(|line| line == "--strictness"));
    assert!(args.lines().any(|line| line == "signature"));
}

#[tokio::test]
#[serial]
async fn structural_search_passes_kind_flag() {
    let temp = TempDir::new().expect("workspace tempdir");
    let args_path = temp.path().join("sg_args.txt");
    let script = format!(
        "#!/bin/sh\nprintf '%s\n' \"$@\" > \"{}\"\nprintf '[]'\n",
        args_path.display()
    );
    let (_script_dir, script_path) = write_fake_sg(&script);

    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let result = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "kind": "function_item",
            "lang": "rust",
            "path": "."
        }),
    )
    .await
    .expect("kind-only search should run");

    assert_eq!(result["kind"], json!("function_item"));
    assert!(
        result.get("pattern").is_none(),
        "pattern should be absent for kind-only queries"
    );
    let args = fs::read_to_string(args_path).expect("read sg args");
    assert!(
        args.lines().any(|line| line == "--kind"),
        "--kind flag missing from args: {args}"
    );
    assert!(args.lines().any(|line| line == "function_item"));
    assert!(
        !args.lines().any(|line| line.starts_with("--pattern")),
        "--pattern should not be passed for kind-only queries: {args}"
    );
}

#[tokio::test]
#[serial]
async fn structural_search_passes_kind_with_pattern() {
    let temp = TempDir::new().expect("workspace tempdir");
    let args_path = temp.path().join("sg_args.txt");
    let script = format!(
        "#!/bin/sh\nprintf '%s\n' \"$@\" > \"{}\"\nprintf '[]'\n",
        args_path.display()
    );
    let (_script_dir, script_path) = write_fake_sg(&script);

    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let result = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "pattern": "fn $NAME() {}",
            "kind": "function_item",
            "lang": "rust",
            "path": "."
        }),
    )
    .await
    .expect("kind+pattern search should run");

    assert_eq!(result["kind"], json!("function_item"));
    assert_eq!(result["pattern"], json!("fn $NAME() {}"));
    let args = fs::read_to_string(args_path).expect("read sg args");
    assert!(args.lines().any(|line| line == "--kind"));
    assert!(args.lines().any(|line| line == "function_item"));
    assert!(args.lines().any(|line| line == "--pattern=fn $NAME() {}"));
}

#[tokio::test]
#[serial]
async fn structural_search_passes_esquery_compound_kind() {
    let temp = TempDir::new().expect("workspace tempdir");
    let args_path = temp.path().join("sg_args.txt");
    let script = format!(
        "#!/bin/sh\nprintf '%s\n' \"$@\" > \"{}\"\nprintf '[]'\n",
        args_path.display()
    );
    let (_script_dir, script_path) = write_fake_sg(&script);

    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let result = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "kind": "call_expression > identifier",
            "lang": "javascript",
            "path": "."
        }),
    )
    .await
    .expect("ESQuery compound kind should run");

    assert_eq!(result["kind"], json!("call_expression > identifier"));
    let args = fs::read_to_string(args_path).expect("read sg args");
    assert!(args.lines().any(|line| line == "--kind"));
    assert!(
        args.lines()
            .any(|line| line == "call_expression > identifier")
    );
}

#[tokio::test]
#[serial]
async fn structural_search_debug_query_uses_inferred_path_language() {
    let temp = TempDir::new().expect("workspace tempdir");
    let src_dir = temp.path().join("src");
    fs::create_dir_all(&src_dir).expect("create src dir");
    fs::write(src_dir.join("lib.rs"), "fn alpha() {}\n").expect("write rust file");
    let (_script_dir, script_path) = write_fake_sg("#!/bin/sh\nprintf 'query-ast'\n");

    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let result = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "pattern": "fn $NAME() {}",
            "path": "src/lib.rs",
            "debug_query": "ast"
        }),
    )
    .await
    .expect("debug query should succeed");

    assert_eq!(result["lang"], "rust");
    assert_eq!(result["debug_query"], "ast");
    assert_eq!(result["debug_query_output"], "query-ast");
}

#[test]
fn structural_path_defaults_to_workspace_root() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "fn $NAME() {}"
    }))
    .expect("valid request");

    assert_eq!(request.requested_path(), ".");
}

#[test]
fn structural_failure_message_includes_faq_guidance() {
    let text = format_ast_grep_failure(
        "ast-grep structural search failed",
        "parse error".to_string(),
    );

    assert!(text.contains("valid parseable code"));
    assert!(text.contains("use `selector`"));
    assert!(text.contains("`kind` and `pattern`"));
    assert!(text.contains("operators and keywords"));
    assert!(text.contains("`$$VAR`"));
    assert!(text.contains("whole AST node text"));
    assert!(text.contains("`constraints.regex`"));
    assert!(text.contains("override the default effective node"));
    assert!(text.contains("`strictness`"));
    assert!(text.contains("`smart` default"));
    assert!(text.contains("`debug_query`"));
    assert!(text.contains("not scope/type/data-flow analysis"));
    assert!(text.contains("transform"));
    assert!(text.contains("replace for regex substitution"));
    assert!(text.contains("substring for Python-style"));
    assert!(text.contains("convert for identifier case changes"));
    assert!(text.contains("Retry `unified_search`"));
    assert!(text.contains("`unified_exec`"));
}

#[test]
fn structural_failure_message_skips_project_config_hint_for_parse_errors() {
    let text = format_ast_grep_failure(
        "ast-grep structural search failed",
        "parse error near pattern".to_string(),
    );

    assert!(!text.contains("customLanguages"));
    assert!(!text.contains("languageGlobs"));
    assert!(!text.contains("languageInjections"));
}

#[test]
fn structural_failure_message_includes_custom_language_guidance() {
    let text = format_ast_grep_failure(
        "ast-grep structural search failed",
        "unsupported language: mojo".to_string(),
    );

    assert!(text.contains("customLanguages"));
    assert!(text.contains("tree-sitter dynamic library"));
    assert!(text.contains("tree-sitter build"));
    assert!(text.contains("TREE_SITTER_LIBDIR"));
    assert!(text.contains("Neovim"));
    assert!(text.contains("`<script>` / `<style>`"));
    assert!(text.contains("languageGlobs"));
    assert!(text.contains("languageInjections"));
    assert!(text.contains("hostLanguage"));
    assert!(text.contains("injected"));
    assert!(text.contains("$CONTENT"));
    assert!(text.contains("expandoChar"));
    assert!(text.contains("tree-sitter parse"));
    assert!(text.contains("single-language"));
    assert!(text.contains("Retry `unified_search`"));
    assert!(text.contains("bundled `ast-grep` skill"));
    assert!(text.contains("`unified_exec`"));
}

#[tokio::test]
#[serial]
async fn structural_search_failure_surfaces_faq_guidance() {
    let temp = TempDir::new().expect("workspace tempdir");
    let (_script_dir, script_path) =
        write_fake_sg("#!/bin/sh\nprintf 'parse error near pattern' >&2\nexit 2\n");

    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let err = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "pattern": "\"key\": \"$VAL\"",
            "lang": "json",
            "path": "."
        }),
    )
    .await
    .expect_err("structural search should fail");

    let text = err.to_string();
    assert!(text.contains("valid parseable code"));
    assert!(text.contains("use `selector`"));
    assert!(!text.contains("customLanguages"));
    assert!(text.contains("Retry `unified_search`"));
}

#[tokio::test]
#[serial]
async fn structural_search_failure_surfaces_custom_language_guidance() {
    let temp = TempDir::new().expect("workspace tempdir");
    let (_script_dir, script_path) =
        write_fake_sg("#!/bin/sh\nprintf 'unsupported language: mojo' >&2\nexit 2\n");

    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let err = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "pattern": "target($A)",
            "lang": "mojo",
            "path": "."
        }),
    )
    .await
    .expect_err("structural search should fail");

    let text = err.to_string();
    assert!(text.contains("customLanguages"), "{text}");
    assert!(text.contains("tree-sitter build"), "{text}");
    assert!(text.contains("TREE_SITTER_LIBDIR"), "{text}");
    assert!(text.contains("Neovim"), "{text}");
    assert!(text.contains("`<script>` / `<style>`"), "{text}");
    assert!(text.contains("languageGlobs"), "{text}");
    assert!(text.contains("languageInjections"), "{text}");
    assert!(text.contains("hostLanguage"), "{text}");
    assert!(text.contains("injected"), "{text}");
    assert!(text.contains("$CONTENT"), "{text}");
    assert!(text.contains("expandoChar"), "{text}");
    assert!(text.contains("tree-sitter parse"), "{text}");
    assert!(text.contains("Retry `unified_search`"), "{text}");
    assert!(text.contains("`unified_exec`"), "{text}");
}

#[tokio::test]
#[serial]
async fn structural_scan_normalizes_findings_and_truncates() {
    let temp = TempDir::new().expect("workspace tempdir");
    fs::create_dir_all(temp.path().join("src")).expect("create src");
    fs::write(temp.path().join("sgconfig.yml"), "ruleDirs: []\n").expect("write config");
    let args_path = temp.path().join("sg_args.txt");
    let script = format!(
        "#!/bin/sh\nprintf '%s\n' \"$@\" > \"{}\"\nprintf '%s\n' '{}' '{}'\n",
        args_path.display(),
        r#"{"text":"items.iter().for_each(handle);","range":{"start":{"line":5,"column":4},"end":{"line":5,"column":29}},"file":"src/lib.rs","lines":"5: items.iter().for_each(handle);","language":"Rust","ruleId":"no-iterator-for-each","severity":"warning","message":"Prefer a for loop.","note":"Avoid side-effect-only for_each.","metadata":{"docs":"https://example.com/rule","category":"style"}}"#,
        r#"{"text":"items.into_iter().for_each(handle);","range":{"start":{"line":9,"column":0},"end":{"line":9,"column":34}},"file":"src/main.rs","lines":"9: items.into_iter().for_each(handle);","language":"Rust","ruleId":"no-iterator-for-each","severity":"warning","message":"Prefer a for loop.","note":null,"metadata":{"docs":"https://example.com/rule","category":"style"}}"#
    );
    let (_script_dir, script_path) = write_fake_sg(&script);

    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let result = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "workflow": "scan",
            "path": "src",
            "config_path": "sgconfig.yml",
            "filter": "no-iterator-for-each",
            "globs": ["**/*.rs", "!target/**"],
            "context_lines": 2,
            "max_results": 1
        }),
    )
    .await
    .expect("scan should succeed");

    assert_eq!(result["backend"], "ast-grep");
    assert_eq!(result["workflow"], "scan");
    assert_eq!(result["path"], "src");
    assert_eq!(result["config_path"], "sgconfig.yml");
    assert_eq!(result["truncated"], true);

    let findings = result["findings"].as_array().expect("findings");
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0]["file"], "src/lib.rs");
    assert_eq!(findings[0]["line_number"], 5);
    assert_eq!(findings[0]["language"], "Rust");
    assert_eq!(findings[0]["rule_id"], "no-iterator-for-each");
    assert_eq!(findings[0]["severity"], "warning");
    assert_eq!(findings[0]["message"], "Prefer a for loop.");
    assert_eq!(findings[0]["note"], "Avoid side-effect-only for_each.");
    assert_eq!(findings[0]["metadata"]["category"], "style");
    assert_eq!(result["summary"]["total_findings"], 2);
    assert_eq!(result["summary"]["returned_findings"], 1);
    assert_eq!(result["summary"]["by_rule"]["no-iterator-for-each"], 2);
    assert_eq!(result["summary"]["by_severity"]["warning"], 2);

    let args = fs::read_to_string(args_path).expect("read sg args");
    assert!(args.lines().any(|line| line == "scan"));
    assert!(args.lines().any(|line| line == "--config"));
    assert!(args.lines().any(|line| line == "sgconfig.yml"));
    assert!(args.lines().any(|line| line == "--filter"));
    assert!(args.lines().any(|line| line == "no-iterator-for-each"));
    assert!(args.lines().any(|line| line == "--globs"));
    assert!(args.lines().any(|line| line == "--context"));
    assert!(args.lines().any(|line| line == "2"));
    assert!(args.lines().any(|line| line == "src"));
}

#[tokio::test]
#[serial]
async fn structural_scan_treats_exit_code_one_as_findings() {
    let temp = TempDir::new().expect("workspace tempdir");
    fs::create_dir_all(temp.path().join("src")).expect("create src");
    fs::write(temp.path().join("sgconfig.yml"), "ruleDirs: []\n").expect("write config");
    let script = format!(
        "#!/bin/sh\nprintf '%s\n' '{}' >&1\nexit 1\n",
        r#"{"text":"danger();","range":{"start":{"line":3,"column":2},"end":{"line":3,"column":10}},"file":"src/lib.rs","lines":"3: danger();","language":"Rust","ruleId":"deny-danger","severity":"error","message":"Do not call danger.","note":null,"metadata":{"docs":"https://example.com/rule"}}"#,
    );
    let (_script_dir, script_path) = write_fake_sg(&script);

    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let result = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "workflow": "scan",
            "path": "src",
            "config_path": "sgconfig.yml"
        }),
    )
    .await
    .expect("error-severity findings should be normalized");

    assert_eq!(result["backend"], "ast-grep");
    assert_eq!(result["workflow"], "scan");
    assert_eq!(result["findings"][0]["rule_id"], "deny-danger");
    assert_eq!(result["findings"][0]["severity"], "error");
    assert_eq!(result["summary"]["total_findings"], 1);
}

#[tokio::test]
#[serial]
async fn structural_test_returns_stdout_stderr_and_summary() {
    let temp = TempDir::new().expect("workspace tempdir");
    fs::create_dir_all(temp.path().join("config")).expect("create config dir");
    fs::write(temp.path().join("config/sgconfig.yml"), "ruleDirs: []\n").expect("write config");
    let args_path = temp.path().join("sg_args.txt");
    let script = format!(
        "#!/bin/sh\nprintf '%s\n' \"$@\" > \"{}\"\nprintf '\\033[32mRunning 2 tests\\033[0m\nPASS rust/no-iterator-for-each\nFAIL rust/for-each-snapshot\ntest result: failed. 1 passed; 1 failed;\n'\nprintf 'snapshot mismatch\n' >&2\nexit 1\n",
        args_path.display(),
    );
    let (_script_dir, script_path) = write_fake_sg(&script);

    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let result = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "workflow": "test",
            "config_path": "config/sgconfig.yml",
            "filter": "rust/no-iterator-for-each",
            "skip_snapshot_tests": true
        }),
    )
    .await
    .expect("test workflow should return structured result");

    assert_eq!(result["backend"], "ast-grep");
    assert_eq!(result["workflow"], "test");
    assert_eq!(result["config_path"], "config/sgconfig.yml");
    assert_eq!(result["passed"], false);
    assert!(
        result["stdout"]
            .as_str()
            .expect("stdout")
            .contains("Running 2 tests")
    );
    assert!(
        result["stderr"]
            .as_str()
            .expect("stderr")
            .contains("snapshot mismatch")
    );
    assert_eq!(result["summary"]["status"], "failed");
    assert_eq!(result["summary"]["passed_cases"], 1);
    assert_eq!(result["summary"]["failed_cases"], 1);
    assert_eq!(result["summary"]["total_cases"], 2);

    let args = fs::read_to_string(args_path).expect("read sg args");
    assert!(args.lines().any(|line| line == "test"));
    assert!(args.lines().any(|line| line == "--config"));
    assert!(args.lines().any(|line| line == "config/sgconfig.yml"));
    assert!(args.lines().any(|line| line == "--filter"));
    assert!(args.lines().any(|line| line == "rust/no-iterator-for-each"));
    assert!(args.lines().any(|line| line == "--skip-snapshot-tests"));
}

#[test]
fn normalize_match_emits_byte_offset() {
    let match_value = normalize_match(AstGrepMatch {
        file: "src/lib.rs".to_string(),
        text: "fn alpha() {}".to_string(),
        lines: None,
        language: None,
        range: AstGrepRange {
            start: AstGrepPoint {
                line: 12,
                column: 0,
            },
            end: AstGrepPoint {
                line: 12,
                column: 13,
            },
            byte_offset: Some(AstGrepByteOffset {
                start: 200,
                end: 213,
            }),
        },
        meta_variables: None,
    });

    assert_eq!(match_value["range"]["byteOffset"]["start"], 200);
    assert_eq!(match_value["range"]["byteOffset"]["end"], 213);
}

#[test]
fn normalize_match_omits_byte_offset_when_absent() {
    let match_value = normalize_match(AstGrepMatch {
        file: "src/lib.rs".to_string(),
        text: "fn alpha() {}".to_string(),
        lines: None,
        language: None,
        range: AstGrepRange {
            start: AstGrepPoint {
                line: 12,
                column: 0,
            },
            end: AstGrepPoint {
                line: 12,
                column: 13,
            },
            byte_offset: None,
        },
        meta_variables: None,
    });

    assert!(match_value["range"].get("byteOffset").is_none());
}

#[test]
fn normalize_match_emits_meta_variables() {
    let mut single = std::collections::BTreeMap::new();
    single.insert(
        "NAME".to_string(),
        AstGrepMetaVar {
            text: "alpha".to_string(),
            range: AstGrepRange {
                start: AstGrepPoint {
                    line: 12,
                    column: 3,
                },
                end: AstGrepPoint {
                    line: 12,
                    column: 8,
                },
                byte_offset: None,
            },
        },
    );
    let mut multi = std::collections::BTreeMap::new();
    multi.insert(
        "ARGS".to_string(),
        vec![AstGrepMetaVar {
            text: "a".to_string(),
            range: AstGrepRange {
                start: AstGrepPoint {
                    line: 12,
                    column: 9,
                },
                end: AstGrepPoint {
                    line: 12,
                    column: 10,
                },
                byte_offset: None,
            },
        }],
    );
    let mut transformed = std::collections::BTreeMap::new();
    transformed.insert("UPPER".to_string(), "ALPHA".to_string());

    let match_value = normalize_match(AstGrepMatch {
        file: "src/lib.rs".to_string(),
        text: "fn alpha(a) {}".to_string(),
        lines: None,
        language: None,
        range: AstGrepRange {
            start: AstGrepPoint {
                line: 12,
                column: 0,
            },
            end: AstGrepPoint {
                line: 12,
                column: 14,
            },
            byte_offset: None,
        },
        meta_variables: Some(AstGrepMetaVariables {
            single,
            multi,
            transformed,
        }),
    });

    assert_eq!(
        match_value["metaVariables"]["single"]["NAME"]["text"],
        "alpha"
    );
    assert_eq!(
        match_value["metaVariables"]["single"]["NAME"]["range"]["start"]["line"],
        12
    );
    assert_eq!(
        match_value["metaVariables"]["multi"]["ARGS"][0]["text"],
        "a"
    );
    assert_eq!(
        match_value["metaVariables"]["transformed"]["UPPER"],
        "ALPHA"
    );
}

#[test]
fn normalize_match_omits_meta_variables_when_absent() {
    let match_value = normalize_match(AstGrepMatch {
        file: "src/lib.rs".to_string(),
        text: "fn alpha() {}".to_string(),
        lines: None,
        language: None,
        range: AstGrepRange {
            start: AstGrepPoint {
                line: 12,
                column: 0,
            },
            end: AstGrepPoint {
                line: 12,
                column: 13,
            },
            byte_offset: None,
        },
        meta_variables: None,
    });

    assert!(match_value.get("metaVariables").is_none());
}

#[test]
fn normalize_scan_finding_extracts_url_from_metadata() {
    let finding = AstGrepScanFinding {
        file: "src/lib.rs".to_string(),
        text: "danger();".to_string(),
        lines: None,
        language: None,
        range: AstGrepRange {
            start: AstGrepPoint { line: 3, column: 2 },
            end: AstGrepPoint {
                line: 3,
                column: 10,
            },
            byte_offset: None,
        },
        rule_id: Some("deny-danger".to_string()),
        severity: Some("error".to_string()),
        message: Some("Do not call danger.".to_string()),
        note: None,
        metadata: Some(serde_json::json!({"docs": "https://example.com/rule"})),
        labels: vec![],
    };

    let value = normalize_scan_finding(&finding);
    assert_eq!(value["url"], "https://example.com/rule");
    assert_eq!(value["metadata"]["docs"], "https://example.com/rule");
}

#[test]
fn normalize_scan_finding_prefers_url_over_docs() {
    let finding = AstGrepScanFinding {
        file: "src/lib.rs".to_string(),
        text: "danger();".to_string(),
        lines: None,
        language: None,
        range: AstGrepRange {
            start: AstGrepPoint { line: 3, column: 2 },
            end: AstGrepPoint {
                line: 3,
                column: 10,
            },
            byte_offset: None,
        },
        rule_id: Some("deny-danger".to_string()),
        severity: Some("error".to_string()),
        message: Some("Do not call danger.".to_string()),
        note: None,
        metadata: Some(serde_json::json!({
            "url": "https://example.com/primary",
            "docs": "https://example.com/secondary"
        })),
        labels: vec![],
    };

    let value = normalize_scan_finding(&finding);
    assert_eq!(value["url"], "https://example.com/primary");
}

#[test]
fn normalize_scan_finding_omits_url_when_metadata_empty() {
    let finding = AstGrepScanFinding {
        file: "src/lib.rs".to_string(),
        text: "danger();".to_string(),
        lines: None,
        language: None,
        range: AstGrepRange {
            start: AstGrepPoint { line: 3, column: 2 },
            end: AstGrepPoint {
                line: 3,
                column: 10,
            },
            byte_offset: None,
        },
        rule_id: Some("deny-danger".to_string()),
        severity: Some("error".to_string()),
        message: Some("Do not call danger.".to_string()),
        note: None,
        metadata: Some(serde_json::json!({"category": "security"})),
        labels: vec![],
    };

    let value = normalize_scan_finding(&finding);
    assert!(value.get("url").is_none());
    assert_eq!(value["metadata"]["category"], "security");
}

#[test]
fn deserialize_compact_match_with_meta_variables() {
    let json_input = r#"[{
        "file": "src/lib.rs",
        "text": "fn alpha() {}",
        "lines": "12: fn alpha() {}",
        "language": "Rust",
        "range": {
            "byteOffset": { "start": 200, "end": 213 },
            "start": { "line": 12, "column": 0 },
            "end": { "line": 12, "column": 13 }
        },
        "metaVariables": {
            "single": {
                "NAME": { "text": "alpha", "range": { "start": { "line": 12, "column": 3 }, "end": { "line": 12, "column": 8 } } }
            },
            "multi": {},
            "transformed": {}
        }
    }]"#;

    let matches = parse_compact_matches(json_input.as_bytes()).expect("should parse");
    assert_eq!(matches.len(), 1);
    let m = &matches[0];
    assert_eq!(m.range.byte_offset.as_ref().unwrap().start, 200);
    assert_eq!(
        m.meta_variables.as_ref().unwrap().single["NAME"].text,
        "alpha"
    );
}

#[test]
fn deserialize_compact_match_without_optional_fields() {
    let json_input = r#"[{
        "file": "src/lib.rs",
        "text": "fn alpha() {}",
        "range": {
            "start": { "line": 12, "column": 0 },
            "end": { "line": 12, "column": 13 }
        }
    }]"#;

    let matches = parse_compact_matches(json_input.as_bytes()).expect("should parse");
    assert_eq!(matches.len(), 1);
    let m = &matches[0];
    assert!(m.range.byte_offset.is_none());
    assert!(m.meta_variables.is_none());
    assert!(m.lines.is_none());
    assert!(m.language.is_none());
}

#[test]
fn normalize_scan_finding_emits_labels_when_present() {
    let finding = AstGrepScanFinding {
        file: "src/lib.rs".to_string(),
        text: "danger();".to_string(),
        lines: None,
        language: None,
        range: AstGrepRange {
            start: AstGrepPoint { line: 3, column: 2 },
            end: AstGrepPoint {
                line: 3,
                column: 10,
            },
            byte_offset: None,
        },
        rule_id: Some("deny-danger".to_string()),
        severity: Some("error".to_string()),
        message: Some("Do not call danger.".to_string()),
        note: None,
        metadata: None,
        labels: vec![AstGrepLabel {
            text: "dangerous call".to_string(),
            range: AstGrepRange {
                start: AstGrepPoint { line: 3, column: 2 },
                end: AstGrepPoint {
                    line: 3,
                    column: 10,
                },
                byte_offset: None,
            },
            source: Some("rule".to_string()),
        }],
    };

    let value = normalize_scan_finding(&finding);
    let labels = value["labels"].as_array().expect("labels array");
    assert_eq!(labels.len(), 1);
    assert_eq!(labels[0]["text"], "dangerous call");
    assert_eq!(labels[0]["range"]["start"]["line"], 3);
    assert_eq!(labels[0]["source"], "rule");
}

#[test]
fn normalize_scan_finding_omits_labels_when_empty() {
    let finding = AstGrepScanFinding {
        file: "src/lib.rs".to_string(),
        text: "danger();".to_string(),
        lines: None,
        language: None,
        range: AstGrepRange {
            start: AstGrepPoint { line: 3, column: 2 },
            end: AstGrepPoint {
                line: 3,
                column: 10,
            },
            byte_offset: None,
        },
        rule_id: Some("deny-danger".to_string()),
        severity: Some("error".to_string()),
        message: Some("Do not call danger.".to_string()),
        note: None,
        metadata: None,
        labels: vec![],
    };

    let value = normalize_scan_finding(&finding);
    assert!(
        value.get("labels").is_none(),
        "labels should be omitted when empty"
    );
}

#[test]
fn normalize_scan_finding_emits_multiple_labels() {
    let finding = AstGrepScanFinding {
        file: "src/lib.rs".to_string(),
        text: "let x = foo(a, b);".to_string(),
        lines: None,
        language: None,
        range: AstGrepRange {
            start: AstGrepPoint { line: 5, column: 0 },
            end: AstGrepPoint {
                line: 5,
                column: 19,
            },
            byte_offset: None,
        },
        rule_id: Some("no-foo".to_string()),
        severity: Some("warning".to_string()),
        message: Some("Avoid foo.".to_string()),
        note: None,
        metadata: None,
        labels: vec![
            AstGrepLabel {
                text: "foo call".to_string(),
                range: AstGrepRange {
                    start: AstGrepPoint { line: 5, column: 8 },
                    end: AstGrepPoint {
                        line: 5,
                        column: 16,
                    },
                    byte_offset: None,
                },
                source: None,
            },
            AstGrepLabel {
                text: "assignment".to_string(),
                range: AstGrepRange {
                    start: AstGrepPoint { line: 5, column: 0 },
                    end: AstGrepPoint {
                        line: 5,
                        column: 19,
                    },
                    byte_offset: None,
                },
                source: Some("rule".to_string()),
            },
        ],
    };

    let value = normalize_scan_finding(&finding);
    let labels = value["labels"].as_array().expect("labels array");
    assert_eq!(labels.len(), 2);
    assert_eq!(labels[0]["text"], "foo call");
    assert!(
        labels[0].get("source").is_none(),
        "source should be omitted when absent"
    );
    assert_eq!(labels[1]["text"], "assignment");
    assert_eq!(labels[1]["source"], "rule");
}

#[test]
fn deserialize_scan_finding_with_labels() {
    let json_input = r#"{"text":"danger();","range":{"start":{"line":3,"column":2},"end":{"line":3,"column":10}},"file":"src/lib.rs","lines":"3: danger();","language":"Rust","ruleId":"deny-danger","severity":"error","message":"Do not call danger.","note":null,"labels":[{"text":"call site","range":{"start":{"line":3,"column":2},"end":{"line":3,"column":10}},"source":"rule"}]}"#;

    let finding: AstGrepScanFinding =
        serde_json::from_str(json_input).expect("should parse scan finding with labels");
    assert_eq!(finding.labels.len(), 1);
    assert_eq!(finding.labels[0].text, "call site");
    assert_eq!(finding.labels[0].source.as_deref(), Some("rule"));
}

#[test]
fn deserialize_scan_finding_without_labels() {
    let json_input = r#"{"text":"danger();","range":{"start":{"line":3,"column":2},"end":{"line":3,"column":10}},"file":"src/lib.rs","ruleId":"deny-danger","severity":"error","message":"Do not call danger."}"#;

    let finding: AstGrepScanFinding =
        serde_json::from_str(json_input).expect("should parse scan finding without labels");
    assert!(finding.labels.is_empty());
}

// --- Config validation tests ---

#[tokio::test]
#[serial]
async fn structural_scan_reports_missing_default_config_with_actionable_guidance() {
    let temp = TempDir::new().expect("workspace tempdir");

    let err = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "workflow": "scan",
            "path": "."
        }),
    )
    .await
    .expect_err("missing config should fail");

    let text = err.to_string();
    assert!(text.contains("sgconfig.yml"), "{text}");
    assert!(text.contains("ruleDirs"), "{text}");
    assert!(text.contains("ast-grep new project"), "{text}");
    assert!(text.contains("bundled `ast-grep` skill"), "{text}");
}

#[tokio::test]
#[serial]
async fn structural_test_reports_missing_default_config_with_actionable_guidance() {
    let temp = TempDir::new().expect("workspace tempdir");

    let err = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "workflow": "test"
        }),
    )
    .await
    .expect_err("missing config should fail");

    let text = err.to_string();
    assert!(text.contains("sgconfig.yml"), "{text}");
    assert!(text.contains("ruleDirs"), "{text}");
}

#[tokio::test]
#[serial]
async fn structural_scan_reports_missing_custom_config_with_specific_path() {
    let temp = TempDir::new().expect("workspace tempdir");

    let err = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "workflow": "scan",
            "path": ".",
            "config_path": "custom/sgconfig.yml"
        }),
    )
    .await
    .expect_err("missing custom config should fail");

    let text = err.to_string();
    assert!(text.contains("custom/sgconfig.yml"), "{text}");
    assert!(text.contains("not found"), "{text}");
    assert!(text.contains("bundled `ast-grep` skill"), "{text}");
}

#[tokio::test]
#[serial]
async fn structural_scan_discovers_config_in_parent_directory() {
    let parent = TempDir::new().expect("parent tempdir");
    let child = parent.path().join("subdir");
    fs::create_dir_all(&child).expect("create subdir");
    fs::write(parent.path().join("sgconfig.yml"), "ruleDirs: []\n").expect("write config");

    let err = execute_structural_search(
        &child,
        json!({
            "action": "structural",
            "workflow": "scan",
            "path": "."
        }),
    )
    .await
    .expect_err("scan should fail but mention discovered config");

    let text = err.to_string();
    assert!(text.contains("sgconfig.yml"), "{text}");
    // Should mention the discovered config in parent
    assert!(
        text.contains("found `sgconfig.yml`") || text.contains("Note:"),
        "{text}"
    );
}

#[tokio::test]
#[serial]
async fn structural_scan_succeeds_when_config_exists() {
    let temp = TempDir::new().expect("workspace tempdir");
    fs::create_dir_all(temp.path().join("src")).expect("create src");
    fs::write(temp.path().join("sgconfig.yml"), "ruleDirs: []\n").expect("write config");
    let script = "#!/bin/sh\nprintf ''\n";
    let (_script_dir, script_path) = write_fake_sg(script);

    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let result = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "workflow": "scan",
            "path": "src",
            "config_path": "sgconfig.yml"
        }),
    )
    .await
    .expect("scan should succeed when config exists");

    assert_eq!(result["backend"], "ast-grep");
    assert_eq!(result["workflow"], "scan");
}

// --- Inspect workflow tests ---

#[tokio::test]
#[serial]
async fn structural_inspect_reports_config_exists() {
    let temp = TempDir::new().expect("workspace tempdir");
    fs::write(temp.path().join("sgconfig.yml"), "ruleDirs:\n  - rules\n").expect("write config");

    let result = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "workflow": "inspect"
        }),
    )
    .await
    .expect("inspect should succeed");

    assert_eq!(result["backend"], "ast-grep");
    assert_eq!(result["workflow"], "inspect");
    assert_eq!(result["config_exists"], true);
    assert_eq!(result["config_path"], "sgconfig.yml");
    let hints = result["rule_dir_hints"].as_array().expect("rule_dir_hints");
    assert_eq!(hints.len(), 1);
    assert_eq!(hints[0], "rules");
}

#[tokio::test]
#[serial]
async fn structural_inspect_reports_config_missing() {
    let temp = TempDir::new().expect("workspace tempdir");

    let result = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "workflow": "inspect"
        }),
    )
    .await
    .expect("inspect should succeed even without config");

    assert_eq!(result["backend"], "ast-grep");
    assert_eq!(result["workflow"], "inspect");
    assert_eq!(result["config_exists"], false);
    let hints = result["rule_dir_hints"].as_array().expect("rule_dir_hints");
    assert!(hints.is_empty());
    let discovered = result["discovered_configs"]
        .as_array()
        .expect("discovered_configs");
    assert!(discovered.is_empty());
}

#[tokio::test]
#[serial]
async fn structural_inspect_discovers_parent_config() {
    let parent = TempDir::new().expect("parent tempdir");
    let child = parent.path().join("subdir");
    fs::create_dir_all(&child).expect("create subdir");
    fs::write(parent.path().join("sgconfig.yml"), "ruleDirs:\n  - rules\n").expect("write config");

    let result = execute_structural_search(
        &child,
        json!({
            "action": "structural",
            "workflow": "inspect"
        }),
    )
    .await
    .expect("inspect should succeed");

    assert_eq!(result["config_exists"], false);
    let discovered = result["discovered_configs"]
        .as_array()
        .expect("discovered_configs");
    assert_eq!(discovered.len(), 1);
    // Should contain the relative path to the parent config
    let discovered_path = discovered[0].as_str().expect("discovered path");
    assert!(
        discovered_path.contains("sgconfig.yml"),
        "{discovered_path}"
    );
}

#[tokio::test]
#[serial]
async fn structural_inspect_parses_inline_rule_dirs() {
    let temp = TempDir::new().expect("workspace tempdir");
    fs::write(
        temp.path().join("sgconfig.yml"),
        "ruleDirs: [rules, custom-rules]\n",
    )
    .expect("write config");

    let result = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "workflow": "inspect"
        }),
    )
    .await
    .expect("inspect should succeed");

    let hints = result["rule_dir_hints"].as_array().expect("rule_dir_hints");
    assert_eq!(hints.len(), 2);
    assert_eq!(hints[0], "rules");
    assert_eq!(hints[1], "custom-rules");
}

#[test]
fn structural_request_defaults_workflow_still_query() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "fn $NAME() {}"
    }))
    .expect("valid request");

    assert_eq!(request.workflow, StructuralWorkflow::Query);
}

#[test]
fn structural_request_accepts_inspect_workflow() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "inspect"
    }))
    .expect("inspect workflow should be valid");

    assert_eq!(request.workflow, StructuralWorkflow::Inspect);
}

#[test]
fn structural_inspect_rejects_pattern() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "inspect",
        "pattern": "fn $NAME() {}"
    }))
    .expect_err("inspect rejects pattern");

    assert!(err.to_string().contains("workflow='inspect'"));
    assert!(err.to_string().contains("does not accept `pattern`"));
}

#[test]
fn structural_inspect_rejects_globs() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "inspect",
        "globs": ["**/*.rs"]
    }))
    .expect_err("inspect rejects globs");

    assert!(err.to_string().contains("workflow='inspect'"));
    assert!(err.to_string().contains("does not accept `globs`"));
}

#[test]
fn structural_inspect_rejects_filter() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "inspect",
        "filter": "some-rule"
    }))
    .expect_err("inspect rejects filter");

    assert!(err.to_string().contains("workflow='inspect'"));
    assert!(err.to_string().contains("does not accept `filter`"));
}

#[tokio::test]
#[serial]
async fn structural_inspect_accepts_config_path_and_path() {
    let temp = TempDir::new().expect("workspace tempdir");
    let sub = temp.path().join("project");
    fs::create_dir_all(&sub).expect("create project dir");
    fs::write(sub.join("sgconfig.yml"), "ruleDirs:\n  - rules\n").expect("write config");

    let result = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "workflow": "inspect",
            "path": "project",
            "config_path": "project/sgconfig.yml"
        }),
    )
    .await
    .expect("inspect with custom paths should succeed");

    assert_eq!(result["config_exists"], true);
    assert_eq!(result["project_dir"], "project");
    assert_eq!(result["config_path"], "project/sgconfig.yml");
}

// --- Language injection extraction tests ---

#[tokio::test]
async fn extract_language_injections_from_config() {
    let temp = TempDir::new().expect("tempdir");
    // Use simple YAML without nested rule objects to isolate the parser
    fs::write(
        temp.path().join("sgconfig.yml"),
        "\
languageInjections:
- hostLanguage: js
  injected: css
- hostLanguage: html
  injected: javascript
",
    )
    .expect("write config");

    let config_file = temp.path().join("sgconfig.yml");
    let injections = extract_language_injections(&config_file).await;
    assert_eq!(injections.len(), 2, "injections: {injections:?}");
    assert_eq!(injections[0]["hostLanguage"], "js");
    assert_eq!(injections[0]["injected"], "css");
    assert_eq!(injections[1]["hostLanguage"], "html");
    assert_eq!(injections[1]["injected"], "javascript");
}

#[tokio::test]
async fn extract_language_injections_returns_empty_for_missing_config() {
    let injections = extract_language_injections(Path::new("/nonexistent/sgconfig.yml")).await;
    assert!(injections.is_empty());
}

#[tokio::test]
async fn extract_language_injections_returns_empty_when_section_absent() {
    let temp = TempDir::new().expect("tempdir");
    fs::write(temp.path().join("sgconfig.yml"), "ruleDirs:\n  - rules\n").expect("write config");

    let injections = extract_language_injections(&temp.path().join("sgconfig.yml")).await;
    assert!(injections.is_empty());
}

#[tokio::test]
async fn extract_language_injections_handles_graphql_template_literal() {
    let temp = TempDir::new().expect("tempdir");
    fs::write(
        temp.path().join("sgconfig.yml"),
        "\
languageInjections:
- hostLanguage: js
  injected: graphql
",
    )
    .expect("write config");

    let injections = extract_language_injections(&temp.path().join("sgconfig.yml")).await;
    assert_eq!(injections.len(), 1);
    assert_eq!(injections[0]["hostLanguage"], "js");
    assert_eq!(injections[0]["injected"], "graphql");
}

// --- Custom languages extraction tests ---

#[tokio::test]
async fn extract_custom_languages_from_config() {
    let temp = TempDir::new().expect("tempdir");
    fs::write(
        temp.path().join("sgconfig.yml"),
        "ruleDirs:\n  - rules\ncustomLanguages:\n  graphql:\n    libraryPath: graphql.so\n    extensions: [graphql]\n    expandoChar: $\n",
    )
    .expect("write config");

    let languages = extract_custom_languages(&temp.path().join("sgconfig.yml")).await;
    assert_eq!(languages["graphql"]["libraryPath"], "graphql.so");
    assert_eq!(languages["graphql"]["expandoChar"], "$");
    let extensions = languages["graphql"]["extensions"]
        .as_array()
        .expect("extensions array");
    assert_eq!(extensions.len(), 1);
    assert_eq!(extensions[0], "graphql");
}

#[tokio::test]
async fn extract_custom_languages_returns_empty_for_missing_config() {
    let languages = extract_custom_languages(Path::new("/nonexistent/sgconfig.yml")).await;
    assert!(languages.as_object().expect("object").is_empty());
}

#[tokio::test]
async fn extract_custom_languages_returns_empty_when_section_absent() {
    let temp = TempDir::new().expect("tempdir");
    fs::write(temp.path().join("sgconfig.yml"), "ruleDirs:\n  - rules\n").expect("write config");

    let languages = extract_custom_languages(&temp.path().join("sgconfig.yml")).await;
    assert!(languages.as_object().expect("object").is_empty());
}

// --- Language globs extraction tests ---

#[tokio::test]
async fn extract_language_globs_from_config() {
    let temp = TempDir::new().expect("tempdir");
    fs::write(
        temp.path().join("sgconfig.yml"),
        "ruleDirs:\n  - rules\nlanguageGlobs:\n  tsx:\n    - \"*.tsx\"\n    - \"*.jsx\"\n  javascript:\n    - \"*.mjs\"\n",
    )
    .expect("write config");

    let globs = extract_language_globs(&temp.path().join("sgconfig.yml")).await;
    let tsx_patterns = globs["tsx"].as_array().expect("tsx array");
    assert_eq!(tsx_patterns.len(), 2);
    assert_eq!(tsx_patterns[0], "*.tsx");
    assert_eq!(tsx_patterns[1], "*.jsx");
    let js_patterns = globs["javascript"].as_array().expect("js array");
    assert_eq!(js_patterns.len(), 1);
    assert_eq!(js_patterns[0], "*.mjs");
}

#[tokio::test]
async fn extract_language_globs_returns_empty_for_missing_config() {
    let globs = extract_language_globs(Path::new("/nonexistent/sgconfig.yml")).await;
    assert!(globs.as_object().expect("object").is_empty());
}

#[tokio::test]
async fn extract_language_globs_returns_empty_when_section_absent() {
    let temp = TempDir::new().expect("tempdir");
    fs::write(temp.path().join("sgconfig.yml"), "ruleDirs:\n  - rules\n").expect("write config");

    let globs = extract_language_globs(&temp.path().join("sgconfig.yml")).await;
    assert!(globs.as_object().expect("object").is_empty());
}

// --- Inspect workflow integration tests for injection config ---

#[tokio::test]
#[serial]
async fn structural_inspect_reports_language_injections() {
    let temp = TempDir::new().expect("workspace tempdir");
    fs::write(
        temp.path().join("sgconfig.yml"),
        "\
ruleDirs:
  - rules
languageInjections:
- hostLanguage: js
  injected: css
",
    )
    .expect("write config");

    let result = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "workflow": "inspect"
        }),
    )
    .await
    .expect("inspect should succeed");

    let injections = result["language_injections"]
        .as_array()
        .expect("language_injections array");
    assert_eq!(injections.len(), 1);
    assert_eq!(injections[0]["hostLanguage"], "js");
    assert_eq!(injections[0]["injected"], "css");
}

#[tokio::test]
#[serial]
async fn structural_inspect_reports_custom_languages() {
    let temp = TempDir::new().expect("workspace tempdir");
    fs::write(
        temp.path().join("sgconfig.yml"),
        "ruleDirs:\n  - rules\ncustomLanguages:\n  graphql:\n    libraryPath: graphql.so\n    extensions: [graphql]\n",
    )
    .expect("write config");

    let result = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "workflow": "inspect"
        }),
    )
    .await
    .expect("inspect should succeed");

    let custom = &result["custom_languages"];
    assert_eq!(custom["graphql"]["libraryPath"], "graphql.so");
}

#[tokio::test]
#[serial]
async fn structural_inspect_reports_language_globs() {
    let temp = TempDir::new().expect("workspace tempdir");
    fs::write(
        temp.path().join("sgconfig.yml"),
        "ruleDirs:\n  - rules\nlanguageGlobs:\n  tsx:\n    - \"*.tsx\"\n",
    )
    .expect("write config");

    let result = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "workflow": "inspect"
        }),
    )
    .await
    .expect("inspect should succeed");

    let globs = &result["language_globs"];
    let tsx = globs["tsx"].as_array().expect("tsx array");
    assert_eq!(tsx.len(), 1);
    assert_eq!(tsx[0], "*.tsx");
}

#[tokio::test]
#[serial]
async fn structural_inspect_reports_empty_injection_config_when_missing() {
    let temp = TempDir::new().expect("workspace tempdir");

    let result = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "workflow": "inspect"
        }),
    )
    .await
    .expect("inspect should succeed");

    assert!(
        result["language_injections"]
            .as_array()
            .expect("array")
            .is_empty()
    );
    assert!(
        result["custom_languages"]
            .as_object()
            .expect("object")
            .is_empty()
    );
    assert!(
        result["language_globs"]
            .as_object()
            .expect("object")
            .is_empty()
    );
}

#[tokio::test]
#[serial]
async fn structural_inspect_reports_all_config_sections_together() {
    let temp = TempDir::new().expect("workspace tempdir");
    fs::write(
        temp.path().join("sgconfig.yml"),
        "\
ruleDirs:
  - rules
  - custom-rules
customLanguages:
  graphql:
    libraryPath: graphql.so
    extensions: [graphql]
languageGlobs:
  tsx:
    - \"*.tsx\"
languageInjections:
- hostLanguage: js
  injected: graphql
",
    )
    .expect("write config");

    let result = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "workflow": "inspect"
        }),
    )
    .await
    .expect("inspect should succeed");

    assert_eq!(result["config_exists"], true);

    let rules = result["rule_dir_hints"].as_array().expect("rule_dir_hints");
    assert_eq!(rules.len(), 2);
    assert_eq!(rules[0], "rules");
    assert_eq!(rules[1], "custom-rules");

    let custom = &result["custom_languages"];
    assert_eq!(custom["graphql"]["libraryPath"], "graphql.so");

    let globs = &result["language_globs"];
    assert_eq!(globs["tsx"].as_array().expect("array").len(), 1);

    let injections = result["language_injections"].as_array().expect("array");
    assert_eq!(injections.len(), 1);
    assert_eq!(injections[0]["hostLanguage"], "js");
    assert_eq!(injections[0]["injected"], "graphql");
}
