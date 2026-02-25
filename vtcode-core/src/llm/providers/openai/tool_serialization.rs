//! Tool serialization helpers for OpenAI payloads.
//!
//! Keeps tool JSON shaping isolated from the main provider logic.

use crate::llm::provider;
use serde_json::{Value, json};
use std::collections::HashSet;

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

            Some(match tool.tool_type.as_str() {
                "function" => {
                    let func = tool.function.as_ref()?;
                    let name = &func.name;
                    let description = &func.description;
                    let parameters = &func.parameters;

                    json!({
                        "type": &tool.tool_type,
                        "name": name,
                        "description": description,
                        "parameters": parameters,
                        "function": {
                            "name": name,
                            "description": description,
                            "parameters": parameters,
                        }
                    })
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
                _ => json!(tool),
            })
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
        .filter_map(|tool| match tool.tool_type.as_str() {
            "function" => {
                let func = tool.function.as_ref()?;
                if !seen_names.insert(func.name.clone()) {
                    return None;
                }
                Some(json!({
                    "type": "function",
                    "name": &func.name,
                    "description": &func.description,
                    "parameters": &func.parameters
                }))
            }
            "apply_patch" => {
                let name = tool
                    .function
                    .as_ref()
                    .map(|f| f.name.as_str())
                    .unwrap_or("apply_patch");
                if !seen_names.insert(name.to_string()) {
                    return None;
                }
                if let Some(func) = tool.function.as_ref() {
                    Some(json!({
                        "type": "function",
                        "name": &func.name,
                        "description": &func.description,
                        "parameters": &func.parameters
                    }))
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
            "shell" => tool.function.as_ref().map(|func| {
                json!({
                    "type": "function",
                    "name": &func.name,
                    "description": &func.description,
                    "parameters": &func.parameters
                })
            }),
            "custom" => {
                if let Some(func) = &tool.function {
                    if !seen_names.insert(func.name.clone()) {
                        return None;
                    }
                    Some(json!({
                        "type": "custom",
                        "name": &func.name,
                        "description": &func.description,
                        "format": func.parameters.get("format")
                    }))
                } else {
                    None
                }
            }
            "grammar" => {
                let name = tool
                    .function
                    .as_ref()
                    .map(|f| f.name.as_str())
                    .unwrap_or("apply_patch_grammar");
                if !seen_names.insert(name.to_string()) {
                    return None;
                }
                tool.grammar.as_ref().map(|grammar| {
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
                })
            }
            _ => {
                if let Some(func) = &tool.function {
                    if !seen_names.insert(func.name.clone()) {
                        return None;
                    }
                    Some(json!({
                        "type": "function",
                        "name": &func.name,
                        "description": &func.description,
                        "parameters": &func.parameters
                    }))
                } else {
                    None
                }
            }
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
