use serde_json::{Value, json};
use serial_test::serial;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use vtcode_core::tools::ToolRegistry;
use vtcode_core::tools::editing::patch::set_ast_grep_binary_override_for_tests;

async fn setup_registry(root: &Path) -> ToolRegistry {
    let registry = ToolRegistry::new(root.to_path_buf()).await;
    registry.initialize_async().await.unwrap();
    registry
}

fn error_message(result: &Value) -> String {
    result["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

fn create_ast_grep_stub(temp_dir: &TempDir, lines: &[&str]) -> PathBuf {
    #[cfg(windows)]
    let script_path = temp_dir.path().join("ast-grep-stub.cmd");
    #[cfg(not(windows))]
    let script_path = temp_dir.path().join("ast-grep-stub.sh");

    #[cfg(windows)]
    let body = {
        let mut script = String::from("@echo off\r\n");
        for line in lines {
            script.push_str("echo ");
            script.push_str(line);
            script.push_str("\r\n");
        }
        script.push_str("exit /b 0\r\n");
        script
    };

    #[cfg(not(windows))]
    let body = {
        let mut script = String::from("#!/bin/sh\n");
        script.push_str("printf '%s\\n'");
        for line in lines {
            script.push(' ');
            script.push('\'');
            script.push_str(line);
            script.push('\'');
        }
        script.push('\n');
        script
    };

    fs::write(&script_path, body).unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(&script_path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).unwrap();
    }

    script_path
}

#[tokio::test]
#[serial]
async fn rust_function_anchor_resolves_without_exact_context_line() {
    let temp_dir = TempDir::new().unwrap();
    let source = temp_dir.path().join("sample.rs");
    fs::write(
        &source,
        concat!(
            "pub(crate) fn first() -> usize {\n",
            "    let value = 1;\n",
            "    value\n",
            "}\n\n",
            "pub(crate) fn second() -> usize {\n",
            "    let value = 2;\n",
            "    value\n",
            "}\n"
        ),
    )
    .unwrap();

    let stub = create_ast_grep_stub(
        &temp_dir,
        &[
            r#"{"text":"pub(crate) fn second() -> usize {\n    let value = 2;\n    value\n}","range":{"start":{"line":5,"column":0},"end":{"line":8,"column":0}}}"#,
        ],
    );
    let _guard = set_ast_grep_binary_override_for_tests(Some(stub));

    let patch = r#"*** Begin Patch
*** Update File: sample.rs
@@ fn second()
-    let value = 2;
-    value
+    let value = 20;
+    value + 1
*** End Patch"#;

    let registry = setup_registry(temp_dir.path()).await;
    let result = registry
        .execute_tool("apply_patch", json!({ "patch": patch }))
        .await
        .unwrap();

    assert_eq!(result["success"].as_bool(), Some(true), "{result:?}");
    let updated = fs::read_to_string(source).unwrap();
    assert!(updated.contains("let value = 20;"));
    assert!(updated.contains("value + 1"));
}

#[tokio::test]
#[serial]
async fn typescript_method_anchor_resolves_without_exact_signature_line() {
    let temp_dir = TempDir::new().unwrap();
    let source = temp_dir.path().join("view.ts");
    fs::write(
        &source,
        concat!(
            "class View {\n",
            "    public render(): string {\n",
            "        const label = \"ok\";\n",
            "        return label;\n",
            "    }\n",
            "}\n"
        ),
    )
    .unwrap();

    let stub = create_ast_grep_stub(
        &temp_dir,
        &[
            r#"{"text":"public render(): string {\n        const label = \"ok\";\n        return label;\n    }","range":{"start":{"line":1,"column":0},"end":{"line":4,"column":0}}}"#,
        ],
    );
    let _guard = set_ast_grep_binary_override_for_tests(Some(stub));

    let patch = r#"*** Begin Patch
*** Update File: view.ts
@@ render()
-        const label = "ok";
-        return label;
+        const label = "ready";
+        return label.toUpperCase();
*** End Patch"#;

    let registry = setup_registry(temp_dir.path()).await;
    let result = registry
        .execute_tool("apply_patch", json!({ "patch": patch }))
        .await
        .unwrap();

    assert_eq!(result["success"].as_bool(), Some(true), "{result:?}");
    let updated = fs::read_to_string(source).unwrap();
    assert!(updated.contains("const label = \"ready\";"));
    assert!(updated.contains("return label.toUpperCase();"));
}

#[tokio::test]
#[serial]
async fn python_function_anchor_preserves_indentation() {
    let temp_dir = TempDir::new().unwrap();
    let source = temp_dir.path().join("greet.py");
    fs::write(
        &source,
        concat!(
            "async def greet(name: str) -> str:\n",
            "    message = f\"hi {name}\"\n",
            "    return message\n"
        ),
    )
    .unwrap();

    let stub = create_ast_grep_stub(
        &temp_dir,
        &[
            r#"{"text":"async def greet(name: str) -> str:\n    message = f\"hi {name}\"\n    return message","range":{"start":{"line":0,"column":0},"end":{"line":2,"column":0}}}"#,
        ],
    );
    let _guard = set_ast_grep_binary_override_for_tests(Some(stub));

    let patch = r#"*** Begin Patch
*** Update File: greet.py
@@ def greet(...)
-    message = f"hi {name}"
-    return message
+    message = f"hello {name}"
+    return message.upper()
*** End Patch"#;

    let registry = setup_registry(temp_dir.path()).await;
    let result = registry
        .execute_tool("apply_patch", json!({ "patch": patch }))
        .await
        .unwrap();

    assert_eq!(result["success"].as_bool(), Some(true), "{result:?}");
    let updated = fs::read_to_string(source).unwrap();
    assert!(updated.contains("    message = f\"hello {name}\""));
    assert!(updated.contains("    return message.upper()"));
}

#[tokio::test]
#[serial]
async fn go_method_anchor_resolves_receiver_signature() {
    let temp_dir = TempDir::new().unwrap();
    let source = temp_dir.path().join("server.go");
    fs::write(
        &source,
        concat!(
            "package main\n\n",
            "type server struct{}\n\n",
            "func (s *server) greet(name string) string {\n",
            "    message := \"hi \" + name\n",
            "    return message\n",
            "}\n"
        ),
    )
    .unwrap();

    let stub = create_ast_grep_stub(
        &temp_dir,
        &[
            r#"{"text":"func (s *server) greet(name string) string {\n    message := \"hi \" + name\n    return message\n}","range":{"start":{"line":4,"column":0},"end":{"line":7,"column":0}}}"#,
        ],
    );
    let _guard = set_ast_grep_binary_override_for_tests(Some(stub));

    let patch = r#"*** Begin Patch
*** Update File: server.go
@@ func greet(...)
-    message := "hi " + name
-    return message
+    message := "hello " + name
+    return message + "!"
*** End Patch"#;

    let registry = setup_registry(temp_dir.path()).await;
    let result = registry
        .execute_tool("apply_patch", json!({ "patch": patch }))
        .await
        .unwrap();

    assert_eq!(result["success"].as_bool(), Some(true), "{result:?}");
    let updated = fs::read_to_string(source).unwrap();
    assert!(updated.contains("message := \"hello \" + name"));
    assert!(updated.contains("return message + \"!\""));
}

#[tokio::test]
#[serial]
async fn semantic_anchor_fails_when_multiple_locations_match() {
    let temp_dir = TempDir::new().unwrap();
    let source = temp_dir.path().join("view.ts");
    fs::write(
        &source,
        concat!(
            "class Primary {\n",
            "    render(): string {\n",
            "        const label = \"ok\";\n",
            "        return label;\n",
            "    }\n",
            "}\n\n",
            "class Secondary {\n",
            "    render(): string {\n",
            "        const label = \"ok\";\n",
            "        return label;\n",
            "    }\n",
            "}\n"
        ),
    )
    .unwrap();

    let stub = create_ast_grep_stub(
        &temp_dir,
        &[
            r#"{"text":"render(): string {\n        const label = \"ok\";\n        return label;\n    }","range":{"start":{"line":1,"column":0},"end":{"line":4,"column":0}}}"#,
            r#"{"text":"render(): string {\n        const label = \"ok\";\n        return label;\n    }","range":{"start":{"line":8,"column":0},"end":{"line":11,"column":0}}}"#,
        ],
    );
    let _guard = set_ast_grep_binary_override_for_tests(Some(stub));

    let patch = r#"*** Begin Patch
*** Update File: view.ts
@@ render()
-        const label = "ok";
-        return label;
+        const label = "ready";
+        return label.toUpperCase();
*** End Patch"#;

    let registry = setup_registry(temp_dir.path()).await;
    let result = registry
        .execute_tool("apply_patch", json!({ "patch": patch }))
        .await
        .unwrap();

    assert!(result["error"].is_object(), "{result:?}");
    assert!(error_message(&result).contains("matched 2 possible locations"));
}

#[tokio::test]
#[serial]
async fn semantic_anchor_fails_for_unsupported_language() {
    let temp_dir = TempDir::new().unwrap();
    let source = temp_dir.path().join("service.rb");
    fs::write(
        &source,
        "class Service\n  def greet(name)\n    \"hi #{name}\"\n  end\nend\n",
    )
    .unwrap();

    let patch = r#"*** Begin Patch
*** Update File: service.rb
@@ def greet(...)
-    "hi #{name}"
+    "hello #{name}"
*** End Patch"#;

    let registry = setup_registry(temp_dir.path()).await;
    let result = registry
        .execute_tool("apply_patch", json!({ "patch": patch }))
        .await
        .unwrap();

    assert!(result["error"].is_object(), "{result:?}");
    assert!(error_message(&result).contains("unsupported language"));
}

#[tokio::test]
#[serial]
async fn semantic_anchor_fails_when_ast_grep_is_unavailable() {
    let temp_dir = TempDir::new().unwrap();
    let source = temp_dir.path().join("sample.rs");
    fs::write(
        &source,
        concat!(
            "pub(crate) fn second() -> usize {\n",
            "    let value = 2;\n",
            "    value\n",
            "}\n"
        ),
    )
    .unwrap();

    let _guard = set_ast_grep_binary_override_for_tests(None);

    let patch = r#"*** Begin Patch
*** Update File: sample.rs
@@ fn second()
-    let value = 2;
-    value
+    let value = 20;
+    value + 1
*** End Patch"#;

    let registry = setup_registry(temp_dir.path()).await;
    let result = registry
        .execute_tool("apply_patch", json!({ "patch": patch }))
        .await
        .unwrap();

    assert!(result["error"].is_object(), "{result:?}");
    assert!(error_message(&result).contains("ast-grep is not available"));
}

#[tokio::test]
#[serial]
async fn semantic_anchor_fails_when_candidate_region_is_not_safe() {
    let temp_dir = TempDir::new().unwrap();
    let source = temp_dir.path().join("sample.rs");
    fs::write(
        &source,
        concat!(
            "pub(crate) fn second() -> usize {\n",
            "    let value = 3;\n",
            "    value\n",
            "}\n"
        ),
    )
    .unwrap();

    let stub = create_ast_grep_stub(
        &temp_dir,
        &[
            r#"{"text":"pub(crate) fn second() -> usize {\n    let value = 3;\n    value\n}","range":{"start":{"line":0,"column":0},"end":{"line":3,"column":0}}}"#,
        ],
    );
    let _guard = set_ast_grep_binary_override_for_tests(Some(stub));

    let patch = r#"*** Begin Patch
*** Update File: sample.rs
@@ fn second()
-    let value = 2;
-    value
+    let value = 20;
+    value + 1
*** End Patch"#;

    let registry = setup_registry(temp_dir.path()).await;
    let result = registry
        .execute_tool("apply_patch", json!({ "patch": patch }))
        .await
        .unwrap();

    assert!(result["error"].is_object(), "{result:?}");
    assert!(error_message(&result).contains("removal/context lines were not found safely"));
}

#[tokio::test]
#[serial]
async fn numeric_hunk_headers_do_not_trigger_semantic_fallback() {
    let temp_dir = TempDir::new().unwrap();
    let source = temp_dir.path().join("sample.rs");
    fs::write(
        &source,
        concat!(
            "pub(crate) fn second() -> usize {\n",
            "    let value = 3;\n",
            "    value\n",
            "}\n"
        ),
    )
    .unwrap();

    let patch = r#"*** Begin Patch
*** Update File: sample.rs
@@ -2,2 +2,2 @@
-    let value = 2;
-    value
+    let value = 20;
+    value + 1
*** End Patch"#;

    let registry = setup_registry(temp_dir.path()).await;
    let result = registry
        .execute_tool("apply_patch", json!({ "patch": patch }))
        .await
        .unwrap();

    assert!(result["error"].is_object(), "{result:?}");
    let message = error_message(&result);
    assert!(message.contains("failed to locate expected lines"));
    assert!(!message.contains("semantic anchor"), "{message}");
    assert!(!message.contains("usable symbol"), "{message}");
}
