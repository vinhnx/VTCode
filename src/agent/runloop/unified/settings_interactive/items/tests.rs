use super::advanced_label;

#[test]
fn advanced_label_uses_quoted_map_key() {
    assert_eq!(advanced_label(r#"mcp.providers[0].env["A.B"]"#), "A B");
}
