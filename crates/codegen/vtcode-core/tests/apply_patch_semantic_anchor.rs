#![allow(missing_docs, clippy::expect_used, clippy::unwrap_used, reason = "Test assertions.")]
//! Regression coverage for the native semantic `@@` anchor of `apply_patch`.
//!
//! The anchor names a symbol, and the hunk is applied relative to where that
//! symbol is declared. These tests pin the behaviour that the removed
//! `ast-grep` backend used to provide.

use serde_json::{Value, json};
use std::fs;
use tempfile::TempDir;
use vtcode_core::tools::ToolRegistry;

async fn setup_registry(root: &std::path::Path) -> ToolRegistry {
    let registry = ToolRegistry::new(root.to_path_buf()).await;
    registry.initialize_async().await.unwrap();
    registry.allow_all_tools().await.unwrap();
    registry
}

fn error_message(result: &Value) -> String {
    let error = result.get("error");
    let message = error
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let original = error
        .and_then(|error| error.get("original_error"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if original.is_empty() {
        message.to_string()
    } else {
        format!("{message} {original}")
    }
}

/// Two functions that share the same body, so the anchor alone decides where
/// the hunk lands.
const SOURCE: &str = "\
fn deploy() {
    let steps = 1;
    run(steps);
}

fn rollback() {
    let steps = 1;
    run(steps);
}
";

#[tokio::test]
async fn semantic_anchor_applies_the_hunk_at_the_named_declaration() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("lib.rs");
    fs::write(&file_path, SOURCE).unwrap();

    let patch = r#"*** Begin Patch
*** Update File: lib.rs
@@ fn deploy
-    let steps = 1;
+    let steps = 2;
     run(steps);
*** End Patch"#;

    let registry = setup_registry(temp_dir.path()).await;
    let result = registry.execute_tool("apply_patch", json!({ "input": patch })).await.unwrap();
    assert!(result["success"].as_bool().unwrap_or(false), "{result}");

    let updated = fs::read_to_string(&file_path).unwrap();
    assert_eq!(
        updated,
        "\
fn deploy() {
    let steps = 2;
    run(steps);
}

fn rollback() {
    let steps = 1;
    run(steps);
}
",
        "the hunk must land in the anchored declaration only"
    );
}

#[tokio::test]
async fn semantic_anchor_resolves_a_multi_line_hunk_inside_the_declaration() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("lib.rs");
    fs::write(&file_path, SOURCE).unwrap();

    // The removal and the context span several lines, so the anchor cannot be
    // resolved by matching the declared name alone.
    let patch = r#"*** Begin Patch
*** Update File: lib.rs
@@ fn rollback
-    let steps = 1;
-    run(steps);
+    let steps = 3;
+    run(steps);
*** End Patch"#;

    let registry = setup_registry(temp_dir.path()).await;
    let result = registry.execute_tool("apply_patch", json!({ "input": patch })).await.unwrap();
    assert!(result["success"].as_bool().unwrap_or(false), "{result}");

    let updated = fs::read_to_string(&file_path).unwrap();
    assert_eq!(
        updated,
        "\
fn deploy() {
    let steps = 1;
    run(steps);
}

fn rollback() {
    let steps = 3;
    run(steps);
}
",
        "the multi-line hunk must land in the anchored declaration"
    );
}

#[tokio::test]
async fn semantic_anchor_reports_an_ambiguous_declaration() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("lib.rs");
    // Two declarations with the same name *and* the same body: the hunk fits
    // both, so the anchor cannot say which one is meant.
    let original = "\
fn deploy() {
    let steps = 1;
}

fn deploy() {
    let steps = 1;
}
";
    fs::write(&file_path, original).unwrap();

    let patch = r#"*** Begin Patch
*** Update File: lib.rs
@@ fn deploy
-    let steps = 1;
+    let steps = 9;
*** End Patch"#;

    let registry = setup_registry(temp_dir.path()).await;
    let result = registry.execute_tool("apply_patch", json!({ "input": patch })).await.unwrap();
    let message = error_message(&result);
    assert!(message.contains("possible locations"), "{message}");
    assert_eq!(fs::read_to_string(&file_path).unwrap(), original, "an ambiguous anchor must not change the file");
}

#[tokio::test]
async fn semantic_anchor_reports_an_unknown_symbol() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("lib.rs");
    fs::write(&file_path, SOURCE).unwrap();

    let patch = r#"*** Begin Patch
*** Update File: lib.rs
@@ fn missing
-    let steps = 1;
+    let steps = 9;
*** End Patch"#;

    let registry = setup_registry(temp_dir.path()).await;
    let result = registry.execute_tool("apply_patch", json!({ "input": patch })).await.unwrap();
    let message = error_message(&result);
    assert!(message.contains("could not be resolved safely"), "{message}");
    assert!(message.contains("no structural candidates"), "{message}");
    assert_eq!(fs::read_to_string(&file_path).unwrap(), SOURCE);
}

#[tokio::test]
async fn semantic_anchor_falls_back_to_exact_text_for_unsupported_languages() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("notes.md");
    let original = "# Notes\n\nsome prose here\n";
    fs::write(&file_path, original).unwrap();

    // Markdown has no declaration support, so the anchor falls back to an
    // exact text search.
    let patch = r#"*** Begin Patch
*** Update File: notes.md
@@ some prose here
-some prose here
+some edited prose
*** End Patch"#;

    let registry = setup_registry(temp_dir.path()).await;
    let result = registry.execute_tool("apply_patch", json!({ "input": patch })).await.unwrap();
    assert!(result["success"].as_bool().unwrap_or(false), "{result}");
    assert_eq!(fs::read_to_string(&file_path).unwrap(), "# Notes\n\nsome edited prose\n");
}
