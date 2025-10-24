use serde_json::json;
use vtcode_core::tools::types::{EditInput, WriteInput};

#[test]
fn write_input_supports_alias_fields() {
    let payload = json!({
        "file_path": "src/main.rs",
        "contents": "fn main() {}\n",
        "write_mode": "append"
    });

    let parsed: WriteInput =
        serde_json::from_value(payload).expect("write_input should parse aliases");
    assert_eq!(parsed.path, "src/main.rs");
    assert_eq!(parsed.content, "fn main() {}\n");
    assert_eq!(parsed.mode, "append");
}

#[test]
fn edit_input_supports_alias_fields() {
    let payload = json!({
        "filepath": "lib.rs",
        "old_text": "old",
        "new_text": "new"
    });

    let parsed: EditInput =
        serde_json::from_value(payload).expect("edit_input should parse aliases");
    assert_eq!(parsed.path, "lib.rs");
    assert_eq!(parsed.old_str, "old");
    assert_eq!(parsed.new_str, "new");
}
