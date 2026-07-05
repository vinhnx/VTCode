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
    // `full` view must preserve the raw zero-based `range`, the derived
    // 1-based `lineRange` (for `unified_file read` pagination), and `astKind`
    // for both items and members — the contract advertised these but they were
    // previously discarded during deserialization.
    // alpha: range lines 0..=0 (0-based) -> lineRange {1, 1}.
    assert_eq!(items[0]["astKind"], "function_item");
    assert_eq!(items[0]["lineRange"], json!({"start": 1, "end": 1}));
    assert_eq!(items[0]["range"]["start"]["line"], 0);
    // Foo: range lines 1..=3 (0-based) -> lineRange {2, 4}.
    assert_eq!(items[1]["astKind"], "struct_item");
    assert_eq!(items[1]["lineRange"], json!({"start": 2, "end": 4}));
    assert_eq!(items[1]["range"]["byteOffset"]["start"], 11);
    // member x: range line 2 (0-based) -> lineRange {3, 3}; astKind preserved.
    assert_eq!(members[0]["astKind"], "field_definition");
    assert_eq!(members[0]["lineRange"], json!({"start": 3, "end": 3}));
    assert_eq!(members[0]["isPublic"], true);
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
    // `by_kind` counts symbols (not files/groups), so per-kind counts sum to
    // `total_symbols`.
    let by_kind_sum: i64 = summary["by_kind"]
        .as_object()
        .expect("by_kind object")
        .values()
        .map(|v| v.as_i64().unwrap_or(0))
        .sum();
    assert_eq!(by_kind_sum, 3, "sum(by_kind) must equal total_symbols");
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
    // Newly added extensions for the tolerant directory fallback.
    assert!(has_source_extension("Program.cs"));
    assert!(has_source_extension("lib.ex"));
    assert!(has_source_extension("view.zig"));
    assert!(has_source_extension("schema.graphql"));
    assert!(has_source_extension("Component.vue"));
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

// ── Summary cap: large symbol counts ───────────────────────────────────

/// Build a one-file NDJSON stream with `n` function symbols so the directory
/// `summary.all_symbols` cap (`MAX_SUMMARY_SYMBOLS`) can be exercised without
/// a giant literal fixture.
fn stream_with_n_symbols(n: usize) -> String {
    let mut items = String::new();
    for i in 0..n {
        let line = i;
        items.push_str(&format!(
            r#"{{"role":"item","symbolType":"function","name":"f{i}","range":{{"byteOffset":{{"start":{line},"end":{line}}},"start":{{"line":{line},"column":0}},"end":{{"line":{line},"column":2}}}},"signature":"fn f{i}()","astKind":"function_item","isImport":false,"isExported":false,"members":[]}}"#
        ));
        if i + 1 < n {
            items.push(',');
        }
    }
    format!(r#"{{"path":"src/big.rs","language":"Rust","items":[{items}]}}"#)
}

/// Build an N-file NDJSON stream (one function per file) for the large-
/// directory `view=full` auto-downgrade test.
fn stream_with_n_files(n: usize) -> String {
    let mut out = String::new();
    for i in 0..n {
        out.push_str(&format!(
            r#"{{"path":"src/m{i}.rs","language":"Rust","items":[{{"role":"item","symbolType":"function","name":"m{i}","range":{{"byteOffset":{{"start":0,"end":2}},"start":{{"line":0,"column":0}},"end":{{"line":0,"column":2}}}},"signature":"fn m{i}()","astKind":"function_item","isImport":false,"isExported":true,"members":[]}}]}}"#
        ));
        out.push('\n');
    }
    out
}

#[tokio::test]
#[serial]
async fn outline_directory_caps_all_symbols_and_marks_truncated() {
    // 250 symbols exceeds MAX_SUMMARY_SYMBOLS (200): the visible array is
    // capped, `total_symbols` stays accurate, and `truncated`/`visible_symbols`
    // are set with a narrowed `next_action`.
    let (_script_dir, script_path) = write_fake_sg(&stream_with_n_symbols(250));
    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let temp = workspace_with_source();

    let result = execute_outline_search(temp.path(), json!({"action": "outline", "path": "src"}))
        .await
        .expect("capped directory outline ok");

    let summary = result.get("summary").expect("summary block");
    assert_eq!(summary["total_symbols"], json!(250), "true count preserved");
    assert_eq!(summary["truncated"], json!(true));
    assert_eq!(summary["visible_symbols"], json!(200));
    let all_symbols = summary["all_symbols"]
        .as_array()
        .expect("all_symbols array");
    assert_eq!(all_symbols.len(), 200, "visible array capped");
    assert_eq!(all_symbols[0]["name"], "f0");
    assert_eq!(all_symbols[199]["name"], "f199");
    assert!(
        summary["next_action"]
            .as_str()
            .is_some_and(|s| s.contains("Narrow with `type`")),
        "truncated next_action should guide narrowing, got: {:?}",
        summary["next_action"]
    );
    // `by_kind` must still sum to the true total (250 functions counted).
    let by_kind_sum: i64 = summary["by_kind"]
        .as_object()
        .expect("by_kind object")
        .values()
        .map(|v| v.as_i64().unwrap_or(0))
        .sum();
    assert_eq!(by_kind_sum, 250);
}

#[tokio::test]
#[serial]
async fn outline_directory_not_truncated_under_cap() {
    // 50 symbols is under the cap: no `truncated`/`visible_symbols` fields.
    let (_script_dir, script_path) = write_fake_sg(&stream_with_n_symbols(50));
    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let temp = workspace_with_source();

    let result = execute_outline_search(temp.path(), json!({"action": "outline", "path": "src"}))
        .await
        .expect("directory outline ok");

    let summary = result.get("summary").expect("summary block");
    assert_eq!(summary["total_symbols"], json!(50));
    assert!(
        summary.get("truncated").is_none(),
        "no truncated flag under the cap"
    );
    assert_eq!(
        summary["all_symbols"]
            .as_array()
            .expect("all_symbols")
            .len(),
        50
    );
}

// ── Large-directory `view=full` auto-downgrade ─────────────────────────

#[tokio::test]
#[serial]
async fn outline_large_directory_full_view_auto_downgrades_to_names() {
    // >LARGE_DIR_FULL_VIEW_THRESHOLD (20) files with an explicit `view=full`
    // should auto-downgrade to `names` and emit a hint explaining it.
    let (_script_dir, script_path) = write_fake_sg(&stream_with_n_files(25));
    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let temp = workspace_with_source();

    let result = execute_outline_search(
        temp.path(),
        json!({"action": "outline", "path": "src", "view": "full"}),
    )
    .await
    .expect("downgrade outline ok");

    assert_eq!(result["view"], "names", "full should downgrade to names");
    let hints = result["hints"].as_array().expect("hints array");
    assert!(
        hints
            .iter()
            .any(|h| h.as_str().is_some_and(|s| s.contains("Auto-downgraded"))),
        "should hint about the auto-downgrade, got: {hints:?}"
    );
    assert!(result.get("summary").is_some(), "summary still attached");
}

// ── Hints for grep-only params ─────────────────────────────────────────

#[tokio::test]
#[serial]
async fn outline_emits_hint_for_ignored_grep_only_params() {
    let (_script_dir, script_path) = write_fake_sg(TWO_FILE_STREAM);
    let _override = set_ast_grep_binary_override_for_tests(Some(script_path));
    let temp = workspace_with_source();

    let result = execute_outline_search(
        temp.path(),
        json!({
            "action": "outline",
            "path": "src",
            "context_lines": 3,
            "case_sensitive": true,
        }),
    )
    .await
    .expect("outline should succeed even with ignored grep params");

    let hints = result["hints"].as_array().expect("hints array");
    assert!(
        hints
            .iter()
            .any(|h| h.as_str().is_some_and(|s| s.contains("`context_lines`")
                && s.contains("`case_sensitive`")
                && s.contains("not used by outline"))),
        "should list the ignored grep-only params, got: {hints:?}"
    );
}
