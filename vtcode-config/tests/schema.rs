#![cfg(feature = "schema")]

use vtcode_config::{vtcode_config_schema, vtcode_config_schema_json, vtcode_config_schema_pretty};

#[test]
fn schema_helpers_produce_consistent_output() {
    let root = vtcode_config_schema();
    let as_json = vtcode_config_schema_json().expect("schema to serialize to JSON value");
    assert!(
        as_json.is_object(),
        "expected JSON schema representation to be an object"
    );

    let as_string = vtcode_config_schema_pretty().expect("schema to serialize to JSON string");
    assert!(
        as_string.contains("\"title\": \"VTCodeConfig\""),
        "pretty schema output should mention the root configuration type"
    );

    // Ensure both helper paths describe the same schema title.
    let title_from_value = as_json
        .get("title")
        .and_then(|value| value.as_str())
        .expect("schema JSON should include a title");
    let title_from_root = root
        .get("title")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    assert_eq!(title_from_value, title_from_root);
}
