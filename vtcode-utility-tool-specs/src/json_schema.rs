use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum JsonSchema {
    Object {
        #[serde(default)]
        properties: BTreeMap<String, JsonSchema>,
        #[serde(skip_serializing_if = "Option::is_none")]
        required: Option<Vec<String>>,
        #[serde(
            rename = "additionalProperties",
            skip_serializing_if = "Option::is_none"
        )]
        additional_properties: Option<AdditionalProperties>,
        #[serde(rename = "anyOf", skip_serializing_if = "Option::is_none")]
        any_of: Option<Vec<Value>>,
    },
    String {
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    Number {
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    Boolean {
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    Array {
        items: Box<JsonSchema>,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    Null,
}

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
    match value {
        Value::Object(map) => match map.get("type").and_then(Value::as_str) {
            Some("object") => {
                let properties = map
                    .get("properties")
                    .and_then(Value::as_object)
                    .map(|props| {
                        props
                            .iter()
                            .map(|(key, value)| (key.clone(), parse_tool_input_schema(value)))
                            .collect()
                    })
                    .unwrap_or_default();
                let required = map.get("required").and_then(Value::as_array).map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(ToOwned::to_owned)
                        .collect::<Vec<_>>()
                });
                let additional_properties =
                    map.get("additionalProperties").map(|value| match value {
                        Value::Bool(flag) => AdditionalProperties::Boolean(*flag),
                        Value::Object(_) => {
                            AdditionalProperties::Schema(Box::new(parse_tool_input_schema(value)))
                        }
                        _ => AdditionalProperties::Boolean(true),
                    });
                let any_of = map.get("anyOf").and_then(Value::as_array).cloned();

                JsonSchema::Object {
                    properties,
                    required,
                    additional_properties,
                    any_of,
                }
            }
            Some("array") => JsonSchema::Array {
                items: Box::new(
                    map.get("items")
                        .map(parse_tool_input_schema)
                        .unwrap_or(JsonSchema::Null),
                ),
                description: map
                    .get("description")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
            },
            Some("boolean") => JsonSchema::Boolean {
                description: map
                    .get("description")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
            },
            Some("integer" | "number") => JsonSchema::Number {
                description: map
                    .get("description")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
            },
            Some("string") => JsonSchema::String {
                description: map
                    .get("description")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
            },
            _ => {
                if map.contains_key("enum") {
                    JsonSchema::String {
                        description: map
                            .get("description")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned),
                    }
                } else {
                    JsonSchema::Null
                }
            }
        },
        _ => JsonSchema::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::{AdditionalProperties, JsonSchema, parse_tool_input_schema};
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

        let JsonSchema::Object {
            additional_properties,
            ..
        } = schema
        else {
            panic!("expected object schema");
        };

        let Some(AdditionalProperties::Schema(nested)) = additional_properties else {
            panic!("expected nested additional properties schema");
        };

        match *nested {
            JsonSchema::String { description } => {
                assert_eq!(description.as_deref(), Some("value"));
            }
            other => panic!("expected string schema, got {other:?}"),
        }
    }
}
