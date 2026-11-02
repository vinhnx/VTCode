pub(super) use super::*;
pub(super) use crate::tools::ast_grep_binary::set_ast_grep_binary_override_for_tests;
pub(super) use crate::tools::ast_grep_language::AstGrepLanguage;
pub(super) use crate::tools::grep_backend::LiteralSearchOutcome;
pub(super) use crate::tools::grep_file::LiteralSearchCandidate;
pub(super) use crate::tools::outline_search::{
    DeclarationFileRecord, DeclarationRange, DeclarationRecord, DeclarationSearchOutcome,
};
pub(super) use serde_json::json;
pub(super) use serial_test::serial;
pub(super) use std::collections::HashSet;
pub(super) use std::fs;
pub(super) use tempfile::TempDir;

fn request(value: serde_json::Value) -> CodeSearchRequest {
    serde_json::from_value(value).expect("valid request shape")
}

struct CodeSearchFixture {
    workspace: TempDir,
    fake_outline: TempDir,
    fake_outline_path: PathBuf,
}

fn code_search_fixture() -> CodeSearchFixture {
    let workspace = TempDir::new().expect("fixture workspace");
    fs::create_dir_all(workspace.path().join("src")).expect("src directory");
    fs::create_dir_all(workspace.path().join(".git")).expect("git marker");
    let rust_source = concat!(
        "fn Widget() {\n",
        "    Widget();\n",
        "}\n",
        "fn other() {\n",
        "    Widget();\n",
        "    let Widget = 1;\n",
        "    let WidgetExtra = \"Widget\";\n",
        "}\n",
        "// Widget comment\n",
        "// widget lower-case comment\n",
        "const LABEL: &str = \"Widget\";\n",
    );
    fs::write(workspace.path().join("src/widget.rs"), rust_source).expect("Rust fixture");
    fs::write(workspace.path().join("src/WidgetConfig.rs"), "pub const VALUE: &str = \"other\";\n")
        .expect("matching path fixture");
    fs::write(workspace.path().join("src/long.rs"), format!("// Widget {}\n", "こ".repeat(200)))
        .expect("long UTF-8 fixture");
    fs::write(workspace.path().join("src/widget.py"), "class Widget:\n    pass\nprint(Widget)\n")
        .expect("Python fixture");
    let bash_source = "function Widget() { echo Widget; }\n";
    fs::write(workspace.path().join("src/widget.sh"), bash_source).expect("Bash fixture");
    fs::write(workspace.path().join("src/readme.mdx"), "Widget prose\n").expect("unsupported parser fixture");
    fs::write(workspace.path().join("ignored.rs"), "fn Widget() {}\n").expect("ignored fixture");
    fs::write(workspace.path().join(".env"), "Widget=secret\n").expect("sensitive fixture");
    fs::write(workspace.path().join(".gitignore"), "ignored.rs\n").expect("ignore file");

    let declaration_end = rust_source.find("}\n").expect("function end") + 1;
    let outline_record = json!({
        "path": "src/widget.rs",
        "language": "Rust",
        "items": [{
            "name": "Widget",
            "symbolType": "function",
            "astKind": "function_item",
            "isImport": false,
            "range": {
                "byteOffset": {"start": 0, "end": declaration_end},
                "start": {"line": 0, "column": 0},
                "end": {"line": 2, "column": 1}
            }
        }]
    });
    let bash_outline_record = json!({
        "path": "src/widget.sh",
        "language": "Bash",
        "items": [{
            "name": "Widget",
            "symbolType": "function",
            "astKind": "function_definition",
            "isImport": false,
            "range": {
                "byteOffset": {"start": 0, "end": bash_source.len()},
                "start": {"line": 0, "column": 0},
                "end": {"line": 0, "column": bash_source.len()}
            }
        }]
    });
    let fake_outline = TempDir::new().expect("fake outline directory");
    let fake_outline_path = fake_outline.path().join("ast-grep");
    let script = format!(
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then printf 'ast-grep 0.44.0\\n'; exit 0; fi\nprintf '%s\\n' '{}' '{}'\n",
        outline_record.to_string().replace('\'', "'\\''"),
        bash_outline_record.to_string().replace('\'', "'\\''")
    );
    fs::write(&fake_outline_path, script).expect("fake outline executable");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&fake_outline_path)
            .expect("fake executable metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&fake_outline_path, permissions).expect("fake executable permissions");
    }

    CodeSearchFixture { workspace, fake_outline, fake_outline_path }
}

mod categories;
mod contract_and_validation;
mod definitions_scope;
mod execution;
