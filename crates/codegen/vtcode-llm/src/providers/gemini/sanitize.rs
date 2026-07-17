use super::*;

pub fn sanitize_function_parameters(parameters: Value) -> Value {
    sanitize_function_parameters_impl(parameters, false)
}

fn should_alias_description_property(properties: &Map<String, Value>) -> bool {
    properties.contains_key("description") && !properties.contains_key("details")
}

fn sanitize_property_name(name: &str, alias_description: bool) -> &str {
    if alias_description && name == "description" {
        "details"
    } else {
        name
    }
}

fn sanitize_function_parameters_impl(parameters: Value, inside_properties_map: bool) -> Value {
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

            let alias_description_property =
                inside_properties_map && should_alias_description_property(&map);
            let alias_required_description = map
                .get("properties")
                .and_then(Value::as_object)
                .is_some_and(should_alias_description_property);

            // Process all properties recursively, removing unsupported fields
            let mut sanitized = Map::new();
            for (key, value) in map {
                let is_properties_map = key == "properties";
                // Skip unsupported fields at this level
                if !inside_properties_map
                    && (UNSUPPORTED_FIELDS.contains(&key.as_str()) || key.starts_with("x-"))
                {
                    continue;
                }
                // Recursively sanitize nested values
                let sanitized_key = if inside_properties_map {
                    sanitize_property_name(&key, alias_description_property).to_string()
                } else {
                    key
                };
                sanitized.insert(
                    sanitized_key,
                    sanitize_function_parameters_impl(value, is_properties_map),
                );
            }

            let property_names = sanitized
                .get("properties")
                .and_then(Value::as_object)
                .map(|properties| properties.keys().cloned().collect::<Vec<_>>());
            let drop_required = sanitized
                .get_mut("required")
                .and_then(Value::as_array_mut)
                .map(|required| {
                    let Some(property_names) = property_names.as_ref() else {
                        return true;
                    };

                    for item in required.iter_mut() {
                        if let Some(name) = item.as_str() {
                            let sanitized_name =
                                sanitize_property_name(name, alias_required_description);
                            if sanitized_name != name {
                                *item = Value::String(sanitized_name.to_string());
                            }
                        }
                    }
                    required.retain(|item| {
                        item.as_str()
                            .map(|name| property_names.iter().any(|property| property == name))
                            .unwrap_or(false)
                    });
                    required.is_empty()
                })
                .unwrap_or(false);
            if drop_required {
                sanitized.remove("required");
            }

            Value::Object(sanitized)
        }
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(|value| sanitize_function_parameters_impl(value, false))
                .collect(),
        ),
        other => other,
    }
}
