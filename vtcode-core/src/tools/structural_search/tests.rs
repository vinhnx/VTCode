use super::{
    AstGrepByteOffset, AstGrepLabel, AstGrepMatch, AstGrepMetaVar, AstGrepMetaVariables,
    AstGrepPoint, AstGrepRange, AstGrepRewriteMatch, AstGrepScanFinding, AstGrepSeverity,
    FixConfig, FixExpandRule, StructuralSearchRequest, StructuralWorkflow, build_atomic_rule_yaml,
    build_fixconfig_rule_yaml, build_query_result, build_scan_result, build_scan_summary,
    execute_structural_search, extract_custom_languages, extract_language_globs,
    extract_language_injections, extract_rule_summary, format_ast_grep_failure, normalize_match,
    normalize_rewrite_match, normalize_scan_finding, parse_compact_matches,
    parse_test_failure_details, parse_test_rule_results, preflight_parseable_pattern,
    sanitize_pattern_for_tree_sitter, yaml_escape_scalar,
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

    assert!(err.to_string().contains("requires a non-empty"));
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
fn structural_request_accepts_esquery_descendant_kind() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "kind": "function_declaration identifier",
        "lang": "javascript"
    }))
    .expect("ESQuery descendant kind should be valid");

    assert_eq!(request.kind(), Some("function_declaration identifier"));
}

#[test]
fn structural_request_accepts_esquery_adjacent_sibling_kind() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "kind": "decorator + method_definition",
        "lang": "typescript"
    }))
    .expect("ESQuery adjacent sibling kind should be valid");

    assert_eq!(request.kind(), Some("decorator + method_definition"));
}

#[test]
fn structural_request_accepts_esquery_following_sibling_kind() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "kind": "decorator ~ method_definition",
        "lang": "typescript"
    }))
    .expect("ESQuery following sibling kind should be valid");

    assert_eq!(request.kind(), Some("decorator ~ method_definition"));
}

#[test]
fn structural_request_accepts_esquery_comma_kind() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "kind": "identifier, number",
        "lang": "javascript"
    }))
    .expect("ESQuery comma kind should be valid");

    assert_eq!(request.kind(), Some("identifier, number"));
}

#[test]
fn structural_request_accepts_esquery_has_pseudo_class() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "kind": "function_declaration:has(return_statement)",
        "lang": "javascript"
    }))
    .expect("ESQuery :has pseudo-class should be valid");

    assert_eq!(
        request.kind(),
        Some("function_declaration:has(return_statement)")
    );
}

#[test]
fn structural_request_accepts_esquery_has_with_direct_child() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "kind": "expression_statement:has(> call_expression)",
        "lang": "javascript"
    }))
    .expect("ESQuery :has with direct child should be valid");

    assert_eq!(
        request.kind(),
        Some("expression_statement:has(> call_expression)")
    );
}

#[test]
fn structural_request_accepts_esquery_not_pseudo_class() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "kind": "identifier:not(number)",
        "lang": "javascript"
    }))
    .expect("ESQuery :not pseudo-class should be valid");

    assert_eq!(request.kind(), Some("identifier:not(number)"));
}

#[test]
fn structural_request_accepts_esquery_is_pseudo_class() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "kind": ":is(identifier, number)",
        "lang": "javascript"
    }))
    .expect("ESQuery :is pseudo-class should be valid");

    assert_eq!(request.kind(), Some(":is(identifier, number)"));
}

#[test]
fn structural_request_accepts_esquery_nth_child_pseudo_class() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "kind": "array > number:nth-child(2n+1)",
        "lang": "javascript"
    }))
    .expect("ESQuery :nth-child pseudo-class should be valid");

    assert_eq!(request.kind(), Some("array > number:nth-child(2n+1)"));
}

#[test]
fn structural_request_accepts_esquery_nth_child_with_of_syntax() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "kind": "array > :nth-child(1 of number)",
        "lang": "javascript"
    }))
    .expect("ESQuery :nth-child with of syntax should be valid");

    assert_eq!(request.kind(), Some("array > :nth-child(1 of number)"));
}

#[test]
fn structural_request_accepts_esquery_nth_last_child_pseudo_class() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "kind": "array > number:nth-last-child(1)",
        "lang": "javascript"
    }))
    .expect("ESQuery :nth-last-child pseudo-class should be valid");

    assert_eq!(request.kind(), Some("array > number:nth-last-child(1)"));
}

#[test]
fn structural_request_accepts_esquery_compound_pseudo_classes() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "kind": "function_declaration:has(return_statement):not(generator_function)",
        "lang": "javascript"
    }))
    .expect("ESQuery compound pseudo-classes should be valid");

    assert_eq!(
        request.kind(),
        Some("function_declaration:has(return_statement):not(generator_function)")
    );
}

#[test]
fn structural_request_accepts_esquery_is_with_relationship() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "kind": "call_expression > :is(identifier, number)",
        "lang": "javascript"
    }))
    .expect("ESQuery :is with relationship should be valid");

    assert_eq!(
        request.kind(),
        Some("call_expression > :is(identifier, number)")
    );
}

#[test]
fn structural_request_accepts_esquery_kind_with_pattern() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "fn $NAME() {}",
        "kind": "function_item:has(return_statement)",
        "lang": "rust"
    }))
    .expect("ESQuery kind combined with pattern should be valid");

    assert_eq!(request.kind(), Some("function_item:has(return_statement)"));
    assert_eq!(request.pattern(), Some("fn $NAME() {}"));
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
fn structural_request_exclude_array_converts_to_negative_globs() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "fn $NAME() {}",
        "lang": "rust",
        "exclude": ["*.md", "tests/**"]
    }))
    .expect("exclude array should be accepted");

    let globs = request.normalized_globs();
    assert!(
        globs.contains(&"!*.md".to_string()),
        "expected negative glob for *.md, got {globs:?}"
    );
    assert!(
        globs.contains(&"!tests/**".to_string()),
        "expected negative glob for tests/**, got {globs:?}"
    );
}

#[test]
fn structural_request_exclude_single_string_converts_to_negative_glob() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "fn $NAME() {}",
        "lang": "rust",
        "exclude": "target/**"
    }))
    .expect("exclude single string should be accepted");

    let globs = request.normalized_globs();
    assert!(
        globs.contains(&"!target/**".to_string()),
        "expected negative glob for target/**, got {globs:?}"
    );
}

#[test]
fn structural_request_exclude_with_leading_bang_deduplicates() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "fn $NAME() {}",
        "lang": "rust",
        "exclude": ["!*.md"]
    }))
    .expect("exclude with leading bang should be accepted");

    let globs = request.normalized_globs();
    // Should produce exactly one `!*.md`, not `!!*.md`.
    let neg_count = globs.iter().filter(|g| g.as_str() == "!*.md").count();
    assert_eq!(neg_count, 1, "expected exactly one !*.md, got {globs:?}");
}

#[test]
fn structural_request_exclude_merges_with_positive_globs() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "fn $NAME() {}",
        "lang": "rust",
        "globs": ["**/*.rs"],
        "exclude": ["*.md"]
    }))
    .expect("exclude and globs should merge");

    let globs = request.normalized_globs();
    assert!(
        globs.contains(&"**/*.rs".to_string()),
        "expected positive glob, got {globs:?}"
    );
    assert!(
        globs.contains(&"!*.md".to_string()),
        "expected negative glob, got {globs:?}"
    );
}

#[test]
fn structural_request_exclude_skips_empty_entries() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "fn $NAME() {}",
        "lang": "rust",
        "exclude": ["", "  ", "*.md"]
    }))
    .expect("empty exclude entries should be skipped");

    let globs = request.normalized_globs();
    assert!(
        !globs.iter().any(|g| g.trim().is_empty()),
        "expected no empty globs, got {globs:?}"
    );
    assert!(
        globs.contains(&"!*.md".to_string()),
        "expected !*.md, got {globs:?}"
    );
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
fn structural_request_accepts_no_ignore_flag() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "fn $NAME() {}",
        "no-ignore": ["hidden", "dot"]
    }))
    .expect("no_ignore should be accepted");

    assert_eq!(
        request.no_ignore.as_ref().unwrap(),
        &vec!["hidden".to_string(), "dot".to_string()]
    );
}

#[test]
fn structural_request_accepts_format_for_scan() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "scan",
        "format": "sarif"
    }))
    .expect("format should be accepted for scan");

    assert_eq!(request.format.as_deref(), Some("sarif"));
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
fn structural_request_accepts_snapshot_dir_for_test() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "test",
        "snapshot_dir": "__snapshots__"
    }))
    .expect("test should accept snapshot_dir");

    assert_eq!(request.snapshot_dir.as_deref(), Some("__snapshots__"));
}

#[test]
fn structural_request_accepts_include_off_for_test() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "test",
        "include_off": true
    }))
    .expect("test should accept include_off");

    assert_eq!(request.include_off, Some(true));
}

#[test]
fn structural_request_accepts_test_dir_for_test() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "test",
        "test_dir": "custom-tests"
    }))
    .expect("test should accept test_dir");

    assert_eq!(request.test_dir.as_deref(), Some("custom-tests"));
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
fn structural_pattern_preflight_accepts_multi_metavariable_patterns() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "fn $NAME($$$ARGS) {}",
        "lang": "rust"
    }))
    .expect("valid request");

    assert!(
        preflight_parseable_pattern(&request)
            .expect("multi-metavariable pattern should preflight")
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

#[test]
fn looks_like_rust_method_call_fragment_detects_bare_method_calls() {
    assert!(super::looks_like_rust_method_call_fragment(
        "unwrap_or($T::default())"
    ));
    assert!(super::looks_like_rust_method_call_fragment("map_err($E)"));
    assert!(super::looks_like_rust_method_call_fragment("and_then($C)"));
    assert!(super::looks_like_rust_method_call_fragment("unwrap_or($T)"));
    // Has receiver — not a fragment.
    assert!(!super::looks_like_rust_method_call_fragment(
        "$X.unwrap_or($T)"
    ));
    // Associated function — not a bare method call.
    assert!(!super::looks_like_rust_method_call_fragment(
        "Type::method($A)"
    ));
    // Function call, not method call.
    assert!(!super::looks_like_rust_method_call_fragment("foo()"));
    // Not a call at all.
    assert!(!super::looks_like_rust_method_call_fragment("Result<$T>"));
    // Empty callee.
    assert!(!super::looks_like_rust_method_call_fragment("($T)"));
}

#[test]
fn structural_pattern_preflight_guides_rust_method_call_fragments() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "unwrap_or($T::default())",
        "lang": "rust"
    }))
    .expect("valid request");

    let hint = preflight_parseable_pattern(&request)
        .expect("fragment hint should be returned")
        .expect("expected guidance");
    assert!(hint.contains("method calls"), "{hint}");
    assert!(hint.contains("$X.unwrap_or"), "{hint}");
    assert!(
        hint.contains("Do not retry the same fragment with grep"),
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
async fn structural_search_passes_selector_for_c_function_call_pattern() {
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
            "pattern": "$M($$$);",
            "lang": "c",
            "selector": "call_expression",
            "path": "."
        }),
    )
    .await
    .expect("C selector search should run");

    assert_eq!(result["matches"], json!([]));
    let args = fs::read_to_string(args_path).expect("read sg args");
    assert!(
        args.lines().any(|line| line == "--lang"),
        "--lang flag missing: {args}"
    );
    assert!(
        args.lines().any(|line| line == "c"),
        "lang value missing: {args}"
    );
    assert!(
        args.lines().any(|line| line == "--selector"),
        "--selector flag missing: {args}"
    );
    assert!(
        args.lines().any(|line| line == "call_expression"),
        "selector value missing: {args}"
    );
    assert!(
        args.lines().any(|line| line == "--pattern=$M($$$);"),
        "pattern flag missing: {args}"
    );
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
async fn structural_search_passes_esquery_pseudo_selectors() {
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
            "kind": "function_declaration:has(return_statement):not(generator_function)",
            "lang": "javascript",
            "path": "."
        }),
    )
    .await
    .expect("ESQuery pseudo-selectors should run");

    assert_eq!(
        result["kind"],
        json!("function_declaration:has(return_statement):not(generator_function)")
    );
    let args = fs::read_to_string(args_path).expect("read sg args");
    assert!(args.lines().any(|line| line == "--kind"));
    assert!(
        args.lines().any(
            |line| line == "function_declaration:has(return_statement):not(generator_function)"
        )
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

    // "tree-sitter dynamic library" is unique to AST_GREP_PROJECT_CONFIG_HINT
    // and should NOT appear for generic parse errors (only for language support
    // issues). Strings like "customLanguages" or "languageInjections" may
    // appear in AST_GREP_FAQ_HINT and are not reliable discriminators.
    assert!(!text.contains("tree-sitter dynamic library"));
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

    // Verify per-rule results are parsed.
    let rules = result["summary"]["rules"].as_array().expect("rules array");
    assert_eq!(rules.len(), 2);
    assert_eq!(rules[0]["rule_id"], "rust/no-iterator-for-each");
    assert_eq!(rules[0]["passed"], true);
    assert_eq!(rules[1]["rule_id"], "rust/for-each-snapshot");
    assert_eq!(rules[1]["passed"], false);

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
        severity: Some(AstGrepSeverity::Error),
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
        severity: Some(AstGrepSeverity::Error),
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
        severity: Some(AstGrepSeverity::Error),
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
        severity: Some(AstGrepSeverity::Error),
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
        severity: Some(AstGrepSeverity::Error),
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
        severity: Some(AstGrepSeverity::Warning),
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
    let (_script_dir, script_path) = write_fake_sg("#!/bin/sh\nprintf ''\n");
    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));

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
    let (_script_dir, script_path) = write_fake_sg("#!/bin/sh\nprintf ''\n");
    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));

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
    let (_script_dir, script_path) = write_fake_sg("#!/bin/sh\nprintf ''\n");
    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));

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
    let (_script_dir, script_path) = write_fake_sg("#!/bin/sh\nprintf ''\n");
    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));

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
    assert!(result["test_configs"].as_array().expect("array").is_empty());
    assert!(result["util_dirs"].as_array().expect("array").is_empty());
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
utilDirs:
  - utils
  - shared
testConfigs:
  - testDir: rule-tests
    snapshotDir: __snapshots__
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

    let util_dirs = result["util_dirs"].as_array().expect("util_dirs");
    assert_eq!(util_dirs.len(), 2);
    assert_eq!(util_dirs[0], "utils");
    assert_eq!(util_dirs[1], "shared");

    let test_configs = result["test_configs"].as_array().expect("test_configs");
    assert_eq!(test_configs.len(), 1);
    assert_eq!(test_configs[0]["testDir"], "rule-tests");
    assert_eq!(test_configs[0]["snapshotDir"], "__snapshots__");

    let custom = &result["custom_languages"];
    assert_eq!(custom["graphql"]["libraryPath"], "graphql.so");

    let globs = &result["language_globs"];
    assert_eq!(globs["tsx"].as_array().expect("array").len(), 1);

    let injections = result["language_injections"].as_array().expect("array");
    assert_eq!(injections.len(), 1);
    assert_eq!(injections[0]["hostLanguage"], "js");
    assert_eq!(injections[0]["injected"], "graphql");
}

#[tokio::test]
#[serial]
async fn structural_inspect_reports_test_configs() {
    let temp = TempDir::new().expect("workspace tempdir");
    fs::write(
        temp.path().join("sgconfig.yml"),
        "\
ruleDirs:
  - rules
testConfigs:
  - testDir: tests
    snapshotDir: __snapshots__
  - testDir: more-tests
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

    let configs = result["test_configs"].as_array().expect("test_configs");
    assert_eq!(configs.len(), 2);
    assert_eq!(configs[0]["testDir"], "tests");
    assert_eq!(configs[0]["snapshotDir"], "__snapshots__");
    assert_eq!(configs[1]["testDir"], "more-tests");
    // snapshotDir should be absent when not specified
    assert!(configs[1].get("snapshotDir").is_none());
}

#[tokio::test]
#[serial]
async fn structural_inspect_reports_util_dirs() {
    let temp = TempDir::new().expect("workspace tempdir");
    fs::write(
        temp.path().join("sgconfig.yml"),
        "ruleDirs:\n  - rules\nutilDirs:\n  - utils\n  - shared-rules\n",
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

    let dirs = result["util_dirs"].as_array().expect("util_dirs");
    assert_eq!(dirs.len(), 2);
    assert_eq!(dirs[0], "utils");
    assert_eq!(dirs[1], "shared-rules");
}

#[tokio::test]
#[serial]
async fn structural_inspect_reports_util_dirs_inline() {
    let temp = TempDir::new().expect("workspace tempdir");
    fs::write(
        temp.path().join("sgconfig.yml"),
        "ruleDirs: [rules]\nutilDirs: [utils]\n",
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

    let dirs = result["util_dirs"].as_array().expect("util_dirs");
    assert_eq!(dirs.len(), 1);
    assert_eq!(dirs[0], "utils");
}

// --------------- rewrite workflow tests ---------------

#[test]
fn structural_request_rejects_config_path_for_rewrite() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "rewrite",
        "pattern": "fn $NAME() {}",
        "rewrite": "fn renamed() {}",
        "config_path": "sgconfig.yml"
    }))
    .expect_err("config_path should be rejected for rewrite workflow");

    assert!(err.to_string().contains("does not accept `config_path`"));
}

#[test]
fn structural_request_rejects_filter_for_rewrite() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "rewrite",
        "pattern": "fn $NAME() {}",
        "rewrite": "fn renamed() {}",
        "filter": "some-rule"
    }))
    .expect_err("filter should be rejected for rewrite workflow");

    assert!(err.to_string().contains("does not accept `filter`"));
}

#[test]
fn normalize_rewrite_match_emits_replacement() {
    let entry = AstGrepRewriteMatch {
        file: "src/lib.rs".to_string(),
        text: "fn old() {}".to_string(),
        lines: Some("5: fn old() {}".to_string()),
        language: Some("Rust".to_string()),
        range: AstGrepRange {
            start: AstGrepPoint { line: 5, column: 0 },
            end: AstGrepPoint {
                line: 5,
                column: 11,
            },
            byte_offset: None,
        },
        meta_variables: None,
        replacement: Some("fn renamed() {}".to_string()),
        replacement_offsets: Some(AstGrepByteOffset { start: 0, end: 11 }),
    };

    let value = normalize_rewrite_match(entry);

    assert_eq!(value["file"], "src/lib.rs");
    assert_eq!(value["line_number"], 5);
    assert_eq!(value["text"], "fn old() {}");
    assert_eq!(value["lines"], "5: fn old() {}");
    assert_eq!(value["language"], "Rust");
    assert_eq!(value["replacement"], "fn renamed() {}");
    assert_eq!(value["replacementOffsets"]["start"], 0);
    assert_eq!(value["replacementOffsets"]["end"], 11);
}

#[test]
fn normalize_rewrite_match_omits_replacement_when_absent() {
    let entry = AstGrepRewriteMatch {
        file: "src/lib.rs".to_string(),
        text: "fn old() {}".to_string(),
        lines: None,
        language: None,
        range: AstGrepRange {
            start: AstGrepPoint { line: 3, column: 0 },
            end: AstGrepPoint {
                line: 3,
                column: 11,
            },
            byte_offset: None,
        },
        meta_variables: None,
        replacement: None,
        replacement_offsets: None,
    };

    let value = normalize_rewrite_match(entry);

    assert_eq!(value["file"], "src/lib.rs");
    assert_eq!(value["line_number"], 3);
    assert_eq!(value["text"], "fn old() {}");
    assert!(value.get("replacement").is_none());
    assert!(value.get("replacementOffsets").is_none());
    assert!(value.get("language").is_none());
    // lines falls back to text when absent
    assert_eq!(value["lines"], "fn old() {}");
}

#[test]
fn normalize_rewrite_match_emits_meta_variables() {
    let mut single = std::collections::BTreeMap::new();
    single.insert(
        "NAME".to_string(),
        AstGrepMetaVar {
            text: "old".to_string(),
            range: AstGrepRange {
                start: AstGrepPoint { line: 5, column: 3 },
                end: AstGrepPoint { line: 5, column: 6 },
                byte_offset: None,
            },
        },
    );
    let mut multi = std::collections::BTreeMap::new();
    multi.insert(
        "ARGS".to_string(),
        vec![AstGrepMetaVar {
            text: "x".to_string(),
            range: AstGrepRange {
                start: AstGrepPoint { line: 5, column: 7 },
                end: AstGrepPoint { line: 5, column: 8 },
                byte_offset: None,
            },
        }],
    );
    let mut transformed = std::collections::BTreeMap::new();
    transformed.insert("UPPER".to_string(), "OLD".to_string());

    let entry = AstGrepRewriteMatch {
        file: "src/lib.rs".to_string(),
        text: "fn old(x) {}".to_string(),
        lines: None,
        language: None,
        range: AstGrepRange {
            start: AstGrepPoint { line: 5, column: 0 },
            end: AstGrepPoint {
                line: 5,
                column: 12,
            },
            byte_offset: None,
        },
        meta_variables: Some(AstGrepMetaVariables {
            single: single.clone(),
            multi: multi.clone(),
            transformed: transformed.clone(),
        }),
        replacement: Some("fn renamed(x) {}".to_string()),
        replacement_offsets: None,
    };

    let value = normalize_rewrite_match(entry);

    assert_eq!(value["metaVariables"]["single"]["NAME"]["text"], "old");
    assert_eq!(
        value["metaVariables"]["single"]["NAME"]["range"]["start"]["line"],
        5
    );
    assert_eq!(value["metaVariables"]["multi"]["ARGS"][0]["text"], "x");
    assert_eq!(value["metaVariables"]["transformed"]["UPPER"], "OLD");
}

#[tokio::test]
#[serial]
async fn structural_rewrite_passes_rewrite_flag_to_ast_grep() {
    let temp = TempDir::new().expect("workspace tempdir");
    let src_dir = temp.path().join("src");
    fs::create_dir_all(&src_dir).expect("create src dir");
    fs::write(src_dir.join("lib.rs"), "fn alpha() {}\n").expect("write rust file");

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
            "workflow": "rewrite",
            "pattern": "fn $NAME() {}",
            "rewrite": "fn renamed() {}",
            "lang": "rust",
            "path": "src"
        }),
    )
    .await
    .expect("rewrite should succeed");

    assert_eq!(result["workflow"], "rewrite");
    assert_eq!(result["rewrites"], json!([]));

    let args = fs::read_to_string(args_path).expect("read sg args");
    assert!(
        args.lines().any(|line| line == "run"),
        "subcommand should be run"
    );
    assert!(
        args.lines().any(|line| line == "--pattern=fn $NAME() {}"),
        "pattern flag should be passed"
    );
    assert!(
        args.lines().any(|line| line == "--rewrite=fn renamed() {}"),
        "rewrite flag should be passed"
    );
    assert!(args.lines().any(|line| line == "--json=compact"));
    assert!(args.lines().any(|line| line == "--color=never"));
    assert!(args.lines().any(|line| line == "--lang"));
    assert!(args.lines().any(|line| line == "rust"));
}

#[tokio::test]
#[serial]
async fn structural_rewrite_normalizes_replacement_fields() {
    let temp = TempDir::new().expect("workspace tempdir");
    let src_dir = temp.path().join("src");
    fs::create_dir_all(&src_dir).expect("create src dir");
    fs::write(src_dir.join("lib.rs"), "fn alpha() {}\n").expect("write rust file");

    let replacement_json = r#"{"text":"fn alpha() {}","range":{"start":{"line":5,"column":0},"end":{"line":5,"column":14}},"file":"src/lib.rs","lines":"5: fn alpha() {}","language":"Rust","replacement":"fn renamed() {}","replacementOffsets":{"start":0,"end":14}}"#;
    let script = format!("#!/bin/sh\nprintf '[{}]\n'\n", replacement_json);
    let (_script_dir, script_path) = write_fake_sg(&script);

    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let result = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "workflow": "rewrite",
            "pattern": "fn $NAME() {}",
            "rewrite": "fn renamed() {}",
            "lang": "rust",
            "path": "src"
        }),
    )
    .await
    .expect("rewrite should succeed");

    let rewrites = result["rewrites"].as_array().expect("rewrites");
    assert_eq!(rewrites.len(), 1);
    assert_eq!(rewrites[0]["file"], "src/lib.rs");
    assert_eq!(rewrites[0]["line_number"], 5);
    assert_eq!(rewrites[0]["text"], "fn alpha() {}");
    assert_eq!(rewrites[0]["replacement"], "fn renamed() {}");
    assert_eq!(rewrites[0]["replacementOffsets"]["start"], 0);
    assert_eq!(rewrites[0]["replacementOffsets"]["end"], 14);
    assert_eq!(rewrites[0]["language"], "Rust");
}

#[tokio::test]
#[serial]
async fn structural_rewrite_treats_exit_code_one_as_no_rewrites() {
    let temp = TempDir::new().expect("workspace tempdir");
    let src_dir = temp.path().join("src");
    fs::create_dir_all(&src_dir).expect("create src dir");
    fs::write(src_dir.join("lib.rs"), "fn beta() {}\n").expect("write rust file");

    let script = "#!/bin/sh\nexit 1\n";
    let (_script_dir, script_path) = write_fake_sg(script);

    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let result = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "workflow": "rewrite",
            "pattern": "fn $NAME() {}",
            "rewrite": "fn renamed() {}",
            "lang": "rust",
            "path": "src"
        }),
    )
    .await
    .expect("exit code 1 should be treated as no rewrites");

    assert_eq!(result["workflow"], "rewrite");
    assert_eq!(result["rewrites"], json!([]));
    assert_eq!(result["truncated"], false);
}

#[tokio::test]
#[serial]
async fn structural_rewrite_passes_selector_and_strictness() {
    let temp = TempDir::new().expect("workspace tempdir");
    let src_dir = temp.path().join("src");
    fs::create_dir_all(&src_dir).expect("create src dir");
    fs::write(src_dir.join("lib.rs"), "fn alpha() {}\n").expect("write rust file");

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
            "workflow": "rewrite",
            "pattern": "fn $NAME() {}",
            "rewrite": "fn renamed() {}",
            "lang": "rust",
            "path": "src",
            "selector": "function_item",
            "strictness": "smart"
        }),
    )
    .await
    .expect("rewrite with selector/strictness should succeed");

    assert_eq!(result["workflow"], "rewrite");

    let args = fs::read_to_string(args_path).expect("read sg args");
    assert!(args.lines().any(|line| line == "--selector"));
    assert!(args.lines().any(|line| line == "function_item"));
    assert!(args.lines().any(|line| line == "--strictness"));
    assert!(args.lines().any(|line| line == "smart"));
}

// --------------- FixConfig rewrite tests ---------------

#[test]
fn fixconfig_request_requires_rewrite_or_fix_config() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "rewrite",
        "pattern": "fn $NAME() {}",
        "lang": "rust"
    }))
    .expect_err("should reject when neither rewrite nor fix_config is present");

    let msg = err.to_string();
    assert!(
        msg.contains("requires a non-empty `rewrite`") || msg.contains("requires"),
        "unexpected error: {msg}"
    );
}

#[test]
fn fixconfig_request_accepts_template_only() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "rewrite",
        "pattern": "console.log($$$ARGS)",
        "lang": "javascript",
        "fix_config": {
            "template": "logger.log($$$ARGS)"
        }
    }))
    .expect("fix_config with template-only should be accepted");

    let fc = request.fix_config.as_ref().expect("fix_config present");
    assert_eq!(fc.template, "logger.log($$$ARGS)");
    assert!(fc.expand_start.is_none());
    assert!(fc.expand_end.is_none());
    assert!(!fc.has_expansion());
}

#[test]
fn fixconfig_request_accepts_expand_end() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "rewrite",
        "pattern": "$KEY: $VAL",
        "lang": "javascript",
        "fix_config": {
            "template": "",
            "expand_end": {
                "regex": ","
            }
        }
    }))
    .expect("fix_config with expand_end should be accepted");

    let fc = request.fix_config.as_ref().expect("fix_config present");
    assert_eq!(fc.template, "");
    assert!(fc.expand_start.is_none());
    let expand_end = fc.expand_end.as_ref().expect("expand_end present");
    assert_eq!(expand_end.regex.as_deref(), Some(","));
    assert!(expand_end.kind.is_none());
    assert!(expand_end.pattern.is_none());
    assert!(fc.has_expansion());
}

#[test]
fn fixconfig_request_accepts_expand_start_and_end() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "rewrite",
        "pattern": "$ITEM",
        "lang": "javascript",
        "fix_config": {
            "template": "",
            "expand_start": {
                "regex": ",",
                "stop_by": "line"
            },
            "expand_end": {
                "regex": ","
            }
        }
    }))
    .expect("fix_config with both expand_start and expand_end should be accepted");

    let fc = request.fix_config.as_ref().expect("fix_config present");
    assert!(fc.has_expansion());
    let es = fc.expand_start.as_ref().expect("expand_start present");
    assert_eq!(es.regex.as_deref(), Some(","));
    assert_eq!(es.stop_by.as_ref().unwrap(), &json!("line"));
}

#[test]
fn fixconfig_accepts_empty_template_for_delete() {
    // Empty template is valid for "delete" operations (replace matched
    // node with nothing). Whitespace-only templates are also accepted
    // as they could be intentional formatting.
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "rewrite",
        "pattern": "fn $NAME() {}",
        "lang": "rust",
        "fix_config": {
            "template": ""
        }
    }))
    .expect("empty template should be accepted for delete operations");

    let fc = request.fix_config.as_ref().expect("fix_config present");
    assert_eq!(fc.template, "");
}

#[test]
fn fixconfig_rejects_empty_expand_rule() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "rewrite",
        "pattern": "$KEY: $VAL",
        "lang": "javascript",
        "fix_config": {
            "template": "",
            "expand_end": {}
        }
    }))
    .expect_err("empty expand rule should be rejected");

    assert!(err.to_string().contains("expand_end"));
}

#[test]
fn fixconfig_accepts_expand_rule_with_kind() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "rewrite",
        "pattern": "foo($ARG)",
        "lang": "javascript",
        "fix_config": {
            "template": "bar($ARG)",
            "expand_start": { "kind": "(" },
            "expand_end": { "kind": ")" }
        }
    }))
    .expect("expand rules with kind should be accepted");

    let fc = request.fix_config.as_ref().expect("fix_config present");
    assert_eq!(fc.expand_start.as_ref().unwrap().kind.as_deref(), Some("("));
    assert_eq!(fc.expand_end.as_ref().unwrap().kind.as_deref(), Some(")"));
}

#[test]
fn fixconfig_accepts_expand_rule_with_pattern() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "rewrite",
        "pattern": "foo($ARG)",
        "lang": "javascript",
        "fix_config": {
            "template": "bar($ARG)",
            "expand_end": { "pattern": "," }
        }
    }))
    .expect("expand rules with pattern should be accepted");

    let fc = request.fix_config.as_ref().expect("fix_config present");
    assert_eq!(
        fc.expand_end.as_ref().unwrap().pattern.as_deref(),
        Some(",")
    );
}

#[test]
fn fixconfig_effective_rewrite_template_prefers_rewrite_string() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "rewrite",
        "pattern": "fn $NAME() {}",
        "lang": "rust",
        "rewrite": "fn renamed() {}",
        "fix_config": {
            "template": "from_fix_config"
        }
    }))
    .expect("both rewrite and fix_config should be accepted");

    // rewrite string takes precedence
    assert_eq!(
        request.effective_rewrite_template(),
        Some("fn renamed() {}")
    );
}

#[test]
fn fixconfig_effective_rewrite_template_uses_template_when_no_rewrite() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "rewrite",
        "pattern": "fn $NAME() {}",
        "lang": "rust",
        "fix_config": {
            "template": "from_template"
        }
    }))
    .expect("fix_config without rewrite string should be accepted");

    assert_eq!(request.effective_rewrite_template(), Some("from_template"));
}

#[test]
fn build_fixconfig_rule_yaml_generates_correct_structure() {
    let fix_config = FixConfig {
        template: "".to_string(),
        expand_start: None,
        expand_end: Some(FixExpandRule {
            regex: Some(",".to_string()),
            kind: None,
            pattern: None,
            stop_by: None,
        }),
    };

    let yaml = build_fixconfig_rule_yaml("$KEY: $VAL", "javascript", &fix_config, None, None);

    assert!(yaml.contains("id: fixconfig-rewrite"), "yaml: {yaml}");
    assert!(yaml.contains("language: javascript"), "yaml: {yaml}");
    assert!(yaml.contains("severity: info"), "yaml: {yaml}");
    assert!(yaml.contains("pattern: '$KEY: $VAL'"), "yaml: {yaml}");
    assert!(yaml.contains("template: ''"), "yaml: {yaml}");
    assert!(yaml.contains("expandEnd:"), "yaml: {yaml}");
    assert!(yaml.contains("regex: ','"), "yaml: {yaml}");
}

#[test]
fn build_fixconfig_rule_yaml_includes_selector() {
    let fix_config = FixConfig {
        template: "bar($ARG)".to_string(),
        expand_start: Some(FixExpandRule {
            regex: None,
            kind: Some("(".to_string()),
            pattern: None,
            stop_by: None,
        }),
        expand_end: Some(FixExpandRule {
            regex: None,
            kind: Some(")".to_string()),
            pattern: None,
            stop_by: None,
        }),
    };

    let yaml = build_fixconfig_rule_yaml(
        "foo($ARG)",
        "javascript",
        &fix_config,
        Some("call_expression"),
        None,
    );

    assert!(yaml.contains("pattern: foo($ARG)"), "yaml: {yaml}");
    assert!(yaml.contains("selector: call_expression"), "yaml: {yaml}");
    assert!(yaml.contains("expandStart:"), "yaml: {yaml}");
    assert!(yaml.contains("expandEnd:"), "yaml: {yaml}");
    // `(` and `)` are not special YAML characters, so they are not quoted.
    assert!(yaml.contains("kind: ("), "yaml: {yaml}");
    assert!(yaml.contains("kind: )"), "yaml: {yaml}");
}

#[test]
fn build_fixconfig_rule_yaml_escapes_pattern_with_special_chars() {
    let fix_config = FixConfig {
        template: "$VAR".to_string(),
        expand_start: None,
        expand_end: None,
    };

    // Pattern contains `:` which is a YAML special character.
    let yaml = build_fixconfig_rule_yaml("$KEY: $VAL", "javascript", &fix_config, None, None);
    assert!(
        yaml.contains("pattern: '$KEY: $VAL'"),
        "pattern with colon should be quoted: {yaml}"
    );

    // Pattern with `#` (YAML comment character).
    let yaml = build_fixconfig_rule_yaml("$A # $B", "javascript", &fix_config, None, None);
    assert!(
        yaml.contains("pattern: '$A # $B'"),
        "pattern with hash should be quoted: {yaml}"
    );

    // Selector is also escaped.
    let yaml = build_fixconfig_rule_yaml(
        "foo($A)",
        "javascript",
        &fix_config,
        Some("call: expression"),
        None,
    );
    assert!(
        yaml.contains("selector: 'call: expression'"),
        "selector with colon should be quoted: {yaml}"
    );
}

#[test]
fn fixexpand_rule_to_yaml_value_serializes_fields() {
    let rule = FixExpandRule {
        regex: Some(",".to_string()),
        kind: None,
        pattern: None,
        stop_by: Some(json!("line")),
    };

    let value = rule.to_yaml_value();
    assert_eq!(value["regex"], ",");
    assert_eq!(value["stopBy"], "line");
    assert!(value.get("kind").is_none());
    assert!(value.get("pattern").is_none());
}

#[test]
fn yaml_escape_scalar_handles_special_characters() {
    // Simple value, no quoting needed
    assert_eq!(yaml_escape_scalar("hello"), "hello");

    // Empty value
    assert_eq!(yaml_escape_scalar(""), "''");

    // Value with colon needs quoting
    let escaped = yaml_escape_scalar("key: value");
    assert!(escaped.starts_with('\''), "should be quoted: {escaped}");

    // Value with hash needs quoting
    let escaped = yaml_escape_scalar("foo # bar");
    assert!(escaped.starts_with('\''), "should be quoted: {escaped}");

    // Value with single quote escapes by doubling
    let escaped = yaml_escape_scalar("it's");
    assert!(escaped.contains("''"), "should escape quote: {escaped}");
}

#[tokio::test]
#[serial]
async fn fixconfig_rewrite_with_expansion_uses_sg_scan() {
    let temp = TempDir::new().expect("workspace tempdir");
    let src_dir = temp.path().join("src");
    fs::create_dir_all(&src_dir).expect("create src dir");
    fs::write(src_dir.join("lib.js"), "const x = { a: 1, b: 2 }\n").expect("write js file");

    let args_path = temp.path().join("sg_args.txt");
    // Scan uses newline-delimited JSON. Empty results are represented by
    // exit code 1 with empty stdout (same as the query rewrite path).
    let script = format!(
        "#!/bin/sh\nprintf '%s\n' \"$@\" > \"{}\"\nexit 1\n",
        args_path.display()
    );
    let (_script_dir, script_path) = write_fake_sg(&script);

    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let result = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "workflow": "rewrite",
            "pattern": "$KEY: $VAL",
            "lang": "javascript",
            "path": "src",
            "fix_config": {
                "template": "",
                "expand_end": {
                    "regex": ","
                }
            }
        }),
    )
    .await
    .expect("fixconfig rewrite should succeed");

    assert_eq!(result["workflow"], "rewrite");
    assert_eq!(result["rewrites"], json!([]));

    let args = fs::read_to_string(args_path).expect("read sg args");
    // FixConfig with expansion should use scan, not run
    assert!(
        args.lines().any(|line| line == "scan"),
        "subcommand should be scan for fixconfig with expansion, got: {args}"
    );
    assert!(
        args.lines().any(|line| line == "--json=stream"),
        "should use stream output for scan"
    );
    assert!(
        args.lines().any(|line| line == "--config"),
        "should pass --config for scan"
    );
}

#[tokio::test]
#[serial]
async fn fixconfig_rewrite_without_expansion_uses_sg_run_rewrite() {
    let temp = TempDir::new().expect("workspace tempdir");
    let src_dir = temp.path().join("src");
    fs::create_dir_all(&src_dir).expect("create src dir");
    fs::write(src_dir.join("lib.js"), "console.log('hi')\n").expect("write js file");

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
            "workflow": "rewrite",
            "pattern": "console.log($$$ARGS)",
            "lang": "javascript",
            "path": "src",
            "fix_config": {
                "template": "logger.log($$$ARGS)"
            }
        }),
    )
    .await
    .expect("fixconfig template-only rewrite should succeed");

    assert_eq!(result["workflow"], "rewrite");

    let args = fs::read_to_string(args_path).expect("read sg args");
    // Template-only FixConfig (no expansion) should use run --rewrite
    assert!(
        args.lines().any(|line| line == "run"),
        "subcommand should be run for template-only fixconfig, got: {args}"
    );
    assert!(
        args.lines()
            .any(|line| line == "--rewrite=logger.log($$$ARGS)"),
        "should pass --rewrite with template"
    );
}

// --------------- Go-specific tests ---------------

#[test]
fn structural_request_accepts_go_contextual_selector() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "fmt.Println($A)",
        "lang": "go",
        "selector": "call_expression"
    }))
    .expect("Go pattern with call_expression selector should be valid");

    assert_eq!(request.pattern(), Some("fmt.Println($A)"));
    assert_eq!(request.selector.as_deref(), Some("call_expression"));
}

#[test]
fn structural_request_accepts_go_defer_kind() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "kind": "defer_statement",
        "lang": "go"
    }))
    .expect("Go defer_statement kind should be valid");

    assert_eq!(request.kind(), Some("defer_statement"));
}

#[test]
fn structural_request_accepts_go_import_spec_kind_with_regex() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "kind": "import_spec",
        "lang": "go"
    }))
    .expect("Go import_spec kind should be valid");

    assert_eq!(request.kind(), Some("import_spec"));
}

#[test]
fn looks_like_html_attribute_pattern_detects_attribute_fragments() {
    assert!(super::looks_like_html_attribute_pattern("class=$VAL"));
    assert!(super::looks_like_html_attribute_pattern("id=$ID"));
    assert!(super::looks_like_html_attribute_pattern("href=$URL"));
    assert!(super::looks_like_html_attribute_pattern("data-testid=$VAL"));
    assert!(super::looks_like_html_attribute_pattern("xml:lang=$VAL"));
    assert!(!super::looks_like_html_attribute_pattern(
        "<div class=$VAL>"
    ));
    assert!(!super::looks_like_html_attribute_pattern("class"));
    assert!(!super::looks_like_html_attribute_pattern("=value"));
}

#[test]
fn looks_like_html_tag_pattern_detects_opening_tag_fragments() {
    assert!(super::looks_like_html_tag_pattern("<$TAG>"));
    assert!(super::looks_like_html_tag_pattern("<div>"));
    assert!(super::looks_like_html_tag_pattern("<$TAG $$$ATTRS>"));
    assert!(!super::looks_like_html_tag_pattern("<div></div>"));
    assert!(!super::looks_like_html_tag_pattern("<br/>"));
    assert!(!super::looks_like_html_tag_pattern("class=$VAL"));
}

#[test]
fn html_fragment_hint_for_attribute_pattern_mentions_kinds() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "class=$VAL",
        "lang": "html"
    }))
    .expect("valid request");

    let hint = super::fragment_pattern_hint(&request, super::AstGrepLanguage::Html);
    assert!(
        hint.contains("attribute_name"),
        "should mention attribute_name kind: {hint}"
    );
    assert!(
        hint.contains("attribute_value"),
        "should mention attribute_value kind: {hint}"
    );
}

#[test]
fn html_fragment_hint_for_tag_pattern_mentions_element() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "<$TAG>",
        "lang": "html"
    }))
    .expect("valid request");

    let hint = super::fragment_pattern_hint(&request, super::AstGrepLanguage::Html);
    assert!(hint.contains("tag_name"), "should mention tag_name: {hint}");
    assert!(
        hint.contains("element"),
        "should mention element kind: {hint}"
    );
}

#[test]
fn looks_like_java_declaration_fragment_detects_bare_annotations() {
    assert!(super::looks_like_java_declaration_fragment("@Override"));
    assert!(super::looks_like_java_declaration_fragment("@Nullable"));
    assert!(super::looks_like_java_declaration_fragment("@Bean($$$)"));
    assert!(!super::looks_like_java_declaration_fragment("class Foo {}"));
    assert!(!super::looks_like_java_declaration_fragment("foo()"));
}

#[test]
fn looks_like_java_declaration_fragment_detects_type_declaration_fragments() {
    assert!(super::looks_like_java_declaration_fragment("String $F;"));
    assert!(super::looks_like_java_declaration_fragment(
        "private String $NAME;"
    ));
    assert!(super::looks_like_java_declaration_fragment("int $X;"));
    assert!(!super::looks_like_java_declaration_fragment("String"));
    assert!(!super::looks_like_java_declaration_fragment("$F"));
}

#[test]
fn java_fragment_hint_for_annotation_mentions_kinds() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "@Override",
        "lang": "java"
    }))
    .expect("valid request");

    let hint = super::fragment_pattern_hint(&request, super::AstGrepLanguage::Java);
    assert!(
        hint.contains("marker_annotation") || hint.contains("annotation"),
        "should mention annotation kind: {hint}"
    );
    assert!(
        hint.contains("field_declaration"),
        "should mention field_declaration: {hint}"
    );
}

#[test]
fn java_fragment_hint_for_type_declaration_mentions_field_type_pattern() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "String $F;",
        "lang": "java"
    }))
    .expect("valid request");

    let hint = super::fragment_pattern_hint(&request, super::AstGrepLanguage::Java);
    assert!(
        hint.contains("field: type"),
        "should mention field: type pattern: {hint}"
    );
    assert!(
        hint.contains("field_declaration"),
        "should mention field_declaration: {hint}"
    );
}

// --------------- FixConfig stopBy object tests ---------------

#[test]
fn fixconfig_accepts_expand_rule_with_stop_by_object() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "rewrite",
        "pattern": "$ITEM",
        "lang": "javascript",
        "fix_config": {
            "template": "",
            "expand_start": {
                "regex": ",",
                "stop_by": { "kind": "," }
            },
            "expand_end": {
                "regex": ",",
                "stop_by": { "regex": "," }
            }
        }
    }))
    .expect("fix_config with object stop_by should be accepted");

    let fc = request.fix_config.as_ref().expect("fix_config present");
    let es = fc.expand_start.as_ref().expect("expand_start present");
    assert_eq!(es.regex.as_deref(), Some(","));
    let stop_by = es.stop_by.as_ref().expect("stop_by present");
    assert_eq!(stop_by["kind"], ",");

    let ee = fc.expand_end.as_ref().expect("expand_end present");
    let stop_by_end = ee.stop_by.as_ref().expect("stop_by present");
    assert_eq!(stop_by_end["regex"], ",");
}

#[test]
fn build_fixconfig_rule_yaml_renders_stop_by_object() {
    let fix_config = FixConfig {
        template: "".to_string(),
        expand_start: None,
        expand_end: Some(FixExpandRule {
            regex: Some(",".to_string()),
            kind: None,
            pattern: None,
            stop_by: Some(json!({ "kind": "," })),
        }),
    };
    let yaml = build_fixconfig_rule_yaml("$ITEM", "javascript", &fix_config, None, None);

    assert!(yaml.contains("expandEnd:"), "yaml: {yaml}");
    assert!(yaml.contains("regex: ','"), "yaml: {yaml}");
    assert!(yaml.contains("stopBy:"), "yaml: {yaml}");
    assert!(yaml.contains("kind: ','"), "yaml: {yaml}");
}

#[test]
fn build_fixconfig_rule_yaml_renders_stop_by_string() {
    let fix_config = FixConfig {
        template: "".to_string(),
        expand_start: Some(FixExpandRule {
            regex: Some(",".to_string()),
            kind: None,
            pattern: None,
            stop_by: Some(json!("line")),
        }),
        expand_end: None,
    };
    let yaml = build_fixconfig_rule_yaml("$ITEM", "javascript", &fix_config, None, None);

    assert!(yaml.contains("expandStart:"), "yaml: {yaml}");
    assert!(yaml.contains("stopBy: line"), "yaml: {yaml}");
}

// --------------- Ruby fragment detection tests ---------------

#[test]
fn looks_like_ruby_block_fragment_detects_pipe_blocks() {
    assert!(super::looks_like_ruby_block_fragment("{ |$V| $V.$METHOD }"));
    assert!(super::looks_like_ruby_block_fragment(
        "do |$V| $V.$METHOD end"
    ));
    assert!(super::looks_like_ruby_block_fragment("| $V | $V.$METHOD"));
    assert!(!super::looks_like_ruby_block_fragment("def foo; end"));
    assert!(!super::looks_like_ruby_block_fragment("$OBJ.method"));
}

#[test]
fn looks_like_ruby_block_fragment_detects_symbol_to_proc() {
    assert!(super::looks_like_ruby_block_fragment("&:$METHOD"));
    assert!(super::looks_like_ruby_block_fragment("&:to_s"));
    assert!(!super::looks_like_ruby_block_fragment("symbol"));
    assert!(!super::looks_like_ruby_block_fragment("&block"));
}

#[test]
fn ruby_fragment_hint_for_block_mentions_selector() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "{ |$V| $V.$METHOD }",
        "lang": "ruby"
    }))
    .expect("valid request");
    let hint = super::fragment_pattern_hint(&request, super::AstGrepLanguage::Ruby);
    assert!(
        hint.contains("selector: call"),
        "should mention selector: call: {hint}"
    );
    assert!(
        hint.contains("block"),
        "should mention block node kind: {hint}"
    );
}

#[test]
fn ruby_fragment_hint_for_do_block_mentions_do_block() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "do |$V| $V.$METHOD end",
        "lang": "ruby"
    }))
    .expect("valid request");
    let hint = super::fragment_pattern_hint(&request, super::AstGrepLanguage::Ruby);
    assert!(
        hint.contains("do_block"),
        "should mention do_block node kind: {hint}"
    );
}

#[test]
fn ruby_fragment_hint_for_symbol_to_proc_mentions_call() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "&:$METHOD",
        "lang": "ruby"
    }))
    .expect("valid request");
    let hint = super::fragment_pattern_hint(&request, super::AstGrepLanguage::Ruby);
    assert!(
        hint.contains("$LIST.$ITER(&:$METHOD)"),
        "should mention symbol-to-proc pattern: {hint}"
    );
    assert!(
        hint.contains("symbol"),
        "should mention symbol node kind: {hint}"
    );
}

// ---------------------------------------------------------------------------
// regex field
// ---------------------------------------------------------------------------

#[test]
fn regex_field_is_accepted() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "regex": "console\\.log",
        "lang": "typescript"
    }))
    .expect("regex-only request should parse");
    assert_eq!(request.regex_pattern(), Some("console\\.log"));
    assert!(
        request.validate_query().is_ok(),
        "regex-only query should pass validation"
    );
}

#[test]
fn regex_field_with_pattern_and_kind_is_ok() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "pattern": "console.log($A)",
        "kind": "call_expression",
        "regex": "console\\.log",
        "lang": "typescript"
    }))
    .expect("should parse");
    assert!(
        request.validate_query().is_ok(),
        "pattern+kind+regex combo should pass"
    );
}

#[test]
fn regex_field_rejected_for_scan() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "scan",
        "regex": "foo"
    }))
    .expect_err("regex should be rejected for scan");
    assert!(
        err.to_string().contains("regex"),
        "error should mention regex: {err}"
    );
}

#[test]
fn regex_field_rejected_for_test() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "test",
        "regex": "foo"
    }))
    .expect_err("regex should be rejected for test");
    assert!(
        err.to_string().contains("regex"),
        "error should mention regex: {err}"
    );
}

#[test]
fn regex_field_rejected_for_inspect() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "inspect",
        "regex": "foo"
    }))
    .expect_err("regex should be rejected for inspect");
    assert!(
        err.to_string().contains("regex"),
        "error should mention regex: {err}"
    );
}

#[test]
fn regex_field_accepted_for_rewrite() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "rewrite",
        "regex": "console\\.log",
        "rewrite": "logger.debug",
        "lang": "typescript"
    }))
    .expect("should parse");
    assert!(
        request.validate_rewrite().is_ok(),
        "regex should be accepted for rewrite"
    );
}

// ---------------------------------------------------------------------------
// nth_child field
// ---------------------------------------------------------------------------

#[test]
fn nth_child_number_is_accepted() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "nth_child": 2,
        "lang": "typescript"
    }))
    .expect("should parse");
    assert!(
        request.validate_query().is_ok(),
        "nth_child number should pass validation"
    );
}

#[test]
fn nth_child_formula_is_accepted() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "nth_child": "2n+1",
        "lang": "typescript"
    }))
    .expect("should parse");
    assert!(
        request.validate_query().is_ok(),
        "nth_child formula should pass validation"
    );
}

#[test]
fn nth_child_object_is_accepted() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "nth_child": {
            "position": 3,
            "reverse": true,
            "ofRule": {"kind": "method_definition"}
        },
        "lang": "typescript"
    }))
    .expect("should parse");
    assert!(
        request.validate_query().is_ok(),
        "nth_child object should pass validation"
    );
}

#[test]
fn nth_child_reject_zero() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "nth_child": 0,
        "lang": "typescript"
    }))
    .expect_err("nth_child 0 should fail");
    assert!(
        err.to_string().contains("1-based"),
        "error should mention 1-based: {err}"
    );
}

#[test]
fn nth_child_reject_zero_position_in_object() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "nth_child": {"position": 0},
        "lang": "typescript"
    }))
    .expect_err("position 0 should fail");
    assert!(
        err.to_string().contains("1-based"),
        "error should mention 1-based: {err}"
    );
}

#[test]
fn nth_child_rejected_for_scan() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "scan",
        "nth_child": 1
    }))
    .expect_err("nth_child should be rejected for scan");
    assert!(
        err.to_string().contains("nth_child"),
        "error should mention nth_child: {err}"
    );
}

#[test]
fn nth_child_rejected_for_rewrite() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "rewrite",
        "nth_child": 1,
        "pattern": "foo",
        "rewrite": "x"
    }))
    .expect_err("nth_child should be rejected for rewrite");
    assert!(
        err.to_string().contains("nth_child"),
        "error should mention nth_child: {err}"
    );
}

#[test]
fn nth_child_rejected_for_test() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "test",
        "nth_child": 1
    }))
    .expect_err("nth_child should be rejected for test");
    assert!(
        err.to_string().contains("nth_child"),
        "error should mention nth_child: {err}"
    );
}

#[test]
fn nth_child_rejected_for_inspect() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "inspect",
        "nth_child": 1
    }))
    .expect_err("nth_child should be rejected for inspect");
    assert!(
        err.to_string().contains("nth_child"),
        "error should mention nth_child: {err}"
    );
}

// ---------------------------------------------------------------------------
// range field
// ---------------------------------------------------------------------------

#[test]
fn range_field_is_accepted() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "range": {"start": {"line": 1, "column": 0}, "end": {"line": 5, "column": 0}},
        "lang": "typescript"
    }))
    .expect("should parse");
    assert!(
        request.validate_query().is_ok(),
        "range should pass validation"
    );
}

#[test]
fn range_field_rejected_for_scan() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "scan",
        "range": {"start": {"line": 0, "column": 0}, "end": {"line": 1, "column": 0}}
    }))
    .expect_err("range should be rejected for scan");
    assert!(
        err.to_string().contains("range"),
        "error should mention range: {err}"
    );
}

#[test]
fn range_field_rejected_for_rewrite() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "rewrite",
        "range": {"start": {"line": 0, "column": 0}, "end": {"line": 1, "column": 0}},
        "pattern": "foo",
        "rewrite": "x"
    }))
    .expect_err("range should be rejected for rewrite");
    assert!(
        err.to_string().contains("range"),
        "error should mention range: {err}"
    );
}

#[test]
fn range_field_rejected_for_test() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "test",
        "range": {"start": {"line": 0, "column": 0}, "end": {"line": 1, "column": 0}}
    }))
    .expect_err("range should be rejected for test");
    assert!(
        err.to_string().contains("range"),
        "error should mention range: {err}"
    );
}

#[test]
fn range_field_rejected_for_inspect() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "inspect",
        "range": {"start": {"line": 0, "column": 0}, "end": {"line": 1, "column": 0}}
    }))
    .expect_err("range should be rejected for inspect");
    assert!(
        err.to_string().contains("range"),
        "error should mention range: {err}"
    );
}

// ---------------------------------------------------------------------------
// YAML rule generation for nthChild
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Integration: validate_query accepts nthChild/range without pattern or kind
// ---------------------------------------------------------------------------

#[test]
fn query_nth_child_only_without_pattern_or_kind_passes_validation() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "nth_child": 1,
        "lang": "typescript"
    }))
    .expect("should parse");
    assert!(
        request.validate_query().is_ok(),
        "nthChild alone (no pattern/kind) should pass validate_query"
    );
}

#[test]
fn query_range_only_without_pattern_or_kind_passes_validation() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "range": {"start": {"line": 0, "column": 0}, "end": {"line": 10, "column": 0}},
        "lang": "typescript"
    }))
    .expect("should parse");
    assert!(
        request.validate_query().is_ok(),
        "range alone (no pattern/kind) should pass validate_query"
    );
}

#[test]
fn query_regex_only_without_pattern_or_kind_passes_validation() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "regex": "foo",
        "lang": "typescript"
    }))
    .expect("should parse");
    assert!(
        request.validate_query().is_ok(),
        "regex alone (no pattern/kind) should pass validate_query"
    );
}

// ---------------------------------------------------------------------------
// Relational rule YAML generation tests
// ---------------------------------------------------------------------------

#[test]
fn build_atomic_rule_yaml_emits_has_pattern_string() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "pattern": "await $PROMISE",
        "lang": "typescript",
        "has": "return"
    }))
    .expect("valid request");

    let yaml = build_atomic_rule_yaml(&request, "typescript");
    assert!(yaml.contains("pattern: await $PROMISE"), "{yaml}");
    assert!(yaml.contains("has:\n"), "{yaml}");
    assert!(yaml.contains("  pattern: return\n"), "{yaml}");
}

#[test]
fn build_atomic_rule_yaml_emits_has_rule_object() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "pattern": "await $PROMISE",
        "lang": "typescript",
        "has": {
            "kind": "for_in_statement",
            "stopBy": "end"
        }
    }))
    .expect("valid request");

    let yaml = build_atomic_rule_yaml(&request, "typescript");
    assert!(yaml.contains("has:\n"), "{yaml}");
    assert!(yaml.contains("kind: for_in_statement\n"), "{yaml}");
    assert!(yaml.contains("stopBy: end\n"), "{yaml}");
}

#[test]
fn build_atomic_rule_yaml_emits_inside_pattern_string() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "pattern": "return $VAL",
        "lang": "typescript",
        "inside": "function $NAME($$$) { $$$ }"
    }))
    .expect("valid request");

    let yaml = build_atomic_rule_yaml(&request, "typescript");
    assert!(yaml.contains("inside:\n"), "{yaml}");
    assert!(
        yaml.contains("function $NAME($$$) { $$$ }"),
        "should contain inside pattern: {yaml}"
    );
}

#[test]
fn build_atomic_rule_yaml_emits_follows() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "pattern": "console.log('hello')",
        "lang": "javascript",
        "follows": {
            "pattern": "console.log('world')"
        }
    }))
    .expect("valid request");

    let yaml = build_atomic_rule_yaml(&request, "javascript");
    assert!(yaml.contains("follows:\n"), "{yaml}");
    assert!(yaml.contains("follows:"), "{yaml}");
    assert!(yaml.contains("console.log"), "{yaml}");
}

#[test]
fn build_atomic_rule_yaml_emits_precedes() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "pattern": "console.log('world')",
        "lang": "javascript",
        "precedes": {
            "pattern": "console.log('hello')"
        }
    }))
    .expect("valid request");

    let yaml = build_atomic_rule_yaml(&request, "javascript");
    assert!(yaml.contains("precedes:\n"), "{yaml}");
    assert!(yaml.contains("precedes:"), "{yaml}");
    assert!(yaml.contains("console.log"), "{yaml}");
}

#[test]
fn build_atomic_rule_yaml_emits_nested_relational_rules() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "pattern": "pass",
        "lang": "python",
        "inside": {
            "kind": "block",
            "has": {
                "kind": "expression_statement",
                "stopBy": "end"
            }
        }
    }))
    .expect("valid request");

    let yaml = build_atomic_rule_yaml(&request, "python");
    assert!(yaml.contains("inside:\n"), "{yaml}");
    assert!(yaml.contains("kind: block\n"), "{yaml}");
    assert!(yaml.contains("has:\n"), "{yaml}");
    assert!(yaml.contains("kind: expression_statement\n"), "{yaml}");
    assert!(yaml.contains("stopBy: end\n"), "{yaml}");
}

#[test]
fn build_atomic_rule_yaml_emits_constraints() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "pattern": "$FN($$$ARGS)",
        "lang": "typescript",
        "constraints": {
            "$FN": {
                "kind": "identifier",
                "regex": "^use"
            }
        }
    }))
    .expect("valid request");

    let yaml = build_atomic_rule_yaml(&request, "typescript");
    assert!(yaml.contains("constraints:\n"), "{yaml}");
    assert!(yaml.contains("$FN:\n"), "{yaml}");
    assert!(yaml.contains("kind: identifier\n"), "{yaml}");
    assert!(yaml.contains("regex: ^use\n"), "{yaml}");
}

#[test]
fn build_atomic_rule_yaml_emits_stopby_object() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "pattern": "$CALL",
        "lang": "typescript",
        "inside": {
            "kind": "function",
            "stopBy": {
                "kind": "function"
            }
        }
    }))
    .expect("valid request");

    let yaml = build_atomic_rule_yaml(&request, "typescript");
    assert!(yaml.contains("inside:\n"), "{yaml}");
    assert!(yaml.contains("stopBy:\n"), "{yaml}");
    assert!(yaml.contains("kind: function\n"), "{yaml}");
}

// ---------------------------------------------------------------------------
// Relational rule validation tests
// ---------------------------------------------------------------------------

#[test]
fn validate_query_accepts_has_as_sufficient() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "lang": "typescript",
        "has": { "kind": "for_in_statement" }
    }));
    assert!(
        request.is_ok(),
        "has alone should satisfy query validation: {:?}",
        request.err()
    );
}

#[test]
fn validate_query_accepts_inside_as_sufficient() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "lang": "typescript",
        "inside": { "kind": "function" }
    }));
    assert!(
        request.is_ok(),
        "inside alone should satisfy query validation: {:?}",
        request.err()
    );
}

#[test]
fn validate_query_accepts_follows_as_sufficient() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "lang": "javascript",
        "follows": { "pattern": "console.log('a')" }
    }));
    assert!(
        request.is_ok(),
        "follows alone should satisfy query validation: {:?}",
        request.err()
    );
}

#[test]
fn validate_query_accepts_precedes_as_sufficient() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "lang": "javascript",
        "precedes": { "pattern": "console.log('b')" }
    }));
    assert!(
        request.is_ok(),
        "precedes alone should satisfy query validation: {:?}",
        request.err()
    );
}

#[test]
fn validate_query_requires_lang_with_relational_rules() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "has": { "kind": "for_in_statement" }
    }));
    let err = request.expect_err("relational rules without lang should fail");
    assert!(err.to_string().contains("requires `lang`"), "{err}");
}

#[test]
fn validate_query_rejects_no_pattern_no_relational() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "lang": "typescript"
    }));
    let err = request.expect_err("no pattern and no relational rules should fail");
    assert!(err.to_string().contains("requires a non-empty"), "{err}");
}

#[test]
fn validate_count_accepts_relational_rules() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "count",
        "lang": "typescript",
        "has": { "kind": "for_in_statement" }
    }));
    assert!(
        request.is_ok(),
        "count should accept relational rules: {:?}",
        request.err()
    );
}

#[test]
fn validate_rewrite_rejects_relational_rules() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "rewrite",
        "pattern": "foo()",
        "rewrite": "bar()",
        "lang": "typescript",
        "has": { "kind": "function" }
    }));
    let err = request.expect_err("rewrite should reject relational rules");
    assert!(
        err.to_string().contains("does not accept relational"),
        "{err}"
    );
}

#[test]
fn validate_rewrite_rejects_follows() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "rewrite",
        "pattern": "foo()",
        "rewrite": "bar()",
        "lang": "typescript",
        "follows": { "pattern": "baz()" }
    }));
    let err = request.expect_err("rewrite should reject follows");
    assert!(
        err.to_string().contains("does not accept relational"),
        "{err}"
    );
}

// ---------------------------------------------------------------------------
// Relational rule deserialization tests
// ---------------------------------------------------------------------------

#[test]
fn relational_rule_input_deserializes_pattern_string() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "pattern": "foo()",
        "lang": "typescript",
        "has": "return"
    }))
    .expect("should deserialize");
    assert!(request.has.is_some());
}

#[test]
fn relational_rule_input_deserializes_rule_object() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "pattern": "foo()",
        "lang": "typescript",
        "inside": {
            "kind": "function",
            "stopBy": "end"
        }
    }))
    .expect("should deserialize");
    assert!(request.inside.is_some());
}

#[test]
fn relational_rule_input_deserializes_nested_rules() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "pattern": "pass",
        "lang": "python",
        "inside": {
            "kind": "block",
            "has": {
                "kind": "expression_statement",
                "stopBy": "end"
            }
        }
    }))
    .expect("should deserialize nested rules");
    assert!(request.inside.is_some());
}

#[test]
fn relational_rule_input_deserializes_follows_and_precedes() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "pattern": "bar()",
        "lang": "javascript",
        "follows": { "pattern": "foo()" },
        "precedes": { "pattern": "baz()" }
    }))
    .expect("should deserialize");
    assert!(request.follows.is_some());
    assert!(request.precedes.is_some());
}

// --- interactive / update_all flag tests ---

#[test]
fn structural_request_accepts_interactive_for_test() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "test",
        "interactive": true
    }))
    .expect("interactive should be accepted for test workflow");

    assert_eq!(request.workflow, StructuralWorkflow::Test);
}

#[test]
fn structural_request_accepts_update_all_for_test() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "test",
        "update_all": true
    }))
    .expect("update_all should be accepted for test workflow");

    assert_eq!(request.workflow, StructuralWorkflow::Test);
}

#[test]
fn structural_request_rejects_interactive_for_query() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "fn $NAME() {}",
        "interactive": true
    }))
    .expect_err("interactive should be rejected for query workflow");

    assert!(err.to_string().contains("does not accept `interactive`"));
}

#[test]
fn structural_request_rejects_update_all_for_query() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "fn $NAME() {}",
        "update_all": true
    }))
    .expect_err("update_all should be rejected for query workflow");

    assert!(err.to_string().contains("does not accept `update_all`"));
}

#[test]
fn structural_request_rejects_interactive_for_scan() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "scan",
        "interactive": true
    }))
    .expect_err("interactive should be rejected for scan workflow");

    assert!(err.to_string().contains("does not accept `interactive`"));
}

#[test]
fn structural_request_rejects_update_all_for_scan() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "scan",
        "update_all": true
    }))
    .expect_err("update_all should be rejected for scan workflow");

    assert!(err.to_string().contains("does not accept `update_all`"));
}

#[test]
fn structural_request_rejects_interactive_for_rewrite() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "let a = 1",
        "rewrite": "const a = 1",
        "lang": "javascript",
        "interactive": true
    }))
    .expect_err("interactive should be rejected for rewrite workflow");

    assert!(err.to_string().contains("does not accept `interactive`"));
}

#[test]
fn structural_request_rejects_update_all_for_rewrite() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "let a = 1",
        "rewrite": "const a = 1",
        "lang": "javascript",
        "update_all": true
    }))
    .expect_err("update_all should be rejected for rewrite workflow");

    assert!(err.to_string().contains("does not accept `update_all`"));
}

#[test]
fn structural_request_rejects_interactive_for_inspect() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "inspect",
        "interactive": true
    }))
    .expect_err("interactive should be rejected for inspect workflow");

    assert!(err.to_string().contains("does not accept `interactive`"));
}

#[test]
fn structural_request_rejects_update_all_for_inspect() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "inspect",
        "update_all": true
    }))
    .expect_err("update_all should be rejected for inspect workflow");

    assert!(err.to_string().contains("does not accept `update_all`"));
}

#[test]
fn structural_request_rejects_snapshot_dir_for_query() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "fn $NAME() {}",
        "snapshot_dir": "__snapshots__"
    }))
    .expect_err("snapshot_dir should be rejected for query workflow");

    assert!(err.to_string().contains("does not accept `snapshot_dir`"));
}

#[test]
fn structural_request_rejects_snapshot_dir_for_scan() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "scan",
        "snapshot_dir": "__snapshots__"
    }))
    .expect_err("snapshot_dir should be rejected for scan workflow");

    assert!(err.to_string().contains("does not accept `snapshot_dir`"));
}

#[test]
fn structural_request_rejects_snapshot_dir_for_inspect() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "inspect",
        "snapshot_dir": "__snapshots__"
    }))
    .expect_err("snapshot_dir should be rejected for inspect workflow");

    assert!(err.to_string().contains("does not accept `snapshot_dir`"));
}

#[test]
fn structural_request_rejects_test_dir_for_query() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "fn $NAME() {}",
        "test_dir": "my-tests"
    }))
    .expect_err("test_dir should be rejected for query workflow");

    assert!(err.to_string().contains("does not accept `test_dir`"));
}

#[test]
fn structural_request_rejects_test_dir_for_scan() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "scan",
        "test_dir": "my-tests"
    }))
    .expect_err("test_dir should be rejected for scan workflow");

    assert!(err.to_string().contains("does not accept `test_dir`"));
}

#[test]
fn structural_request_rejects_test_dir_for_rewrite() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "let a = 1",
        "rewrite": "const a = 1",
        "lang": "javascript",
        "test_dir": "my-tests"
    }))
    .expect_err("test_dir should be rejected for rewrite workflow");

    assert!(err.to_string().contains("does not accept `test_dir`"));
}

#[test]
fn structural_request_rejects_include_off_for_query() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "fn $NAME() {}",
        "include_off": true
    }))
    .expect_err("include_off should be rejected for query workflow");

    assert!(err.to_string().contains("does not accept `include_off`"));
}

#[test]
fn structural_request_rejects_include_off_for_scan() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "scan",
        "include_off": true
    }))
    .expect_err("include_off should be rejected for scan workflow");

    assert!(err.to_string().contains("does not accept `include_off`"));
}

#[test]
fn structural_request_rejects_include_off_for_inspect() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "inspect",
        "include_off": true
    }))
    .expect_err("include_off should be rejected for inspect workflow");

    assert!(err.to_string().contains("does not accept `include_off`"));
}

// --- per-rule result parsing tests ---

#[test]
fn parse_test_rule_results_parses_pass_and_fail() {
    let stdout = "\
Running 3 tests
PASS rust/no-iterator-for-each .....
PASS rust/prefer-retain ............
FAIL rust/for-each-snapshot ...N..M
test result: failed. 2 passed; 1 failed;";

    let results = parse_test_rule_results(stdout);
    assert_eq!(results.len(), 3);

    assert_eq!(results[0].rule_id, "rust/no-iterator-for-each");
    assert!(results[0].passed);
    assert!(results[0].markers.is_empty());

    assert_eq!(results[1].rule_id, "rust/prefer-retain");
    assert!(results[1].passed);
    assert!(results[1].markers.is_empty());

    assert_eq!(results[2].rule_id, "rust/for-each-snapshot");
    assert!(!results[2].passed);
    assert_eq!(results[2].markers, vec!["noisy", "missing"]);
}

#[test]
fn parse_test_rule_results_handles_empty_output() {
    let results = parse_test_rule_results("");
    assert!(results.is_empty());
}

#[test]
fn parse_test_rule_results_ignores_non_rule_lines() {
    let stdout = "\
Running 2 tests
some random output
PASS rust/foo ..
test result: ok. 1 passed; 0 failed;";

    let results = parse_test_rule_results(stdout);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].rule_id, "rust/foo");
}

#[test]
fn parse_test_rule_results_handles_noisy_only() {
    let stdout = "FAIL python/no-print .N";

    let results = parse_test_rule_results(stdout);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].rule_id, "python/no-print");
    assert!(!results[0].passed);
    assert_eq!(results[0].markers, vec!["noisy"]);
}

#[test]
fn parse_test_rule_results_handles_missing_only() {
    let stdout = "FAIL python/no-print .M";

    let results = parse_test_rule_results(stdout);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].rule_id, "python/no-print");
    assert!(!results[0].passed);
    assert_eq!(results[0].markers, vec!["missing"]);
}

// --- failure detail parsing tests ---

#[test]
fn parse_test_failure_details_parses_noisy_and_missing() {
    let stdout = "\
----------- Failure Details -----------
[Noisy] Expect no-await-in-loop to report no issue, but some issues found in:

  async function foo() { for (var bar of baz) await bar; }

[Missing] Expect rule no-await-in-loop to report issues, but none found in:

  for (let a of b) { console.log(a) }";

    let details = parse_test_failure_details(stdout, "");
    assert_eq!(details.len(), 2);

    assert_eq!(details[0]["type"], "noisy");
    assert_eq!(details[0]["rule_id"], "no-await-in-loop");
    assert!(
        details[0]["code_snippet"]
            .as_str()
            .unwrap()
            .contains("await bar")
    );

    assert_eq!(details[1]["type"], "missing");
    assert_eq!(details[1]["rule_id"], "no-await-in-loop");
    assert!(
        details[1]["code_snippet"]
            .as_str()
            .unwrap()
            .contains("console.log")
    );
}

#[test]
fn parse_test_failure_details_handles_empty_input() {
    let details = parse_test_failure_details("", "");
    assert!(details.is_empty());
}

#[test]
fn parse_test_failure_details_parses_from_stderr() {
    let stderr = "\
[Noisy] Expect no-print to report no issue, but some issues found in:

  print(\"debug output\")";

    let details = parse_test_failure_details("", stderr);
    assert_eq!(details.len(), 1);
    assert_eq!(details[0]["type"], "noisy");
    assert_eq!(details[0]["rule_id"], "no-print");
    assert!(
        details[0]["code_snippet"]
            .as_str()
            .unwrap()
            .contains("print")
    );
}

// --- command flag passing tests ---

#[tokio::test]
#[serial]
async fn structural_test_passes_interactive_flag() {
    let temp = TempDir::new().expect("workspace tempdir");
    fs::write(temp.path().join("sgconfig.yml"), "ruleDirs: []\n").expect("write config");
    let args_path = temp.path().join("sg_args.txt");
    let script = format!(
        "#!/bin/sh\nprintf '%s\n' \"$@\" > \"{}\"\nprintf 'Running 0 tests\\ntest result: ok. 0 passed; 0 failed;'\n",
        args_path.display(),
    );
    let (_script_dir, script_path) = write_fake_sg(&script);

    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "workflow": "test",
            "interactive": true
        }),
    )
    .await
    .expect("test workflow should succeed");

    let args = fs::read_to_string(args_path).expect("read sg args");
    assert!(
        args.lines().any(|line| line == "--interactive"),
        "expected --interactive in args: {args}"
    );
}

#[tokio::test]
#[serial]
async fn structural_test_passes_update_all_flag() {
    let temp = TempDir::new().expect("workspace tempdir");
    fs::write(temp.path().join("sgconfig.yml"), "ruleDirs: []\n").expect("write config");
    let args_path = temp.path().join("sg_args.txt");
    let script = format!(
        "#!/bin/sh\nprintf '%s\n' \"$@\" > \"{}\"\nprintf 'Running 0 tests\\ntest result: ok. 0 passed; 0 failed;'\n",
        args_path.display(),
    );
    let (_script_dir, script_path) = write_fake_sg(&script);

    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "workflow": "test",
            "update_all": true
        }),
    )
    .await
    .expect("test workflow should succeed");

    let args = fs::read_to_string(args_path).expect("read sg args");
    assert!(
        args.lines().any(|line| line == "--update-all"),
        "expected --update-all in args: {args}"
    );
}

// --- enhanced test summary tests ---

#[tokio::test]
#[serial]
async fn structural_test_enriched_summary_with_rules_and_failures() {
    let temp = TempDir::new().expect("workspace tempdir");
    fs::write(temp.path().join("sgconfig.yml"), "ruleDirs: []\n").expect("write config");
    let args_path = temp.path().join("sg_args.txt");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{}\"\n\
printf '\\033[32mRunning 3 tests\\033[0m\\nPASS rust/no-iterator-for-each .....\\nPASS rust/prefer-retain ............\\nFAIL rust/for-each-snapshot ...N..M\\ntest result: failed. 2 passed; 1 failed;\\n----------- Failure Details -----------\\n[Noisy] Expect rust/for-each-snapshot to report no issue, but some issues found in:\\n\\n  for (let x of arr) {{ await x; }}\\n\\n[Missing] Expect rule rust/for-each-snapshot to report issues, but none found in:\\n\\n  arr.forEach(x => console.log(x))\\n'\n\
exit 1\n",
        args_path.display(),
    );
    let (_script_dir, script_path) = write_fake_sg(&script);

    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let result = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "workflow": "test",
            "skip_snapshot_tests": true
        }),
    )
    .await
    .expect("test workflow should return structured result");

    assert_eq!(result["passed"], false);
    assert_eq!(result["summary"]["status"], "failed");
    assert_eq!(result["summary"]["passed_cases"], 2);
    assert_eq!(result["summary"]["failed_cases"], 1);

    let rules = result["summary"]["rules"].as_array().expect("rules array");
    assert_eq!(rules.len(), 3);
    assert_eq!(rules[0]["rule_id"], "rust/no-iterator-for-each");
    assert_eq!(rules[0]["passed"], true);
    assert!(rules[0]["markers"].as_array().unwrap().is_empty());
    assert_eq!(rules[2]["rule_id"], "rust/for-each-snapshot");
    assert_eq!(rules[2]["passed"], false);
    assert_eq!(rules[2]["markers"][0], "noisy");
    assert_eq!(rules[2]["markers"][1], "missing");

    let failures = result["summary"]["failure_details"]
        .as_array()
        .expect("failure_details array");
    assert_eq!(failures.len(), 2);
    assert_eq!(failures[0]["type"], "noisy");
    assert_eq!(failures[0]["rule_id"], "rust/for-each-snapshot");
    assert!(
        failures[0]["code_snippet"]
            .as_str()
            .unwrap()
            .contains("await x")
    );
    assert_eq!(failures[1]["type"], "missing");
    assert_eq!(failures[1]["rule_id"], "rust/for-each-snapshot");
    assert!(
        failures[1]["code_snippet"]
            .as_str()
            .unwrap()
            .contains("forEach")
    );
}

// ---------------------------------------------------------------------------
// Composite rule validation tests
// ---------------------------------------------------------------------------

#[test]
fn validate_query_accepts_matches_with_lang() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "lang": "javascript",
        "matches": "is-literal"
    }));
    assert!(
        request.is_ok(),
        "query should accept `matches` with `lang`: {:?}",
        request.err()
    );
}

#[test]
fn validate_query_accepts_all_composite() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "lang": "rust",
        "all": [
            { "kind": "function_item" },
            { "has": { "kind": "unsafe_block" } }
        ]
    }));
    assert!(
        request.is_ok(),
        "query should accept `all` composite: {:?}",
        request.err()
    );
}

#[test]
fn validate_query_accepts_any_composite() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "lang": "python",
        "any": [
            { "pattern": "print($$$)" },
            { "pattern": "pprint($$$)" }
        ]
    }));
    assert!(
        request.is_ok(),
        "query should accept `any` composite: {:?}",
        request.err()
    );
}

#[test]
fn validate_query_requires_lang_for_matches() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "matches": "is-literal"
    }));
    let err = request.expect_err("matches without lang should fail");
    assert!(err.to_string().contains("requires `lang`"), "{err}");
}

#[test]
fn validate_query_requires_lang_for_all() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "all": [{ "kind": "function_item" }]
    }));
    let err = request.expect_err("all without lang should fail");
    assert!(err.to_string().contains("requires `lang`"), "{err}");
}

#[test]
fn validate_query_requires_lang_for_any() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "any": [{ "pattern": "foo()" }]
    }));
    let err = request.expect_err("any without lang should fail");
    assert!(err.to_string().contains("requires `lang`"), "{err}");
}

#[test]
fn validate_query_accepts_utils_with_matches() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "lang": "javascript",
        "matches": "is-literal",
        "utils": {
            "is-literal": {
                "any": [
                    { "kind": "number" },
                    { "kind": "string" }
                ]
            }
        }
    }));
    assert!(
        request.is_ok(),
        "query should accept `utils` with `matches`: {:?}",
        request.err()
    );
}

#[test]
fn validate_scan_rejects_composite_rules() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "scan",
        "matches": "is-literal"
    }));
    let err = request.expect_err("scan should reject composite rules");
    assert!(
        err.to_string().contains("does not accept composite"),
        "{err}"
    );
}

#[test]
fn validate_scan_rejects_all_composite() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "scan",
        "all": [{ "kind": "function_item" }]
    }));
    let err = request.expect_err("scan should reject all composite");
    assert!(
        err.to_string().contains("does not accept composite"),
        "{err}"
    );
}

#[test]
fn validate_test_rejects_composite_rules() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "test",
        "any": [{ "pattern": "foo()" }]
    }));
    let err = request.expect_err("test should reject composite rules");
    assert!(
        err.to_string().contains("does not accept composite"),
        "{err}"
    );
}

#[test]
fn validate_inspect_rejects_composite_rules() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "inspect",
        "utils": { "foo": { "kind": "identifier" } }
    }));
    let err = request.expect_err("inspect should reject composite rules");
    assert!(
        err.to_string().contains("does not accept composite"),
        "{err}"
    );
}

#[test]
fn validate_rewrite_rejects_composite_rules() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "rewrite",
        "pattern": "foo()",
        "rewrite": "bar()",
        "lang": "typescript",
        "matches": "is-literal"
    }));
    let err = request.expect_err("rewrite should reject composite rules");
    assert!(
        err.to_string().contains("does not accept composite"),
        "{err}"
    );
}

#[test]
fn validate_count_accepts_matches() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "count",
        "lang": "javascript",
        "matches": "is-literal"
    }));
    assert!(
        request.is_ok(),
        "count should accept `matches`: {:?}",
        request.err()
    );
}

// ---------------------------------------------------------------------------
// Composite rule YAML generation tests
// ---------------------------------------------------------------------------

#[test]
fn build_atomic_rule_yaml_emits_matches() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "lang": "javascript",
        "matches": "is-literal"
    }))
    .expect("valid request");

    let yaml = build_atomic_rule_yaml(&request, "javascript");
    assert!(
        yaml.contains("matches:"),
        "should contain matches key: {yaml}"
    );
    assert!(
        yaml.contains("is-literal"),
        "should contain utility name: {yaml}"
    );
}

#[test]
fn build_atomic_rule_yaml_emits_all_composite() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "lang": "rust",
        "all": [
            { "kind": "function_item" },
            { "has": { "kind": "unsafe_block" } }
        ]
    }))
    .expect("valid request");

    let yaml = build_atomic_rule_yaml(&request, "rust");
    assert!(yaml.contains("all:\n"), "{yaml}");
    assert!(yaml.contains("kind: function_item\n"), "{yaml}");
    assert!(yaml.contains("has:\n"), "{yaml}");
    assert!(yaml.contains("kind: unsafe_block\n"), "{yaml}");
}

#[test]
fn build_atomic_rule_yaml_emits_any_with_string_patterns() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "lang": "python",
        "any": [
            "print($$$)",
            "pprint($$$)"
        ]
    }))
    .expect("valid request");

    let yaml = build_atomic_rule_yaml(&request, "python");
    assert!(yaml.contains("any:\n"), "{yaml}");
    assert!(yaml.contains("pattern: print($$$)\n"), "{yaml}");
    assert!(yaml.contains("pattern: pprint($$$)\n"), "{yaml}");
}

#[test]
fn build_atomic_rule_yaml_emits_not() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "lang": "typescript",
        "kind": "call_expression",
        "not": { "pattern": "console.log($$$)" }
    }))
    .expect("valid request");

    let yaml = build_atomic_rule_yaml(&request, "typescript");
    assert!(yaml.contains("not:\n"), "{yaml}");
    assert!(yaml.contains("pattern: console.log($$$)\n"), "{yaml}");
}

#[test]
fn build_atomic_rule_yaml_emits_not_string_pattern() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "lang": "typescript",
        "kind": "call_expression",
        "not": "console.log($$$)"
    }))
    .expect("valid request");

    let yaml = build_atomic_rule_yaml(&request, "typescript");
    assert!(yaml.contains("not:\n"), "{yaml}");
    assert!(yaml.contains("pattern: console.log($$$)\n"), "{yaml}");
}

#[test]
fn build_atomic_rule_yaml_emits_utils_section() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "lang": "javascript",
        "matches": "is-literal",
        "utils": {
            "is-literal": {
                "any": [
                    { "kind": "number" },
                    { "kind": "string" }
                ]
            }
        }
    }))
    .expect("valid request");

    let yaml = build_atomic_rule_yaml(&request, "javascript");
    assert!(yaml.contains("utils:\n"), "{yaml}");
    assert!(yaml.contains("is-literal:\n"), "{yaml}");
    assert!(yaml.contains("any:\n"), "{yaml}");
    assert!(yaml.contains("kind: number\n"), "{yaml}");
    assert!(yaml.contains("kind: string\n"), "{yaml}");
    // utils section should appear before rule section
    let utils_pos = yaml.find("utils:").expect("utils section");
    let rule_pos = yaml.find("rule:").expect("rule section");
    assert!(
        utils_pos < rule_pos,
        "utils section should appear before rule section: {yaml}"
    );
}

#[test]
fn build_atomic_rule_yaml_emits_matches_with_relational() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "query",
        "lang": "rust",
        "matches": "if-no-else",
        "has": {
            "field": "consequence",
            "kind": "block"
        }
    }))
    .expect("valid request");

    let yaml = build_atomic_rule_yaml(&request, "rust");
    assert!(
        yaml.contains("matches:"),
        "should contain matches key: {yaml}"
    );
    assert!(
        yaml.contains("if-no-else"),
        "should contain utility name: {yaml}"
    );
    assert!(yaml.contains("has:\n"), "{yaml}");
    assert!(yaml.contains("field: consequence\n"), "{yaml}");
    assert!(yaml.contains("kind: block\n"), "{yaml}");
}

// ---------------------------------------------------------------------------
// extract_rule_summary tests
// ---------------------------------------------------------------------------

#[test]
fn extract_rule_summary_extracts_utils_keys() {
    let content = r#"id: let-chain-candidate
language: Rust
severity: hint
message: Nested if/if let can be collapsed.
utils:
  sole-child:
    all:
      - nthChild: 1
      - nthChild:
          position: 1
          reverse: true
  if-no-else:
    kind: if_expression
    not:
      has:
        field: alternative
        kind: else_clause
rule:
  matches: if-no-else
"#;
    let path = Path::new("let-chain-candidate.yml");
    let summary = extract_rule_summary(content, path).expect("should extract");
    assert_eq!(summary["id"], "let-chain-candidate");
    assert_eq!(summary["language"], "Rust");
    let utils = summary["utils"].as_array().expect("utils array");
    assert_eq!(utils.len(), 2);
    assert!(utils.contains(&serde_json::Value::String("sole-child".to_string())));
    assert!(utils.contains(&serde_json::Value::String("if-no-else".to_string())));
}

#[test]
fn extract_rule_summary_no_utils_when_absent() {
    let content = r#"id: no-eval
language: JavaScript
severity: error
message: Do not use eval.
rule:
  pattern: eval($$$)
"#;
    let path = Path::new("no-eval.yml");
    let summary = extract_rule_summary(content, path).expect("should extract");
    assert_eq!(summary["id"], "no-eval");
    assert!(
        summary.get("utils").is_none(),
        "should not have utils field when no utils section: {summary}"
    );
}

#[test]
fn extract_rule_summary_handles_utils_with_comments() {
    let content = r#"id: example
language: Python
severity: warning
utils:
  # This is a comment
  is-string:
    kind: string
  is-number:
    kind: integer
rule:
  matches: is-string
"#;
    let path = Path::new("example.yml");
    let summary = extract_rule_summary(content, path).expect("should extract");
    let utils = summary["utils"].as_array().expect("utils array");
    assert_eq!(utils.len(), 2);
    assert!(utils.contains(&serde_json::Value::String("is-string".to_string())));
    assert!(utils.contains(&serde_json::Value::String("is-number".to_string())));
}

// --- transform field tests ---

#[test]
fn structural_request_accepts_transform_for_query() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "let $X = $Y",
        "lang": "javascript",
        "transform": {
            "UPPER": {
                "substring": { "source": "$Y", "startChar": 0, "endChar": 5 }
            }
        }
    }))
    .expect("transform should be accepted for query workflow");

    assert_eq!(request.workflow, StructuralWorkflow::Query);
    assert!(request.transform.is_some());
}

#[test]
fn structural_request_accepts_transform_for_count() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "count",
        "pattern": "let $X = $Y",
        "lang": "javascript",
        "transform": {
            "CAMEL": {
                "convert": { "source": "$X", "toCase": "camelCase" }
            }
        }
    }))
    .expect("transform should be accepted for count workflow");

    assert_eq!(request.workflow, StructuralWorkflow::Count);
    assert!(request.transform.is_some());
}

#[test]
fn structural_request_rejects_transform_for_scan() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "scan",
        "transform": { "X": { "replace": { "source": "$A", "replace": "a", "by": "b" } } }
    }))
    .expect_err("transform should be rejected for scan workflow");

    assert!(err.to_string().contains("does not accept `transform`"));
}

#[test]
fn structural_request_rejects_transform_for_test() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "test",
        "transform": { "X": { "replace": { "source": "$A", "replace": "a", "by": "b" } } }
    }))
    .expect_err("transform should be rejected for test workflow");

    assert!(err.to_string().contains("does not accept `transform`"));
}

#[test]
fn structural_request_rejects_transform_for_inspect() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "inspect",
        "transform": { "X": { "replace": { "source": "$A", "replace": "a", "by": "b" } } }
    }))
    .expect_err("transform should be rejected for inspect workflow");

    assert!(err.to_string().contains("does not accept `transform`"));
}

#[test]
fn structural_request_requires_lang_for_transform() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "let $X = $Y",
        "transform": {
            "UPPER": { "substring": { "source": "$Y", "startChar": 0, "endChar": 5 } }
        }
    }))
    .expect_err("transform without lang should be rejected");

    assert!(err.to_string().contains("requires `lang` to be set"));
}

#[test]
fn build_atomic_rule_yaml_emits_transform() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "let $X = $Y",
        "lang": "javascript",
        "transform": {
            "CAMEL": {
                "convert": { "source": "$X", "toCase": "camelCase" }
            }
        }
    }))
    .expect("valid request");

    let yaml = build_atomic_rule_yaml(&request, "javascript");
    assert!(
        yaml.contains("transform:"),
        "YAML should contain transform section: {yaml}"
    );
    assert!(
        yaml.contains("CAMEL:"),
        "YAML should contain transform variable name: {yaml}"
    );
    assert!(
        yaml.contains("convert:"),
        "YAML should contain transform operation: {yaml}"
    );
}

#[test]
fn build_atomic_rule_yaml_emits_transform_with_replace() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "console.log($MSG)",
        "lang": "javascript",
        "transform": {
            "CLEAN_MSG": {
                "replace": { "source": "$MSG", "replace": "'", "by": "\\'" }
            }
        }
    }))
    .expect("valid request");

    let yaml = build_atomic_rule_yaml(&request, "javascript");
    assert!(
        yaml.contains("transform:"),
        "YAML should contain transform: {yaml}"
    );
    assert!(
        yaml.contains("CLEAN_MSG:"),
        "YAML should contain var name: {yaml}"
    );
    assert!(
        yaml.contains("replace:"),
        "YAML should contain replace op: {yaml}"
    );
}

#[test]
fn build_fixconfig_rule_yaml_emits_transform() {
    let fix_config = FixConfig {
        template: "$CAMEL".to_string(),
        expand_start: None,
        expand_end: None,
    };
    let mut transform = serde_json::Map::new();
    transform.insert(
        "CAMEL".to_string(),
        json!({ "convert": { "source": "$X", "toCase": "camelCase" } }),
    );

    let yaml = build_fixconfig_rule_yaml(
        "let $X = $Y",
        "javascript",
        &fix_config,
        None,
        Some(&transform),
    );
    assert!(
        yaml.contains("transform:"),
        "YAML should contain transform: {yaml}"
    );
    assert!(
        yaml.contains("CAMEL:"),
        "YAML should contain var name: {yaml}"
    );
    assert!(
        yaml.contains("template: $CAMEL"),
        "YAML should contain fix template: {yaml}"
    );
}

#[test]
fn build_fixconfig_rule_yaml_omits_transform_when_none() {
    let fix_config = FixConfig {
        template: "$X".to_string(),
        expand_start: None,
        expand_end: None,
    };

    let yaml = build_fixconfig_rule_yaml("let $X = $Y", "javascript", &fix_config, None, None);
    assert!(
        !yaml.contains("transform:"),
        "YAML should not contain transform when None: {yaml}"
    );
}

// ---------------------------------------------------------------------------
// New feature tests: severities, has_error_findings, no_ignore, follow,
// threads, format, report_style, before/after lines, builtin_rules
// ---------------------------------------------------------------------------

#[test]
fn structural_request_accepts_severities_for_scan() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "scan",
        "severities": ["error", "warning"]
    }))
    .expect("severities should be accepted for scan");

    assert_eq!(
        request.severities.as_ref().unwrap(),
        &vec!["error".to_string(), "warning".to_string()]
    );
}

#[test]
fn structural_request_accepts_follow_flag() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "fn $NAME() {}",
        "follow": true
    }))
    .expect("follow should be accepted");

    assert!(request.effective_follow());
}

#[test]
fn structural_request_accepts_threads_for_scan() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "scan",
        "threads": 4
    }))
    .expect("threads should be accepted for scan");

    assert_eq!(request.effective_threads(), Some(4));
}

#[test]
fn structural_request_clamps_threads_to_max() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "scan",
        "threads": 999
    }))
    .expect("threads should be accepted");

    assert_eq!(request.effective_threads(), Some(256));
}

#[test]
fn structural_request_accepts_report_style_for_scan() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "scan",
        "report_style": "short"
    }))
    .expect("report_style should be accepted for scan");

    assert_eq!(request.effective_report_style(), Some("short"));
}

#[test]
fn structural_request_rejects_invalid_report_style() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "scan",
        "report_style": "verbose"
    }))
    .expect_err("invalid report_style should be rejected");

    assert!(err.to_string().contains("invalid `report_style`"));
}

#[test]
fn structural_request_rejects_invalid_format() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "scan",
        "format": "xml"
    }))
    .expect_err("invalid format should be rejected");

    assert!(err.to_string().contains("invalid `format`"));
}

#[test]
fn structural_request_accepts_before_after_lines() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "fn $NAME() {}",
        "before_lines": 3,
        "after_lines": 5
    }))
    .expect("before_lines/after_lines should be accepted");

    assert_eq!(request.effective_before_lines(), Some(3));
    assert_eq!(request.effective_after_lines(), Some(5));
}

#[test]
fn structural_request_rejects_context_with_before_after() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "fn $NAME() {}",
        "context_lines": 2,
        "before_lines": 3
    }))
    .expect_err("context_lines + before_lines should be rejected");

    assert!(err.to_string().contains("mutually exclusive"));
}

#[test]
fn structural_request_accepts_builtin_rules() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "scan",
        "builtin_rules": ["unused-suppression:error", "no-suppress-all"]
    }))
    .expect("builtin_rules should be accepted");

    assert_eq!(
        request.effective_builtin_rules().unwrap(),
        &[
            "unused-suppression:error".to_string(),
            "no-suppress-all".to_string()
        ]
    );
}

#[test]
fn structural_request_rejects_invalid_builtin_rule() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "scan",
        "builtin_rules": ["fake-rule"]
    }))
    .expect_err("invalid builtin rule should be rejected");

    assert!(err.to_string().contains("invalid builtin rule"));
}

#[test]
fn structural_request_rejects_invalid_no_ignore_value() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "pattern": "fn $NAME() {}",
        "no-ignore": ["hidden", "bogus"]
    }))
    .expect_err("invalid no_ignore value should be rejected");

    assert!(
        err.to_string()
            .contains("invalid `no_ignore` value `bogus`")
    );
}

#[test]
fn build_scan_summary_includes_has_error_findings() {
    let findings = vec![AstGrepScanFinding {
        file: "src/lib.rs".to_string(),
        text: "danger();".to_string(),
        lines: None,
        language: None,
        range: AstGrepRange {
            start: AstGrepPoint { line: 1, column: 0 },
            end: AstGrepPoint { line: 1, column: 9 },
            byte_offset: None,
        },
        rule_id: Some("deny-danger".to_string()),
        severity: Some(AstGrepSeverity::Error),
        message: None,
        note: None,
        metadata: None,
        labels: vec![],
    }];

    let summary = build_scan_summary(&findings, 1, false);
    assert_eq!(summary["has_error_findings"], true);
}

#[test]
fn build_scan_summary_has_error_findings_false_when_no_errors() {
    let findings = vec![AstGrepScanFinding {
        file: "src/lib.rs".to_string(),
        text: "warn();".to_string(),
        lines: None,
        language: None,
        range: AstGrepRange {
            start: AstGrepPoint { line: 1, column: 0 },
            end: AstGrepPoint { line: 1, column: 7 },
            byte_offset: None,
        },
        rule_id: Some("some-rule".to_string()),
        severity: Some(AstGrepSeverity::Warning),
        message: None,
        note: None,
        metadata: None,
        labels: vec![],
    }];

    let summary = build_scan_summary(&findings, 1, false);
    assert_eq!(summary["has_error_findings"], false);
}

#[test]
fn build_scan_result_filters_by_severities() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "scan",
        "severities": ["error"]
    }))
    .expect("valid request");

    let findings = vec![
        AstGrepScanFinding {
            file: "src/a.rs".to_string(),
            text: "a".to_string(),
            lines: None,
            language: None,
            range: AstGrepRange {
                start: AstGrepPoint { line: 1, column: 0 },
                end: AstGrepPoint { line: 1, column: 1 },
                byte_offset: None,
            },
            rule_id: Some("rule-a".to_string()),
            severity: Some(AstGrepSeverity::Error),
            message: None,
            note: None,
            metadata: None,
            labels: vec![],
        },
        AstGrepScanFinding {
            file: "src/b.rs".to_string(),
            text: "b".to_string(),
            lines: None,
            language: None,
            range: AstGrepRange {
                start: AstGrepPoint { line: 2, column: 0 },
                end: AstGrepPoint { line: 2, column: 1 },
                byte_offset: None,
            },
            rule_id: Some("rule-b".to_string()),
            severity: Some(AstGrepSeverity::Warning),
            message: None,
            note: None,
            metadata: None,
            labels: vec![],
        },
    ];

    let result = build_scan_result(&request, ".", "sgconfig.yml", findings);
    let returned_findings = result["findings"].as_array().expect("findings array");
    assert_eq!(returned_findings.len(), 1);
    assert_eq!(returned_findings[0]["rule_id"], "rule-a");
    assert_eq!(result["summary"]["total_findings"], 1);
}

#[tokio::test]
#[serial]
async fn structural_scan_passes_no_ignore_follow_threads_flags() {
    let temp = TempDir::new().expect("workspace tempdir");
    fs::create_dir_all(temp.path().join("src")).expect("create src");
    fs::write(temp.path().join("sgconfig.yml"), "ruleDirs: []\n").expect("write config");
    let args_path = temp.path().join("sg_args.txt");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{}\"\n",
        args_path.display()
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
            "no-ignore": ["hidden", "vcs"],
            "follow": true,
            "threads": 8
        }),
    )
    .await
    .expect("scan should succeed");

    assert_eq!(result["workflow"], "scan");

    let args = fs::read_to_string(args_path).expect("read sg args");
    let lines: Vec<&str> = args.lines().collect();
    assert!(
        lines.contains(&"--no-ignore"),
        "should have --no-ignore flag: {lines:?}"
    );
    assert!(lines.contains(&"hidden"), "should pass hidden: {lines:?}");
    assert!(lines.contains(&"vcs"), "should pass vcs: {lines:?}");
    assert!(
        lines.contains(&"--follow"),
        "should have --follow flag: {lines:?}"
    );
    assert!(
        lines.contains(&"--threads"),
        "should have --threads flag: {lines:?}"
    );
    assert!(lines.contains(&"8"), "should pass thread count: {lines:?}");
}

#[tokio::test]
#[serial]
async fn structural_scan_passes_report_style_flag() {
    let temp = TempDir::new().expect("workspace tempdir");
    fs::create_dir_all(temp.path().join("src")).expect("create src");
    fs::write(temp.path().join("sgconfig.yml"), "ruleDirs: []\n").expect("write config");
    let args_path = temp.path().join("sg_args.txt");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{}\"\n",
        args_path.display()
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
            "report_style": "medium"
        }),
    )
    .await
    .expect("scan should succeed");

    assert_eq!(result["workflow"], "scan");

    let args = fs::read_to_string(args_path).expect("read sg args");
    assert!(
        args.lines().any(|line| line == "--report-style=medium"),
        "should have --report-style=medium: {args}"
    );
}

#[tokio::test]
#[serial]
async fn structural_scan_passes_builtin_rules_flags() {
    let temp = TempDir::new().expect("workspace tempdir");
    fs::create_dir_all(temp.path().join("src")).expect("create src");
    fs::write(temp.path().join("sgconfig.yml"), "ruleDirs: []\n").expect("write config");
    let args_path = temp.path().join("sg_args.txt");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{}\"\n",
        args_path.display()
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
            "builtin_rules": ["unused-suppression:error", "no-suppress-all"]
        }),
    )
    .await
    .expect("scan should succeed");

    assert_eq!(result["workflow"], "scan");

    let args = fs::read_to_string(args_path).expect("read sg args");
    assert!(
        args.lines()
            .any(|line| line == "--error=unused-suppression"),
        "should have --error=unused-suppression: {args}"
    );
    assert!(
        args.lines().any(|line| line == "--hint=no-suppress-all"),
        "should have --hint=no-suppress-all: {args}"
    );
}

#[tokio::test]
#[serial]
async fn structural_query_passes_follow_and_no_ignore() {
    let temp = TempDir::new().expect("workspace tempdir");
    fs::create_dir_all(temp.path().join("src")).expect("create src");
    let args_path = temp.path().join("sg_args.txt");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{}\"\nprintf '[]'\n",
        args_path.display()
    );
    let (_script_dir, script_path) = write_fake_sg(&script);

    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let result = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "workflow": "query",
            "pattern": "fn $NAME() {}",
            "lang": "rust",
            "path": "src",
            "follow": true,
            "no-ignore": ["dot"]
        }),
    )
    .await
    .expect("query should succeed");

    assert_eq!(result["backend"], "ast-grep");

    let args = fs::read_to_string(args_path).expect("read sg args");
    assert!(
        args.lines().any(|line| line == "--follow"),
        "should have --follow flag: {args}"
    );
    assert!(
        args.lines().any(|line| line == "--no-ignore"),
        "should have --no-ignore flag: {args}"
    );
    assert!(
        args.lines().any(|line| line == "dot"),
        "should pass dot value: {args}"
    );
}

#[tokio::test]
#[serial]
async fn structural_query_passes_before_after_context() {
    let temp = TempDir::new().expect("workspace tempdir");
    fs::create_dir_all(temp.path().join("src")).expect("create src");
    let args_path = temp.path().join("sg_args.txt");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{}\"\nprintf '[]'\n",
        args_path.display()
    );
    let (_script_dir, script_path) = write_fake_sg(&script);

    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let result = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "workflow": "query",
            "pattern": "fn $NAME() {}",
            "lang": "rust",
            "path": "src",
            "before_lines": 3,
            "after_lines": 5
        }),
    )
    .await
    .expect("query should succeed");

    assert_eq!(result["backend"], "ast-grep");

    let args = fs::read_to_string(args_path).expect("read sg args");
    assert!(
        args.lines().any(|line| line == "--before"),
        "should have --before flag: {args}"
    );
    assert!(
        args.lines().any(|line| line == "3"),
        "should pass before count: {args}"
    );
    assert!(
        args.lines().any(|line| line == "--after"),
        "should have --after flag: {args}"
    );
    assert!(
        args.lines().any(|line| line == "5"),
        "should pass after count: {args}"
    );
    assert!(
        !args.lines().any(|line| line == "--context"),
        "should NOT have --context when before/after set: {args}"
    );
}

// ---------------------------------------------------------------------------
// New workflow: validation tests
// ---------------------------------------------------------------------------

#[test]
fn new_workflow_requires_subcommand() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "new"
    }))
    .expect_err("should reject when new_subcommand is missing");

    assert!(
        err.to_string().contains("new_subcommand"),
        "error should mention new_subcommand: {err}"
    );
}

#[test]
fn new_workflow_rejects_invalid_subcommand() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "new",
        "new_subcommand": "invalid"
    }))
    .expect_err("should reject invalid subcommand");

    assert!(
        err.to_string().contains("must be one of"),
        "error should mention valid subcommands: {err}"
    );
}

#[test]
fn new_workflow_accepts_project_subcommand_without_name() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "new",
        "new_subcommand": "project"
    }))
    .expect("project subcommand should not require new_name");

    assert_eq!(request.workflow, StructuralWorkflow::New);
}

#[test]
fn new_workflow_requires_name_for_rule() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "new",
        "new_subcommand": "rule",
        "lang": "rust"
    }))
    .expect_err("rule subcommand should require new_name");

    assert!(
        err.to_string().contains("requires `new_name`"),
        "error should mention new_name: {err}"
    );
}

#[test]
fn new_workflow_requires_name_for_test() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "new",
        "new_subcommand": "test"
    }))
    .expect_err("test subcommand should require new_name");

    assert!(
        err.to_string().contains("requires `new_name`"),
        "error should mention new_name: {err}"
    );
}

#[test]
fn new_workflow_requires_name_for_util() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "new",
        "new_subcommand": "util",
        "lang": "rust"
    }))
    .expect_err("util subcommand should require new_name");

    assert!(
        err.to_string().contains("requires `new_name`"),
        "error should mention new_name: {err}"
    );
}

#[test]
fn new_workflow_requires_lang_for_rule() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "new",
        "new_subcommand": "rule",
        "new_name": "no-console-log"
    }))
    .expect_err("rule subcommand should require lang");

    assert!(
        err.to_string().contains("requires `lang`"),
        "error should mention lang: {err}"
    );
}

#[test]
fn new_workflow_requires_lang_for_util() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "new",
        "new_subcommand": "util",
        "new_name": "is-literal"
    }))
    .expect_err("util subcommand should require lang");

    assert!(
        err.to_string().contains("requires `lang`"),
        "error should mention lang: {err}"
    );
}

#[test]
fn new_workflow_does_not_require_lang_for_test() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "new",
        "new_subcommand": "test",
        "new_name": "no-console-log"
    }))
    .expect("test subcommand should not require lang");

    assert_eq!(request.workflow, StructuralWorkflow::New);
}

#[test]
fn new_workflow_rejects_pattern_field() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "new",
        "new_subcommand": "project",
        "pattern": "fn $NAME() {}"
    }))
    .expect_err("new should reject pattern");

    assert!(
        err.to_string().contains("does not accept `pattern`"),
        "error should mention pattern: {err}"
    );
}

#[test]
fn new_workflow_rejects_rewrite_field() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "new",
        "new_subcommand": "project",
        "rewrite": "foo"
    }))
    .expect_err("new should reject rewrite");

    assert!(
        err.to_string().contains("does not accept `rewrite`"),
        "error should mention rewrite: {err}"
    );
}

#[test]
fn new_workflow_rejects_regex_field() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "new",
        "new_subcommand": "project",
        "regex": "foo"
    }))
    .expect_err("new should reject regex");

    assert!(
        err.to_string().contains("does not accept `regex`"),
        "error should mention regex: {err}"
    );
}

// ---------------------------------------------------------------------------
// Apply workflow: validation tests
// ---------------------------------------------------------------------------

#[test]
fn apply_workflow_requires_pattern_or_regex() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "apply",
        "rewrite": "bar()"
    }))
    .expect_err("apply should require pattern or regex");

    assert!(
        err.to_string().contains("requires a non-empty `pattern`"),
        "error should mention pattern: {err}"
    );
}

#[test]
fn apply_workflow_requires_rewrite_or_fix_config() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "apply",
        "pattern": "foo()",
        "lang": "rust"
    }))
    .expect_err("apply should require rewrite or fix_config");

    assert!(
        err.to_string().contains("requires a non-empty `rewrite`"),
        "error should mention rewrite: {err}"
    );
}

#[test]
fn apply_workflow_accepts_pattern_with_rewrite() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "apply",
        "pattern": "foo()",
        "rewrite": "bar()",
        "lang": "rust"
    }))
    .expect("apply with pattern+rewrite should be accepted");

    assert_eq!(request.workflow, StructuralWorkflow::Apply);
}

#[test]
fn apply_workflow_accepts_regex_with_rewrite() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "apply",
        "regex": "console\\.log",
        "rewrite": "logger.debug",
        "lang": "typescript"
    }))
    .expect("apply with regex+rewrite should be accepted");

    assert_eq!(request.workflow, StructuralWorkflow::Apply);
}

#[test]
fn apply_workflow_accepts_fix_config() {
    let request = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "apply",
        "pattern": "console.log($$$ARGS)",
        "lang": "javascript",
        "fix_config": {
            "template": "logger.log($$$ARGS)"
        }
    }))
    .expect("apply with fix_config should be accepted");

    assert_eq!(request.workflow, StructuralWorkflow::Apply);
    assert!(request.fix_config.is_some());
}

#[test]
fn apply_workflow_rejects_update_all() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "apply",
        "pattern": "foo()",
        "rewrite": "bar()",
        "lang": "rust",
        "update_all": true
    }))
    .expect_err("apply should reject update_all");

    assert!(
        err.to_string().contains("does not accept `update_all`"),
        "error should mention update_all: {err}"
    );
}

#[test]
fn apply_workflow_rejects_interactive() {
    let err = StructuralSearchRequest::from_args(&json!({
        "action": "structural",
        "workflow": "apply",
        "pattern": "foo()",
        "rewrite": "bar()",
        "lang": "rust",
        "interactive": true
    }))
    .expect_err("apply should reject interactive");

    assert!(
        err.to_string().contains("does not accept `interactive`"),
        "error should mention interactive: {err}"
    );
}

// ---------------------------------------------------------------------------
// New workflow: execution tests
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn new_workflow_passes_subcommand_name_and_yes_flag() {
    let temp = TempDir::new().expect("workspace tempdir");
    let args_path = temp.path().join("sg_args.txt");
    let script = format!(
        "#!/bin/sh\nprintf '%s\n' \"$@\" > \"{}\"\n",
        args_path.display()
    );
    let (_script_dir, script_path) = write_fake_sg(&script);

    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let result = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "workflow": "new",
            "new_subcommand": "rule",
            "new_name": "no-console-log",
            "lang": "rust"
        }),
    )
    .await
    .expect("new workflow should succeed");

    assert_eq!(result["workflow"], "new");
    assert_eq!(result["subcommand"], "rule");
    assert_eq!(result["name"], "no-console-log");

    let args = fs::read_to_string(args_path).expect("read sg args");
    let lines: Vec<&str> = args.lines().collect();
    assert!(lines.contains(&"new"), "subcommand should be new: {args}");
    assert!(lines.contains(&"rule"), "should pass rule: {args}");
    assert!(lines.contains(&"--yes"), "should pass --yes flag: {args}");
    assert!(
        lines.contains(&"no-console-log"),
        "should pass name: {args}"
    );
    assert!(lines.contains(&"--lang"), "should pass --lang flag: {args}");
    assert!(lines.contains(&"rust"), "should pass lang: {args}");
}

#[tokio::test]
#[serial]
async fn new_workflow_project_does_not_require_name() {
    let temp = TempDir::new().expect("workspace tempdir");
    let args_path = temp.path().join("sg_args.txt");
    let script = format!(
        "#!/bin/sh\nprintf '%s\n' \"$@\" > \"{}\"\n",
        args_path.display()
    );
    let (_script_dir, script_path) = write_fake_sg(&script);

    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let result = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "workflow": "new",
            "new_subcommand": "project"
        }),
    )
    .await
    .expect("new project should succeed");

    assert_eq!(result["workflow"], "new");
    assert_eq!(result["subcommand"], "project");

    let args = fs::read_to_string(args_path).expect("read sg args");
    assert!(
        args.lines().any(|line| line == "project"),
        "should pass project: {args}"
    );
    assert!(
        args.lines().any(|line| line == "--yes"),
        "should pass --yes: {args}"
    );
}

#[tokio::test]
#[serial]
async fn new_workflow_passes_config_path() {
    let temp = TempDir::new().expect("workspace tempdir");
    let args_path = temp.path().join("sg_args.txt");
    let script = format!(
        "#!/bin/sh\nprintf '%s\n' \"$@\" > \"{}\"\n",
        args_path.display()
    );
    let (_script_dir, script_path) = write_fake_sg(&script);

    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let result = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "workflow": "new",
            "new_subcommand": "test",
            "new_name": "no-eval",
            "config_path": "custom.yml"
        }),
    )
    .await
    .expect("new test with config_path should succeed");

    assert_eq!(result["workflow"], "new");

    let args = fs::read_to_string(args_path).expect("read sg args");
    assert!(
        args.lines().any(|line| line == "--config"),
        "should pass --config flag: {args}"
    );
    assert!(
        args.lines().any(|line| line == "custom.yml"),
        "should pass config path: {args}"
    );
}

// ---------------------------------------------------------------------------
// Apply workflow: execution tests
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn apply_workflow_rewrites_file_content() {
    let temp = TempDir::new().expect("workspace tempdir");
    let src_dir = temp.path().join("src");
    fs::create_dir_all(&src_dir).expect("create src dir");
    fs::write(src_dir.join("app.js"), "console.log('hello')\n").expect("write js file");

    // Fake sg that returns a rewrite match with byte offsets
    let args_path = temp.path().join("sg_args.txt");
    let script = format!(
        r##"#!/bin/sh
printf '%s\n' "$@" > "{args}"
cat <<'SGEOF'
[{{"file":"src/app.js","range":{{"start":{{"line":0,"column":0}},"end":{{"line":0,"column":20}},"byteOffset":{{"start":0,"end":20}}}},"text":"console.log('hello')","replacement":"logger.debug('hello')","replacementOffsets":{{"start":0,"end":20}}}}]
SGEOF
"##,
        args = args_path.display(),
    );
    let (_script_dir, script_path) = write_fake_sg(&script);

    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let result = execute_structural_search(
        temp.path(),
        json!({
            "action": "structural",
            "workflow": "apply",
            "pattern": "console.log($MSG)",
            "rewrite": "logger.debug($MSG)",
            "lang": "javascript",
            "path": "src"
        }),
    )
    .await
    .expect("apply workflow should succeed");

    assert_eq!(result["workflow"], "apply");
    assert_eq!(result["total_replacements"], 1);

    let modified = result["files_modified"]
        .as_array()
        .expect("files_modified array");
    assert_eq!(modified.len(), 1);

    let content = fs::read_to_string(src_dir.join("app.js")).expect("read modified file");
    assert_eq!(content, "logger.debug('hello')\n");
}

#[tokio::test]
#[serial]
async fn apply_workflow_returns_zero_replacements_for_empty_match() {
    let temp = TempDir::new().expect("workspace tempdir");
    let src_dir = temp.path().join("src");
    fs::create_dir_all(&src_dir).expect("create src dir");
    fs::write(src_dir.join("lib.rs"), "fn main() {}\n").expect("write rs file");

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
            "workflow": "apply",
            "pattern": "console.log($$$)",
            "rewrite": "logger.debug($$$)",
            "lang": "javascript",
            "path": "src"
        }),
    )
    .await
    .expect("apply with no matches should succeed");

    assert_eq!(result["workflow"], "apply");
    assert_eq!(result["total_replacements"], 0);

    let modified = result["files_modified"]
        .as_array()
        .expect("files_modified array");
    assert!(modified.is_empty());

    // Original file should be unchanged
    let content = fs::read_to_string(src_dir.join("lib.rs")).expect("read file");
    assert_eq!(content, "fn main() {}\n");
}
