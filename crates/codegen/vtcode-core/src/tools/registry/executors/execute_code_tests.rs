use serde_json::json;

use super::code_language_from_args;
use crate::exec::code_executor::Language;

#[test]
fn code_language_uses_language_field_instead_of_action() {
    assert_eq!(
        code_language_from_args(&json!({
            "action": "code",
            "language": "javascript",
        })),
        Language::JavaScript
    );
    assert_eq!(
        code_language_from_args(&json!({
            "action": "code",
            "lang": "js",
        })),
        Language::JavaScript
    );
    assert_eq!(
        code_language_from_args(&json!({
            "action": "code",
        })),
        Language::Python3
    );
}
