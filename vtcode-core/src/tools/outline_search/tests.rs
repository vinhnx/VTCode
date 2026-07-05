use super::{
    OutlineItems, OutlineRequest, OutlineView, execute_outline_search, has_source_extension,
    strip_extension,
};
use crate::tools::ast_grep_binary::AST_GREP_INSTALL_COMMAND;
use crate::tools::editing::patch::set_ast_grep_binary_override_for_tests;
use serde_json::json;
use serial_test::serial;
use std::{fs, path::PathBuf};
use tempfile::TempDir;

/// Write a fake `ast-grep` executable that ignores its args and writes the
/// given body to stdout, exiting 0. The body is staged in a sidecar file so
/// real newlines are preserved (embedding it via `printf` mangles NDJSON).
fn write_fake_sg(stdout_body: &str) -> (TempDir, PathBuf) {
    let script_dir = TempDir::new().expect("script tempdir");
    let output_path = script_dir.path().join("output.txt");
    fs::write(&output_path, stdout_body).expect("write output");
    let script_path = script_dir.path().join("sg");
    let script = format!("#!/bin/sh\ncat {:?}\n", output_path.as_os_str());
    fs::write(&script_path, script).expect("write fake sg");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&script_path).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script_path, perms).expect("chmod");
    }
    (script_dir, script_path)
}

/// Fake `ast-grep` that writes the given message to stderr and exits 1, to
/// exercise the failure path.
fn write_failing_sg(stderr_body: &str) -> (TempDir, PathBuf) {
    let script_dir = TempDir::new().expect("script tempdir");
    let err_path = script_dir.path().join("err.txt");
    fs::write(&err_path, stderr_body).expect("write err");
    let script_path = script_dir.path().join("sg");
    let script = format!("#!/bin/sh\ncat {:?} 1>&2\nexit 1\n", err_path.as_os_str());
    fs::write(&script_path, script).expect("write failing sg");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&script_path).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script_path, perms).expect("chmod");
    }
    (script_dir, script_path)
}

/// Two-file NDJSON stream mimicking `ast-grep outline --json=stream` output.
const TWO_FILE_STREAM: &str = r#"{"path":"src/lib.rs","language":"Rust","items":[{"role":"item","symbolType":"function","name":"alpha","range":{"byteOffset":{"start":0,"end":10},"start":{"line":0,"column":0},"end":{"line":0,"column":10}},"signature":"pub fn alpha()","astKind":"function_item","isImport":false,"isExported":true,"members":[]},{"role":"item","symbolType":"struct","name":"Foo","range":{"byteOffset":{"start":11,"end":40},"start":{"line":1,"column":0},"end":{"line":3,"column":1}},"signature":"pub struct Foo","astKind":"struct_item","isImport":false,"isExported":true,"members":[{"role":"member","symbolType":"field","name":"x","range":{"byteOffset":{"start":22,"end":28},"start":{"line":2,"column":4},"end":{"line":2,"column":10}},"signature":"","astKind":"field_definition","isPublic":true}]}]}
{"path":"src/util.rs","language":"Rust","items":[{"role":"item","symbolType":"function","name":"beta","range":{"byteOffset":{"start":0,"end":12},"start":{"line":0,"column":0},"end":{"line":0,"column":12}},"signature":"fn beta()","astKind":"function_item","isImport":false,"isExported":false,"members":[]}]}
"#;

fn workspace_with_source() -> TempDir {
    let temp = TempDir::new().expect("workspace tempdir");
    let src_dir = temp.path().join("src");
    fs::create_dir_all(&src_dir).expect("create src dir");
    fs::write(src_dir.join("lib.rs"), "pub fn alpha() {}\n").expect("write lib.rs");
    fs::write(src_dir.join("util.rs"), "fn beta() {}\n").expect("write util.rs");
    temp
}

#[test]
fn outline_request_defaults_path_and_view() {
    let req = OutlineRequest::from_args(&json!({"action": "outline"})).expect("valid");
    assert_eq!(req.path, ".");
    assert_eq!(req.view, OutlineView::Digest);
    assert_eq!(req.items, OutlineItems::Auto);
    assert!(!req.pub_members);
    assert!(!req.follow);
}

#[test]
fn outline_request_parses_type_array_into_comma_string() {
    let req = OutlineRequest::from_args(&json!({
        "action": "outline",
        "type": ["function", "struct"],
        "view": "names",
    }))
    .expect("valid");
    assert_eq!(req.type_filter.as_deref(), Some("function,struct"));
    assert_eq!(req.view, OutlineView::Names);
}

#[test]
fn outline_request_rejects_invalid_view() {
    let err = OutlineRequest::from_args(&json!({"action": "outline", "view": "bogus"}))
        .expect_err("invalid view");
    let text = err.to_string();
    assert!(text.contains("view"), "{text}");
    assert!(text.contains("bogus"), "{text}");
}

#[test]
fn outline_request_rejects_invalid_items() {
    let err = OutlineRequest::from_args(&json!({"action": "outline", "items": "nope"}))
        .expect_err("invalid items");
    let text = err.to_string();
    assert!(text.contains("items"), "{text}");
}

#[tokio::test]
#[serial]
async fn outline_reports_missing_ast_grep() {
    let temp = workspace_with_source();
    let _override = set_ast_grep_binary_override_for_tests(None);

    let err = execute_outline_search(
        temp.path(),
        json!({"action": "outline", "path": "src/lib.rs"}),
    )
    .await
    .expect_err("missing ast-grep");

    let text = err.to_string();
    assert!(text.contains("ast-grep"), "{text}");
    assert!(text.contains(AST_GREP_INSTALL_COMMAND), "{text}");
}

#[tokio::test]
#[serial]
async fn outline_reports_missing_path() {
    // Use a fake succeeding binary so the path-resolution error is reached.
    let (_script_dir, script_path) = write_fake_sg("[]");
    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let temp = TempDir::new().expect("workspace tempdir");

    let err = execute_outline_search(
        temp.path(),
        json!({"action": "outline", "path": "does/not/exist.rs"}),
    )
    .await
    .expect_err("missing path");

    let text = err.to_string();
    assert!(text.contains("does/not/exist.rs"), "{text}");
}

#[tokio::test]
#[serial]
async fn outline_digest_groups_symbols_by_kind() {
    let (_script_dir, script_path) = write_fake_sg(TWO_FILE_STREAM);
    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let temp = workspace_with_source();

    let result = execute_outline_search(
        temp.path(),
        json!({"action": "outline", "path": "src/lib.rs", "view": "digest"}),
    )
    .await
    .expect("digest ok");

    assert_eq!(result["view"], "digest");
    let files = result["files"].as_array().expect("files array");
    assert_eq!(files.len(), 2, "two file records from the stream");

    // src/lib.rs has function + struct groups; src/util.rs has only function.
    let lib = &files[0];
    assert_eq!(lib["path"], "src/lib.rs");
    let groups = lib["groups"].as_array().expect("groups array");
    // BTreeMap ordering: "function" < "struct".
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0]["kind"], "function");
    assert_eq!(groups[0]["names"], json!(["alpha"]));
    assert_eq!(groups[1]["kind"], "struct");
    assert_eq!(groups[1]["names"], json!(["Foo"]));
    // digest includes members (flat across the group).
    assert_eq!(groups[1]["members"], json!(["x"]));
    // function group has no members -> empty array.
    assert_eq!(groups[0]["members"], json!([]));
}

#[tokio::test]
#[serial]
async fn outline_names_drops_members() {
    let (_script_dir, script_path) = write_fake_sg(TWO_FILE_STREAM);
    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let temp = workspace_with_source();

    let result = execute_outline_search(
        temp.path(),
        json!({"action": "outline", "path": "src/lib.rs", "view": "names"}),
    )
    .await
    .expect("names ok");

    assert_eq!(result["view"], "names");
    let lib = &result["files"][0];
    let groups = lib["groups"].as_array().expect("groups array");
    assert_eq!(groups[1]["kind"], "struct");
    assert_eq!(groups[1]["names"], json!(["Foo"]));
    assert!(
        groups[1].get("members").is_none(),
        "names view must not include members"
    );
}

#[tokio::test]
#[serial]
async fn outline_full_passes_through_records() {
    let (_script_dir, script_path) = write_fake_sg(TWO_FILE_STREAM);
    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let temp = workspace_with_source();

    let result = execute_outline_search(
        temp.path(),
        json!({"action": "outline", "path": "src/lib.rs", "view": "full"}),
    )
    .await
    .expect("full ok");

    assert_eq!(result["view"], "full");
    let lib = &result["files"][0];
    assert_eq!(lib["path"], "src/lib.rs");
    let items = lib["items"].as_array().expect("items array");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["kind"], "function");
    assert_eq!(items[0]["name"], "alpha");
    assert_eq!(items[1]["kind"], "struct");
    // full view preserves the nested member record.
    let members = items[1]["members"].as_array().expect("members array");
    assert_eq!(members.len(), 1);
    assert_eq!(members[0]["name"], "x");
    assert_eq!(members[0]["kind"], "field");
}

#[tokio::test]
#[serial]
async fn outline_surfaces_ast_grep_failure_as_structured_error() {
    let (_script_dir, script_path) = write_failing_sg("language not supported");
    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let temp = workspace_with_source();

    let err = execute_outline_search(
        temp.path(),
        json!({"action": "outline", "path": "src/lib.rs"}),
    )
    .await
    .expect_err("ast-grep failure");

    let text = err.to_string();
    assert!(text.contains("outline"), "{text}");
    assert!(text.contains("failed"), "{text}");
    assert!(text.contains("language not supported"), "{text}");
}

#[tokio::test]
#[serial]
async fn outline_handles_empty_stream_gracefully() {
    let (_script_dir, script_path) = write_fake_sg("");
    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let temp = workspace_with_source();

    let result = execute_outline_search(
        temp.path(),
        json!({"action": "outline", "path": "src/lib.rs"}),
    )
    .await
    .expect("empty stream ok");

    assert_eq!(result["view"], "digest");
    assert_eq!(result["files"].as_array().expect("files array").len(), 0);
}

#[tokio::test]
#[serial]
async fn outline_directory_emits_summary_block() {
    let (_script_dir, script_path) = write_fake_sg(TWO_FILE_STREAM);
    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let temp = workspace_with_source();

    // Point at the `src` *directory* (not a single file). The summary block
    // should appear with aggregated counts and a flat symbol list.
    let result = execute_outline_search(temp.path(), json!({"action": "outline", "path": "src"}))
        .await
        .expect("directory outline ok");

    assert_eq!(
        result["view"], "names",
        "directory should default to names view"
    );
    let summary = result.get("summary").expect("summary block on directory");
    assert_eq!(summary["is_directory"], json!(true));
    assert_eq!(summary["file_count"], json!(2));
    assert_eq!(summary["total_symbols"], json!(3));
    assert_eq!(summary["by_lang"]["Rust"], json!(2));
    assert_eq!(summary["by_kind"]["function"], json!(2));
    assert_eq!(summary["by_kind"]["struct"], json!(1));
    let all_symbols = summary["all_symbols"]
        .as_array()
        .expect("all_symbols array");
    assert_eq!(all_symbols.len(), 3);
    assert_eq!(all_symbols[0]["kind"], "function");
    assert_eq!(all_symbols[0]["name"], "alpha");
    assert!(
        summary["next_action"]
            .as_str()
            .is_some_and(|s| s.contains("Synthesize your final answer")),
        "directory summary should include next_action"
    );
}

#[tokio::test]
#[serial]
async fn outline_directory_respects_explicit_view() {
    let (_script_dir, script_path) = write_fake_sg(TWO_FILE_STREAM);
    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let temp = workspace_with_source();

    // Explicit `view=full` on a directory should be respected.
    let result = execute_outline_search(
        temp.path(),
        json!({"action": "outline", "path": "src", "view": "full"}),
    )
    .await
    .expect("directory outline with explicit view");

    assert_eq!(result["view"], "full");
    assert!(
        result.get("summary").is_some(),
        "summary should still be attached for explicit views"
    );
}

// ── Tolerant path fallback: .rs suffix on directory ───────────────────

#[test]
fn has_source_extension_detects_common_extensions() {
    assert!(has_source_extension("src/registry.rs"));
    assert!(has_source_extension("main.py"));
    assert!(has_source_extension("app.tsx"));
    assert!(has_source_extension("App.JAVA")); // case-insensitive
    assert!(!has_source_extension("src/registry"));
    assert!(!has_source_extension("Cargo.toml"));
    assert!(!has_source_extension("README.md"));
}

#[test]
fn strip_extension_removes_final_extension() {
    assert_eq!(strip_extension("foo/bar.rs"), "foo/bar");
    assert_eq!(strip_extension("main.py"), "main");
    assert_eq!(strip_extension("no_extension"), "no_extension");
    assert_eq!(strip_extension("path/to/dir/"), "path/to/dir/");
}

#[tokio::test]
#[serial]
async fn outline_tolerates_rs_suffix_on_directory() {
    // The agent passes `path: "src.rs"` when `src/` is a directory.
    // The tolerant fallback should strip `.rs` and find `src/`.
    let (_script_dir, script_path) = write_fake_sg(TWO_FILE_STREAM);
    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let temp = workspace_with_source();

    let result =
        execute_outline_search(temp.path(), json!({"action": "outline", "path": "src.rs"}))
            .await
            .expect("tolerant fallback should resolve src.rs → src/");

    // Should succeed and produce a names view (directory default)
    assert_eq!(result["view"], "names");
    assert!(
        result.get("summary").is_some(),
        "directory summary expected"
    );
}

// ── Hints for unrecognized parameters ──────────────────────────────────

#[tokio::test]
#[serial]
async fn outline_emits_hint_for_ignored_format() {
    let (_script_dir, script_path) = write_fake_sg(TWO_FILE_STREAM);
    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let temp = workspace_with_source();

    let result = execute_outline_search(
        temp.path(),
        json!({"action": "outline", "path": "src", "format": "github"}),
    )
    .await
    .expect("outline should succeed even with ignored format");

    let hints = result["hints"].as_array().expect("hints array");
    assert!(
        hints
            .iter()
            .any(|h| h.as_str().is_some_and(|s| s.contains("format"))),
        "should hint that format was ignored"
    );
}

#[tokio::test]
#[serial]
async fn outline_emits_hint_for_ignored_max_results() {
    let (_script_dir, script_path) = write_fake_sg(TWO_FILE_STREAM);
    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let temp = workspace_with_source();

    let result = execute_outline_search(
        temp.path(),
        json!({"action": "outline", "path": "src", "max_results": 80}),
    )
    .await
    .expect("outline should succeed even with ignored max_results");

    let hints = result["hints"].as_array().expect("hints array");
    assert!(
        hints
            .iter()
            .any(|h| h.as_str().is_some_and(|s| s.contains("max_results"))),
        "should hint that max_results was ignored"
    );
}

#[tokio::test]
#[serial]
async fn outline_no_hints_when_all_params_recognized() {
    let (_script_dir, script_path) = write_fake_sg(TWO_FILE_STREAM);
    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let temp = workspace_with_source();

    let result = execute_outline_search(
        temp.path(),
        json!({"action": "outline", "path": "src", "view": "names"}),
    )
    .await
    .expect("outline should succeed");

    assert!(
        result.get("hints").is_none(),
        "no hints should be emitted when all params are recognized"
    );
}
