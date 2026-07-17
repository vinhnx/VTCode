use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

pub type JsonSchema = Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AdditionalProperties {
    Boolean(bool),
    Schema(Box<JsonSchema>),
}

impl From<bool> for AdditionalProperties {
    fn from(value: bool) -> Self {
        Self::Boolean(value)
    }
}

#[must_use]
pub fn parse_tool_input_schema(value: &Value) -> JsonSchema {
    let mut schema = value.clone();
    sanitize_json_schema(&mut schema);
    schema
}

fn sanitize_json_schema(value: &mut Value) {
    match value {
        Value::Bool(_) => {
            *value = json!({ "type": "string" });
        }
        Value::Object(map) => sanitize_schema_object(map),
        Value::Array(items) => {
            for item in items {
                sanitize_json_schema(item);
            }
        }
        Value::Null | Value::Number(_) | Value::String(_) => {}
    }
}

fn sanitize_schema_object(map: &mut Map<String, Value>) {
    if let Some(properties) = map.get_mut("properties").and_then(Value::as_object_mut) {
        for schema in properties.values_mut() {
            sanitize_json_schema(schema);
        }
    }

    if let Some(items) = map.get_mut("items") {
        sanitize_json_schema(items);
    }

    if let Some(prefix_items) = map.get_mut("prefixItems") {
        sanitize_json_schema(prefix_items);
    }

    if let Some(additional_properties) = map.get_mut("additionalProperties")
        && !matches!(additional_properties, Value::Bool(_))
    {
        sanitize_json_schema(additional_properties);
    }

    if let Some(any_of) = map.get_mut("anyOf") {
        sanitize_json_schema(any_of);
    }

    if let Some(const_value) = map.remove("const") {
        map.insert("enum".to_string(), Value::Array(vec![const_value]));
    }

    let mut schema_types = normalized_schema_types(map);
    if schema_types.is_empty() && map.contains_key("anyOf") {
        return;
    }

    if schema_types.is_empty() {
        if map.contains_key("properties")
            || map.contains_key("required")
            || map.contains_key("additionalProperties")
        {
            schema_types.push("object");
        } else if map.contains_key("items") || map.contains_key("prefixItems") {
            schema_types.push("array");
        } else if map.contains_key("enum") || map.contains_key("format") {
            schema_types.push("string");
        } else if map.contains_key("minimum")
            || map.contains_key("maximum")
            || map.contains_key("exclusiveMinimum")
            || map.contains_key("exclusiveMaximum")
            || map.contains_key("multipleOf")
        {
            schema_types.push("number");
        } else {
            schema_types.push("string");
        }
    }

    write_schema_types(map, &schema_types);
    ensure_default_children_for_schema_types(map, &schema_types);
}

fn normalized_schema_types(map: &Map<String, Value>) -> Vec<&'static str> {
    let Some(schema_type) = map.get("type") else {
        return Vec::new();
    };

    match schema_type {
        Value::String(schema_type) => schema_type_from_str(schema_type).into_iter().collect(),
        Value::Array(schema_types) => schema_types
            .iter()
            .filter_map(Value::as_str)
            .filter_map(schema_type_from_str)
            .collect(),
        _ => Vec::new(),
    }
}

fn write_schema_types(map: &mut Map<String, Value>, schema_types: &[&'static str]) {
    match schema_types {
        [] => {
            map.remove("type");
        }
        [schema_type] => {
            map.insert(
                "type".to_string(),
                Value::String((*schema_type).to_string()),
            );
        }
        _ => {
            map.insert(
                "type".to_string(),
                Value::Array(
                    schema_types
                        .iter()
                        .map(|schema_type| Value::String((*schema_type).to_string()))
                        .collect(),
                ),
            );
        }
    }
}

fn ensure_default_children_for_schema_types(map: &mut Map<String, Value>, schema_types: &[&str]) {
    if schema_types.contains(&"object") && !map.contains_key("properties") {
        map.insert("properties".to_string(), Value::Object(Map::new()));
    }

    if schema_types.contains(&"array") && !map.contains_key("items") {
        map.insert("items".to_string(), json!({ "type": "string" }));
    }
}

fn schema_type_from_str(schema_type: &str) -> Option<&'static str> {
    match schema_type {
        "string" => Some("string"),
        "number" => Some("number"),
        "integer" => Some("integer"),
        "boolean" => Some("boolean"),
        "object" => Some("object"),
        "array" => Some("array"),
        "null" => Some("null"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::parse_tool_input_schema;
    use serde_json::{Value, json};

    #[test]
    fn parse_tool_input_schema_preserves_schema_field_names() {
        let schema = parse_tool_input_schema(&json!({
            "type": "object",
            "properties": {
                "input": {"type": "string"}
            },
            "additionalProperties": false,
            "anyOf": [
                {"required": ["input"]},
                {"required": ["patch"]}
            ]
        }));

        let serialized = serde_json::to_value(&schema).expect("serialize schema");
        assert_eq!(serialized["additionalProperties"], Value::Bool(false));
        assert!(serialized["anyOf"].is_array());
        assert!(serialized.get("additional_properties").is_none());
        assert!(serialized.get("any_of").is_none());
    }

    #[test]
    fn parse_tool_input_schema_parses_object_additional_properties_schema() {
        let schema = parse_tool_input_schema(&json!({
            "type": "object",
            "additionalProperties": {
                "type": "string",
                "description": "value"
            }
        }));

        assert_eq!(schema["type"], "object");
        assert_eq!(schema["additionalProperties"]["type"], "string");
        assert_eq!(schema["additionalProperties"]["description"], "value");
    }

    #[test]
    fn parse_tool_input_schema_preserves_nested_any_of_and_nullable_type_unions() {
        let schema = parse_tool_input_schema(&json!({
            "type": "object",
            "properties": {
                "open": {
                    "anyOf": [
                        {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "ref_id": {"type": "string"},
                                    "lineno": {"type": ["integer", "null"]}
                                },
                                "required": ["ref_id"],
                                "additionalProperties": false
                            }
                        },
                        {"type": "null"}
                    ]
                },
                "message": {"type": ["string", "null"]}
            },
            "additionalProperties": false
        }));

        let variants = schema["properties"]["open"]["anyOf"]
            .as_array()
            .expect("open anyOf");
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0]["type"], "array");
        assert_eq!(variants[0]["items"]["type"], "object");
        assert_eq!(
            variants[0]["items"]["properties"]["lineno"]["type"],
            json!(["integer", "null"])
        );
        assert_eq!(
            schema["properties"]["message"]["type"],
            json!(["string", "null"])
        );
    }

    #[test]
    fn parse_tool_input_schema_preserves_integer_and_string_enums() {
        let schema = parse_tool_input_schema(&json!({
            "type": "object",
            "properties": {
                "page": {"type": "integer"},
                "response_length": {
                    "type": "string",
                    "enum": ["short", "medium", "long"]
                },
                "kind": {
                    "type": "const",
                    "const": "tagged"
                }
            }
        }));

        assert_eq!(schema["properties"]["page"]["type"], "integer");
        assert_eq!(
            schema["properties"]["response_length"]["enum"],
            json!(["short", "medium", "long"])
        );
        assert_eq!(schema["properties"]["kind"]["type"], "string");
        assert_eq!(schema["properties"]["kind"]["enum"], json!(["tagged"]));
    }
}
