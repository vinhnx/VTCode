pub mod commands;
pub mod paths;

use jsonschema::ValidationError;
use jsonschema::error::{TypeKind, ValidationErrorKind};
use serde_json::Value;

/// Extract a condensed representation of a JSON Schema for error hints.
///
/// Returns a JSON object with:
/// - `required`: array of required field names
/// - `properties`: object mapping field name -> its declared `type` (or `"any"` if absent)
///   with enum choices and numeric bounds included when present
///
/// This is intentionally compact so it can be included in validation error
/// payloads without bloating the context.
pub fn condensed_schema_hint(schema: &Value) -> Option<Value> {
    let properties = schema.get("properties").and_then(Value::as_object)?;
    let required: Vec<Value> = schema.get("required").and_then(Value::as_array).cloned().unwrap_or_default();

    let mut prop_types = serde_json::Map::new();
    for (name, def) in properties {
        let type_str = def.get("type").and_then(Value::as_str).unwrap_or("any").to_string();
        // Surface enum options inline (e.g. "string(grep|glob|list)") so a
        // model that passed an invalid value can self-correct instead of
        // retrying blind with the same malformed arguments.
        let mut rendered = match def.get("enum").and_then(Value::as_array) {
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

        let bounds = [("min", def.get("minimum")), ("max", def.get("maximum"))]
            .into_iter()
            .filter_map(|(label, value)| value.map(|value| format!("{label}={value}")))
            .collect::<Vec<_>>();
        if !bounds.is_empty() {
            if rendered.ends_with(')') {
                rendered.pop();
                rendered.push(',');
            } else {
                rendered.push('(');
            }
            rendered.push_str(&bounds.join(","));
            rendered.push(')');
        }
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
    let path_label = if path.is_empty() { "(root)".to_string() } else { path };
    let schema_path = err.schema_path().to_string();
    let value = err.instance();
    let raw_value_str = match &**value {
        Value::String(s) => format!("\"{s}\""),
        other => other.to_string(),
    };
    let value_str = truncate_for_error(&raw_value_str, 500);
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
            format!("field '{path_label}' value {value_str} is not one of the allowed enum: [{opts}]")
        }
        ValidationErrorKind::Constant { expected_value } => {
            format!("field '{path_label}' value {value_str} must equal the required const {expected_value}")
        }
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
            format!("field '{path_label}' missing required property '{name}' (schema {schema_path})")
        }
        ValidationErrorKind::AdditionalProperties { unexpected } => {
            format!("unexpected field(s) {unexpected:?} not allowed by the schema (did you use the right field name?)")
        }
        ValidationErrorKind::Not { schema } => {
            let forbidden = forbidden_properties_from_not_schema(schema);
            if forbidden.is_empty() {
                let schema_str = truncate_for_error(&schema.to_string(), 300);
                format!(
                    "field '{path_label}' value {value_str} is forbidden by schema {schema_path} (not {schema_str}); remove the forbidden field(s) and retry"
                )
            } else {
                let present = present_forbidden_fields(value, &forbidden);
                let offending = if present.is_empty() { forbidden.clone() } else { present };
                format!(
                    "field '{path_label}' must not include {offending:?} for this action (schema {schema_path} forbids them); remove {offending:?} and retry"
                )
            }
        }
        ValidationErrorKind::AnyOf { context } => {
            describe_combinator_error(&path_label, &schema_path, "anyOf", context)
        }
        ValidationErrorKind::OneOfNotValid { context } => {
            describe_combinator_error(&path_label, &schema_path, "oneOf", context)
        }
        ValidationErrorKind::OneOfMultipleValid { context } => {
            let _ = context;
            format!(
                "field '{path_label}' value {value_str} matches more than one allowed shape (schema {schema_path} oneOf); make the call match exactly one variant"
            )
        }
        ValidationErrorKind::FalseSchema => {
            format!("field '{path_label}' value {value_str} is not allowed here (schema {schema_path} disallows it)")
        }
        _ => {
            // Fall back to the validator's own message plus the schema location
            // so conditional (`if`/`then`/`not`) failures don't collapse to a
            // bare value dump. Truncate to keep large objects out of context.
            let detail = truncate_for_error(&err.to_string(), 500);
            if schema_path.is_empty() {
                format!("field '{path_label}' failed validation: {detail}")
            } else {
                format!("field '{path_label}' failed validation (schema {schema_path}): {detail}")
            }
        }
    }
}

fn truncate_for_error(raw: &str, limit: usize) -> String {
    vtcode_commons::formatting::truncate_byte_budget(raw, limit, "…")
}

fn forbidden_properties_from_not_schema(schema: &Value) -> Vec<String> {
    // Only `required` (direct or inside anyOf/oneOf/allOf branches) forbids
    // presence. A bare `properties` entry without `required` does not forbid
    // the key, so it must not be reported as forbidden.
    let mut out = Vec::new();
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for entry in required {
            if let Some(name) = entry.as_str() {
                out.push(name.to_string());
            }
        }
    }
    for key in ["anyOf", "oneOf", "allOf"] {
        if let Some(branches) = schema.get(key).and_then(Value::as_array) {
            for branch in branches {
                for name in forbidden_properties_from_not_schema(branch) {
                    if !out.contains(&name) {
                        out.push(name);
                    }
                }
            }
        }
    }
    out
}

fn present_forbidden_fields(instance: &Value, forbidden: &[String]) -> Vec<String> {
    let Some(map) = instance.as_object() else {
        return Vec::new();
    };
    forbidden.iter().filter(|name| map.contains_key(*name)).cloned().collect()
}

fn describe_combinator_error(
    path_label: &str,
    schema_path: &str,
    keyword: &str,
    context: &[Vec<ValidationError<'static>>],
) -> String {
    // Label branches so the model can tell variants apart, and render inner
    // errors through the same path-aware describer (one level only: nested
    // combinators fall back to their Display to avoid exponential expansion).
    let mut variants: Vec<String> = Vec::new();
    for (idx, branch) in context.iter().take(3).enumerate() {
        let mut parts: Vec<String> = Vec::new();
        for error in branch.iter().take(2) {
            let rendered = match error.kind() {
                ValidationErrorKind::AnyOf { .. }
                | ValidationErrorKind::OneOfNotValid { .. }
                | ValidationErrorKind::OneOfMultipleValid { .. } => truncate_for_error(&error.to_string(), 300),
                _ => truncate_for_error(&describe_jsonschema_error(error), 300),
            };
            parts.push(rendered);
        }
        if parts.is_empty() {
            continue;
        }
        variants.push(format!("variant {}: {}", idx + 1, parts.join(" + ")));
    }
    if variants.is_empty() {
        format!(
            "field '{path_label}' does not match any allowed shape (schema {schema_path} {keyword}); adjust the arguments to match one variant and retry"
        )
    } else {
        format!(
            "field '{path_label}' does not match any allowed shape (schema {schema_path} {keyword}): {}",
            variants.join(" | ")
        )
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
        assert!(errors.len() >= 2, "expected both missing-action and bad-format errors, got {}", errors.len());
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

    #[test]
    fn numeric_bounds_are_included_in_schema_hint() {
        let schema = json!({
            "type": "object",
            "properties": {
                "max_results": {"type": "integer", "minimum": 1, "maximum": 100}
            }
        });

        let hint = condensed_schema_hint(&schema).expect("object schema should produce a hint");
        assert_eq!(hint["properties"]["max_results"], "integer(min=1,max=100)");
    }

    #[test]
    fn not_error_names_forbidden_fields_and_schema_path() {
        let schema = json!({
            "type": "object",
            "properties": {
                "action": {"type": "string"},
                "index": {"type": "integer"},
                "index_path": {"type": "string"}
            },
            "required": ["action"],
            "allOf": [
                {
                    "if": {"properties": {"action": {"const": "create"}}, "required": ["action"]},
                    "then": {"not": {"anyOf": [{"required": ["index"]}, {"required": ["index_path"]}]}}
                }
            ]
        });
        let instance = json!({"action": "create", "title": "README plan", "index": 1, "index_path": "1"});
        let validator = jsonschema::validator_for(&schema).expect("schema is valid");
        let errors: Vec<_> = validator.iter_errors(&instance).collect();
        assert!(!errors.is_empty(), "expected a not-violation");
        let messages: Vec<String> = errors.iter().map(describe_jsonschema_error).collect();
        let combined = messages.join("; ");
        assert!(combined.contains("must not include"), "msg was: {combined}");
        assert!(combined.contains("index"), "msg was: {combined}");
        assert!(
            !combined.contains("failed validation: {\"action\""),
            "should not dump the whole object, got: {combined}"
        );
    }

    #[test]
    fn fallback_includes_schema_path_instead_of_bare_dump() {
        let schema = json!({
            "type": "object",
            "properties": {"name": {"type": "string", "minLength": 5}}
        });
        let instance = json!({"name": "abc"});
        let validator = jsonschema::validator_for(&schema).expect("schema is valid");
        let errors: Vec<_> = validator.iter_errors(&instance).collect();
        assert!(!errors.is_empty());
        let msg = describe_jsonschema_error(&errors[0]);
        assert!(msg.contains("field '"), "msg was: {msg}");
        assert!(msg.contains("/properties/name"), "msg was: {msg}");
    }

    #[test]
    fn required_error_includes_instance_and_schema_location() {
        let schema = json!({
            "type": "object",
            "properties": {
                "items": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {"description": {"type": "string"}},
                        "required": ["description"]
                    }
                }
            },
            "required": ["items"]
        });
        // Asymmetric: first element valid, second missing description.
        let instance = json!({"items": [{"description": "ok"}, {"status": "completed"}]});
        let validator = jsonschema::validator_for(&schema).expect("schema is valid");
        let errors: Vec<_> = validator.iter_errors(&instance).collect();
        assert!(!errors.is_empty(), "expected a nested required violation");
        let combined = errors.iter().map(describe_jsonschema_error).collect::<Vec<_>>().join("; ");
        assert!(combined.contains("missing required property 'description'"), "msg was: {combined}");
        assert!(combined.contains("/items/1"), "msg was: {combined}");
    }

    #[test]
    fn combinator_error_labels_variants() {
        let schema = json!({
            "type": "object",
            "properties": {"action": {"type": "string"}},
            "required": ["action"],
            "allOf": [
                {
                    "if": {"properties": {"action": {"const": "update"}}, "required": ["action"]},
                    "then": {
                        "anyOf": [
                            {"required": ["index"]},
                            {"required": ["index_path"]}
                        ]
                    }
                }
            ]
        });
        let instance = json!({"action": "update"});
        let validator = jsonschema::validator_for(&schema).expect("schema is valid");
        let errors: Vec<_> = validator.iter_errors(&instance).collect();
        assert!(!errors.is_empty(), "expected an anyOf violation");
        let combined = errors.iter().map(describe_jsonschema_error).collect::<Vec<_>>().join("; ");
        assert!(combined.contains("does not match any allowed shape"), "msg was: {combined}");
        assert!(combined.contains("variant 1"), "msg was: {combined}");
    }
}
