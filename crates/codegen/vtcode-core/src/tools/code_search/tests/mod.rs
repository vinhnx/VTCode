pub(super) use super::*;
pub(super) use crate::tools::code_language::CodeLanguage;
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

    CodeSearchFixture { workspace }
}

mod categories;
mod contract_and_validation;
mod definitions_scope;
mod execution;
