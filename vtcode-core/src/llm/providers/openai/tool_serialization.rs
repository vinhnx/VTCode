//! Tool serialization helpers for OpenAI payloads.
//!
//! Keeps tool JSON shaping isolated from the main provider logic.

use crate::config::constants::tools;
use crate::config::core::{
    OpenAIHostedShellConfig, OpenAIHostedShellDomainSecret, OpenAIHostedShellEnvironment,
    OpenAIHostedShellNetworkPolicy, OpenAIHostedShellNetworkPolicyType, OpenAIHostedSkill,
    OpenAIHostedSkillVersion,
};
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
        "parameters": sanitize_openai_function_parameters(func.parameters.clone())
    });
    if defer_loading && let Some(obj) = value.as_object_mut() {
        obj.insert("defer_loading".to_string(), json!(true));
    }
    value
}

fn sanitize_openai_function_parameters(value: Value) -> Value {
    match value {
        Value::Object(mut map) => {
            map.remove("default");
            map.remove("format");
            map.remove("allOf");
            map.remove("oneOf");
            map.remove("if");
            map.remove("then");
            map.remove("else");

            if map.get("type").is_none()
                && let Some(any_of) = map.remove("anyOf")
                && let Some(replacement) = collapse_supported_any_of(&any_of)
            {
                let mut sanitized = sanitize_openai_function_parameters(replacement);
                if let Value::Object(ref mut replacement_map) = sanitized
                    && let Some(description) = map.remove("description")
                {
                    replacement_map
                        .entry("description".to_string())
                        .or_insert(description);
                }
                return sanitized;
            }

            map.remove("anyOf");

            for nested in map.values_mut() {
                let next = sanitize_openai_function_parameters(nested.clone());
                *nested = next;
            }

            if map.get("type").and_then(Value::as_str) == Some("object")
                && !map.contains_key("properties")
            {
                map.insert("properties".to_string(), json!({}));
            }

            Value::Object(map)
        }
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(sanitize_openai_function_parameters)
                .collect(),
        ),
        other => other,
    }
}

fn collapse_supported_any_of(any_of: &Value) -> Option<Value> {
    let variants = any_of.as_array()?;
    variants
        .iter()
        .find(|variant| variant.get("type").and_then(Value::as_str) == Some("string"))
        .cloned()
        .or_else(|| variants.first().cloned())
}

fn trim_non_empty_owned(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn serialize_hosted_skill_version(version: &OpenAIHostedSkillVersion) -> Option<Value> {
    match version {
        OpenAIHostedSkillVersion::Latest(_) => None,
        OpenAIHostedSkillVersion::Number(value) => Some(json!(value)),
        OpenAIHostedSkillVersion::String(value) => {
            let version = trim_non_empty_owned(value)?;
            (!version.eq_ignore_ascii_case("latest")).then_some(Value::String(version))
        }
    }
}

fn serialize_hosted_skill(skill: &OpenAIHostedSkill) -> Option<Value> {
    match skill {
        OpenAIHostedSkill::SkillReference { skill_id, version } => {
            let skill_id = trim_non_empty_owned(skill_id)?;
            let mut payload = serde_json::Map::from_iter([
                ("type".to_string(), json!("skill_reference")),
                ("skill_id".to_string(), json!(skill_id)),
            ]);

            if let Some(version) = serialize_hosted_skill_version(version) {
                payload.insert("version".to_string(), version);
            }

            Some(Value::Object(payload))
        }
        OpenAIHostedSkill::Inline { bundle_b64, sha256 } => {
            let bundle_b64 = trim_non_empty_owned(bundle_b64)?;
            let mut payload = serde_json::Map::from_iter([
                ("type".to_string(), json!("inline")),
                ("bundle_b64".to_string(), json!(bundle_b64)),
            ]);
            if let Some(sha256) = sha256.as_deref().and_then(trim_non_empty_owned) {
                payload.insert("sha256".to_string(), json!(sha256));
            }
            Some(Value::Object(payload))
        }
    }
}

fn serialize_hosted_shell_domain_secret(secret: &OpenAIHostedShellDomainSecret) -> Option<Value> {
    let domain = trim_non_empty_owned(&secret.domain)?;
    let name = trim_non_empty_owned(&secret.name)?;
    let value = trim_non_empty_owned(&secret.value)?;

    Some(json!({
        "domain": domain,
        "name": name,
        "value": value,
    }))
}

fn serialize_openai_hosted_shell_network_policy(
    policy: &OpenAIHostedShellNetworkPolicy,
) -> Option<Value> {
    match policy.policy_type {
        OpenAIHostedShellNetworkPolicyType::Disabled => Some(json!({ "type": "disabled" })),
        OpenAIHostedShellNetworkPolicyType::Allowlist => {
            let allowed_domains: Vec<String> = policy
                .allowed_domains
                .iter()
                .filter_map(|value| trim_non_empty_owned(value))
                .collect();
            if allowed_domains.is_empty() {
                return None;
            }

            let mut payload = serde_json::Map::from_iter([
                ("type".to_string(), json!("allowlist")),
                ("allowed_domains".to_string(), json!(allowed_domains)),
            ]);

            let domain_secrets: Vec<Value> = policy
                .domain_secrets
                .iter()
                .filter_map(serialize_hosted_shell_domain_secret)
                .collect();
            if !domain_secrets.is_empty() {
                payload.insert("domain_secrets".to_string(), Value::Array(domain_secrets));
            }

            Some(Value::Object(payload))
        }
    }
}

fn serialize_openai_hosted_shell(config: &OpenAIHostedShellConfig) -> Option<Value> {
    if !config.enabled {
        return None;
    }

    let mut environment = serde_json::Map::new();
    environment.insert("type".to_string(), json!(config.environment.as_str()));

    match config.environment {
        OpenAIHostedShellEnvironment::ContainerAuto => {
            if let Some(network_policy) =
                serialize_openai_hosted_shell_network_policy(&config.network_policy)
            {
                environment.insert("network_policy".to_string(), network_policy);
            }

            let file_ids: Vec<String> = config
                .file_ids
                .iter()
                .filter_map(|value| trim_non_empty_owned(value))
                .collect();
            if !file_ids.is_empty() {
                environment.insert("file_ids".to_string(), json!(file_ids));
            }

            let skills: Vec<Value> = config
                .skills
                .iter()
                .filter_map(serialize_hosted_skill)
                .collect();
            if !skills.is_empty() {
                environment.insert("skills".to_string(), Value::Array(skills));
            }
        }
        OpenAIHostedShellEnvironment::ContainerReference => {
            let container_id = config
                .container_id
                .as_deref()
                .and_then(trim_non_empty_owned)?;
            environment.insert("container_id".to_string(), json!(container_id));
        }
    }

    Some(json!({
        "type": "shell",
        "environment": Value::Object(environment),
    }))
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
                tools::APPLY_PATCH | tools::SHELL | "custom" | "grammar" => {
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

pub fn serialize_tools_for_responses(
    tools: &[provider::ToolDefinition],
    hosted_shell: Option<&OpenAIHostedShellConfig>,
) -> Option<Value> {
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
                    if func.name == tools::SHELL {
                        hosted_shell
                            .and_then(serialize_openai_hosted_shell)
                            .or_else(|| {
                                Some(serialize_responses_function_tool(
                                    func,
                                    tool.defer_loading == Some(true),
                                ))
                            })
                    } else {
                        Some(serialize_responses_function_tool(
                            func,
                            tool.defer_loading == Some(true),
                        ))
                    }
                }
                tools::APPLY_PATCH => {
                    if let Some(func) = tool.function.as_ref() {
                        Some(serialize_responses_function_tool(func, false))
                    } else {
                        Some(json!({
                            "type": "function",
                            "name": tools::APPLY_PATCH,
                            "description": crate::tools::apply_patch::with_semantic_anchor_guidance("Apply VT Code patches. Use format: *** Begin Patch, *** Update File: path, @@ context, -/+ lines, *** End Patch. Do NOT use unified diff (---/+++)"),
                            "parameters": crate::tools::apply_patch::parameter_schema("Patch in VT Code format")
                        }))
                    }
                }
                tools::SHELL => hosted_shell.and_then(serialize_openai_hosted_shell),
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
