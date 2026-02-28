use super::*;

pub fn sanitize_function_parameters(parameters: Value) -> Value {
    match parameters {
        Value::Object(map) => {
            // List of unsupported fields for Gemini API
            // Reference: https://ai.google.dev/gemini-api/docs/function-calling
            const UNSUPPORTED_FIELDS: &[&str] = &[
                "additionalProperties",
                "oneOf",
                "anyOf",
                "allOf",
                "exclusiveMaximum",
                "exclusiveMinimum",
                "minimum",
                "maximum",
                "$schema",
                "$id",
                "$ref",
                "definitions",
                "patternProperties",
                "dependencies",
                "const",
                "if",
                "then",
                "else",
                "not",
                "contentMediaType",
                "contentEncoding",
            ];

            // Process all properties recursively, removing unsupported fields
            let mut sanitized = Map::new();
            for (key, value) in map {
                // Skip unsupported fields at this level
                if UNSUPPORTED_FIELDS.contains(&key.as_str()) || key.starts_with("x-") {
                    continue;
                }
                // Recursively sanitize nested values
                sanitized.insert(key, sanitize_function_parameters(value));
            }
            Value::Object(sanitized)
        }
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(sanitize_function_parameters)
                .collect(),
        ),
        other => other,
    }
}
