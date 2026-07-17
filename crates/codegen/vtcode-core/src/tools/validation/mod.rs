pub mod commands;
pub mod paths;
pub mod unified_path;

use jsonschema::ValidationError;
use jsonschema::error::{TypeKind, ValidationErrorKind};
use serde_json::Value;

/// Extract a condensed representation of a JSON Schema for error hints.
///
/// Returns a JSON object with:
/// - `required`: array of required field names
/// - `properties`: object mapping field name -> its declared `type` (or `"any"` if absent)
///
/// This is intentionally compact so it can be included in validation error
/// payloads without bloating the context.
pub fn condensed_schema_hint(schema: &Value) -> Option<Value> {
    let properties = schema.get("properties").and_then(Value::as_object)?;
    let required: Vec<Value> =
        schema.get("required").and_then(Value::as_array).cloned().unwrap_or_default();

    let mut prop_types = serde_json::Map::new();
    for (name, def) in properties {
        let type_str = def.get("type").and_then(Value::as_str).unwrap_or("any").to_string();
        // Surface enum options inline (e.g. "string(grep|glob|list)") so a
        // model that passed an invalid value can self-correct instead of
        // retrying blind with the same malformed arguments.
        let rendered = match def.get("enum").and_then(Value::as_array) {
            Some(options) if !options.is_empty() => {
                let joined = options
                    .iter()
                    .map(|option| match option {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    })
                    .collect::<Vec<_>>()
                    .join("|");
                format!("{type_str}({joined})")
            }
            _ => type_str,
        };
        prop_types.insert(name.clone(), Value::String(rendered));
    }

    Some(serde_json::json!({
        "required": required,
        "properties": prop_types,
    }))
}

/// Render a `jsonschema` validation failure into a model-actionable message.
///
/// The default jsonschema error only quotes the offending *value*
/// (e.g. `"content" is not one of "github", "sarif" ...`), which hides which
/// field was wrong and led agents to retry the same malformed call blindly for
/// many turns. This prefixes the failure with its JSON-pointer path and, for
/// enum/const/type failures, lists the accepted values so the model can
/// self-correct in a single pass instead of burning the tool budget.
pub fn describe_jsonschema_error(err: &ValidationError<'_>) -> String {
    let path = err.instance_path().to_string();
    let path_label = if path.is_empty() {
        "(root)".to_string()
    } else {
        path
    };
    let value = err.instance();
    let value_str = match &**value {
        Value::String(s) => format!("\"{s}\""),
        other => other.to_string(),
    };
    match err.kind() {
        ValidationErrorKind::Enum { options } => {
            let opts = options
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .map(|v| match v {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            format!(
                "field '{path_label}' value {value_str} is not one of the allowed enum: [{opts}]"
            )
        }
        ValidationErrorKind::Constant { expected_value } => format!(
            "field '{path_label}' value {value_str} must equal the required const {expected_value}"
        ),
        ValidationErrorKind::Type { kind } => {
            let expected = match kind {
                TypeKind::Single(t) => t.to_string(),
                TypeKind::Multiple(set) => format!("{set:?}"),
            };
            format!("field '{path_label}' has wrong type: expected {expected}, got {value_str}")
        }
        ValidationErrorKind::Required { property } => {
            let name = match property {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            format!("missing required property '{name}'")
        }
        ValidationErrorKind::AdditionalProperties { unexpected } => format!(
            "unexpected field(s) {unexpected:?} not allowed by the schema (did you use the right field name?)"
        ),
        _ => format!("field '{path_label}' failed validation: {value_str}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn enum_error_names_field_and_valid_options() {
        let schema = json!({
            "type": "object",
            "properties": {
                "format": {"type": "string", "enum": ["github", "sarif", "files_with_matches", "count"]}
            }
        });
        let instance = json!({ "format": "content" });
        let error = jsonschema::validate(&schema, &instance).unwrap_err();
        let msg = describe_jsonschema_error(&error);
        assert!(msg.contains("field '/format'"), "msg was: {msg}");
        assert!(msg.contains("\"content\""), "msg was: {msg}");
        assert!(msg.contains("github, sarif, files_with_matches, count"), "msg was: {msg}");
    }

    #[test]
    fn multiple_errors_are_described_independently() {
        // A single invalid call can violate several schema constraints at once.
        // Each failure must be describable on its own so a caller can join them
        // into one self-correction message.
        let schema = json!({
            "type": "object",
            "required": ["action", "format"],
            "properties": {
                "action": {"type": "string"},
                "format": {"type": "string", "enum": ["github", "sarif"]}
            }
        });
        let instance = json!({ "format": "content" });
        let validator = jsonschema::validator_for(&schema).expect("schema is valid");
        let errors: Vec<_> = validator.iter_errors(&instance).collect();
        assert!(
            errors.len() >= 2,
            "expected both missing-action and bad-format errors, got {}",
            errors.len()
        );
        let messages: Vec<String> = errors.iter().map(describe_jsonschema_error).collect();
        assert!(messages.iter().any(|m| m.contains("missing required property 'action'")));
        assert!(
            messages
                .iter()
                .any(|m| m.contains("field '/format'") && m.contains("\"content\""))
        );
    }

    #[test]
    fn missing_required_names_property() {
        let schema = json!({
            "type": "object",
            "required": ["action"],
            "properties": { "action": {"type": "string"} }
        });
        let instance = json!({});
        let error = jsonschema::validate(&schema, &instance).unwrap_err();
        let msg = describe_jsonschema_error(&error);
        assert!(msg.contains("missing required property 'action'"), "msg was: {msg}");
    }
}
