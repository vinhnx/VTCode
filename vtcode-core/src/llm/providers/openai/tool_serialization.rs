//! Tool serialization helpers for OpenAI payloads.
//!
//! Keeps tool JSON shaping isolated from the main provider logic.

use crate::llm::provider;
use hashbrown::HashSet;
use serde_json::{Value, json};

fn responses_dedupe_key(serialized_tool: &Value) -> String {
    if let Some(name) = serialized_tool.get("name").and_then(Value::as_str) {
        return format!("name:{name}");
    }

    serialized_tool.to_string()
}

fn serialize_responses_hosted_tool(tool_type: &str, config: Option<&Value>) -> Option<Value> {
    let mut payload = serde_json::Map::new();
    payload.insert("type".to_string(), json!(tool_type));

    match config {
        Some(Value::Object(config_map)) => {
            payload.extend(config_map.clone());
        }
        Some(_) | None => return None,
    }

    Some(Value::Object(payload))
}

fn serialize_responses_function_tool(
    func: &provider::FunctionDefinition,
    defer_loading: bool,
) -> Value {
    let mut value = json!({
        "type": "function",
        "name": &func.name,
        "description": &func.description,
        "parameters": &func.parameters
    });
    if defer_loading && let Some(obj) = value.as_object_mut() {
        obj.insert("defer_loading".to_string(), json!(true));
    }
    value
}

pub fn serialize_tools(tools: &[provider::ToolDefinition], model: &str) -> Option<Value> {
    if tools.is_empty() {
        return None;
    }

    let mut seen_names = HashSet::new();
    let serialized_tools = tools
        .iter()
        .filter_map(|tool| {
            let canonical_name = tool
                .function
                .as_ref()
                .map(|f| f.name.as_str())
                .unwrap_or(tool.tool_type.as_str());
            if !seen_names.insert(canonical_name.to_string()) {
                return None;
            }

            let serialized = match tool.tool_type.as_str() {
                "function" => {
                    let func = tool.function.as_ref()?;
                    let name = &func.name;
                    let description = &func.description;
                    let parameters = &func.parameters;
                    let mut value = json!({
                        "type": &tool.tool_type,
                        "name": name,
                        "description": description,
                        "parameters": parameters,
                        "function": {
                            "name": name,
                            "description": description,
                            "parameters": parameters,
                        }
                    });
                    if tool.defer_loading == Some(true)
                        && let Some(obj) = value.as_object_mut()
                    {
                        obj.insert("defer_loading".to_string(), json!(true));
                    }
                    value
                }
                "apply_patch" | "shell" | "custom" | "grammar" => {
                    if is_gpt5_or_newer(model) {
                        json!(tool)
                    } else if let Some(func) = &tool.function {
                        json!({
                            "type": "function",
                            "function": {
                                "name": func.name,
                                "description": func.description,
                                "parameters": func.parameters
                            }
                        })
                    } else {
                        return None;
                    }
                }
                "tool_search" => json!({ "type": "tool_search" }),
                _ => json!(tool),
            };

            Some(serialized)
        })
        .collect::<Vec<Value>>();

    Some(Value::Array(serialized_tools))
}

pub fn serialize_tools_for_responses(tools: &[provider::ToolDefinition]) -> Option<Value> {
    if tools.is_empty() {
        return None;
    }

    let mut seen_names = HashSet::new();
    let serialized_tools = tools
        .iter()
        .filter_map(|tool| {
            let serialized = match tool.tool_type.as_str() {
                "function" => {
                    let func = tool.function.as_ref()?;
                    Some(serialize_responses_function_tool(
                        func,
                        tool.defer_loading == Some(true),
                    ))
                }
                "apply_patch" => {
                    if let Some(func) = tool.function.as_ref() {
                        Some(serialize_responses_function_tool(func, false))
                    } else {
                        Some(json!({
                            "type": "function",
                            "name": "apply_patch",
                            "description": "Apply VT Code patches. Use format: *** Begin Patch, *** Update File: path, @@ context, -/+ lines, *** End Patch. Do NOT use unified diff (---/+++).",
                            "parameters": json!({
                                "type": "object",
                                "properties": {
                                    "input": { "type": "string", "description": "Patch in VT Code format" },
                                    "patch": { "type": "string", "description": "Alias for input" }
                                },
                                "anyOf": [
                                    {"required": ["input"]},
                                    {"required": ["patch"]}
                                ]
                            })
                        }))
                    }
                }
                "shell" => tool
                    .function
                    .as_ref()
                    .map(|func| serialize_responses_function_tool(func, false)),
                "custom" => tool.function.as_ref().map(|func| {
                    json!({
                        "type": "custom",
                        "name": &func.name,
                        "description": &func.description,
                        "format": func.parameters.get("format")
                    })
                }),
                "grammar" => tool.grammar.as_ref().map(|grammar| {
                    json!({
                        "type": "custom",
                        "name": "apply_patch_grammar",
                        "description": "Use the `apply_patch` tool to edit files. This is a FREEFORM tool.",
                        "format": {
                            "type": "grammar",
                            "syntax": &grammar.syntax,
                            "definition": &grammar.definition
                        }
                    })
                }),
                "tool_search" => Some(json!({ "type": "tool_search" })),
                "web_search" => serialize_responses_hosted_tool("web_search", tool.web_search.as_ref()),
                "file_search" | "mcp" => serialize_responses_hosted_tool(
                    tool.tool_type.as_str(),
                    tool.hosted_tool_config.as_ref(),
                ),
                _ => tool
                    .function
                    .as_ref()
                    .map(|func| serialize_responses_function_tool(func, false)),
            }?;

            if !seen_names.insert(responses_dedupe_key(&serialized)) {
                return None;
            }

            Some(serialized)
        })
        .collect::<Vec<Value>>();

    Some(Value::Array(serialized_tools))
}

fn is_gpt5_or_newer(model: &str) -> bool {
    let normalized = model.to_lowercase();
    normalized.contains("gpt-5")
        || normalized.contains("gpt5")
        || normalized.contains("o1")
        || normalized.contains("o3")
        || normalized.contains("o4")
}
