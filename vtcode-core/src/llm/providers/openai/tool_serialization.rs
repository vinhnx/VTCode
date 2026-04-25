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
use vtcode_utility_tool_specs::parse_tool_input_schema;

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
        "parameters": sanitize_openai_function_parameters(
            func.parameters.clone(),
            should_strip_any_of_for_builtin_tool(&func.name),
        )
    });
    if defer_loading && let Some(obj) = value.as_object_mut() {
        obj.insert("defer_loading".to_string(), json!(true));
    }
    value
}

fn should_strip_any_of_for_builtin_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        tools::UNIFIED_SEARCH
            | tools::UNIFIED_EXEC
            | tools::UNIFIED_FILE
            | tools::THINK
            | tools::SEARCH_TOOLS
            | tools::WEB_SEARCH
            | tools::WEB_FETCH
            | tools::FETCH_URL
            | tools::LIST
            | tools::GREP
            | tools::FETCH
            | tools::EXEC_PTY_CMD
            | tools::SHELL
            | tools::GREP_FILE
            | tools::LIST_FILES
            | tools::LIST_SKILLS
            | tools::LOAD_SKILL
            | tools::LOAD_SKILL_RESOURCE
            | tools::EXEC_COMMAND
            | tools::WRITE_STDIN
            | tools::RUN_PTY_CMD
            | tools::CREATE_PTY_SESSION
            | tools::LIST_PTY_SESSIONS
            | tools::CLOSE_PTY_SESSION
            | tools::SEND_PTY_INPUT
            | tools::READ_PTY_SESSION
            | tools::RESIZE_PTY_SESSION
            | tools::EXECUTE_CODE
            | tools::READ_FILE
            | tools::WRITE_FILE
            | tools::EDIT_FILE
            | tools::DELETE_FILE
            | tools::CREATE_FILE
            | tools::APPLY_PATCH
            | tools::SEARCH_REPLACE
            | tools::FILE_OP
            | tools::MOVE_FILE
            | tools::COPY_FILE
            | tools::GET_ERRORS
            | tools::REQUEST_USER_INPUT
            | tools::MEMORY
            | tools::ASK_QUESTIONS
            | tools::ASK_USER_QUESTION
            | tools::CRON_CREATE
            | tools::CRON_LIST
            | tools::CRON_DELETE
            | tools::ENTER_PLAN_MODE
            | tools::EXIT_PLAN_MODE
            | tools::TASK_TRACKER
            | tools::PLAN_TASK_TRACKER
            | tools::SPAWN_AGENT
            | tools::SPAWN_BACKGROUND_SUBPROCESS
            | tools::SEND_INPUT
            | tools::WAIT_AGENT
            | tools::RESUME_AGENT
            | tools::CLOSE_AGENT
    )
}

fn sanitize_openai_function_parameters(value: Value, strip_any_of: bool) -> Value {
    sanitize_openai_schema_node(parse_tool_input_schema(&value), strip_any_of)
}

fn sanitize_openai_schema_node(value: Value, strip_any_of: bool) -> Value {
    match value {
        Value::Object(mut map) => {
            map.remove("default");
            map.remove("format");
            map.remove("allOf");
            map.remove("oneOf");
            map.remove("if");
            map.remove("then");
            map.remove("else");

            if let Some(properties) = map.get_mut("properties").and_then(Value::as_object_mut) {
                for schema in properties.values_mut() {
                    *schema =
                        sanitize_openai_schema_node(parse_tool_input_schema(schema), strip_any_of);
                }
            }

            if let Some(items) = map.get_mut("items") {
                *items = sanitize_openai_schema_node(parse_tool_input_schema(items), strip_any_of);
            }

            if let Some(prefix_items) = map.get_mut("prefixItems") {
                *prefix_items = match std::mem::take(prefix_items) {
                    Value::Array(items) => Value::Array(
                        items
                            .into_iter()
                            .map(|value| sanitize_openai_schema_node(value, strip_any_of))
                            .collect(),
                    ),
                    other => {
                        sanitize_openai_schema_node(parse_tool_input_schema(&other), strip_any_of)
                    }
                };
            }

            if let Some(additional_properties) = map.get_mut("additionalProperties") {
                if matches!(additional_properties, Value::Bool(true)) {
                    *additional_properties = json!({ "type": "string" });
                } else if !matches!(additional_properties, Value::Bool(_)) {
                    *additional_properties = sanitize_openai_schema_node(
                        parse_tool_input_schema(additional_properties),
                        strip_any_of,
                    );
                }
            }

            if let Some(any_of) = map.get_mut("anyOf") {
                let sanitized_any_of = match std::mem::take(any_of) {
                    Value::Array(items) => items
                        .into_iter()
                        .map(|value| sanitize_openai_schema_node(value, strip_any_of))
                        .collect::<Vec<_>>(),
                    other => vec![sanitize_openai_schema_node(other, strip_any_of)],
                };

                if let Some(fallback_type) = fallback_type_from_any_of(&sanitized_any_of) {
                    map.insert("type".to_string(), json!(fallback_type));
                    map.remove("anyOf");
                } else if strip_any_of || any_of_is_constraint_only(&sanitized_any_of) {
                    map.remove("anyOf");
                } else {
                    map.insert("anyOf".to_string(), Value::Array(sanitized_any_of));
                }
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
                .map(|value| sanitize_openai_schema_node(value, strip_any_of))
                .collect(),
        ),
        other => other,
    }
}

fn fallback_type_from_any_of(variants: &[Value]) -> Option<&'static str> {
    variants.iter().find_map(|variant| {
        variant
            .get("type")
            .and_then(Value::as_str)
            .filter(|schema_type| *schema_type == "string")
            .map(|_| "string")
    })
}

fn any_of_is_constraint_only(variants: &[Value]) -> bool {
    variants.iter().all(|variant| {
        let Some(map) = variant.as_object() else {
            return false;
        };

        !map.contains_key("type")
            && !map.contains_key("properties")
            && !map.contains_key("items")
            && !map.contains_key("prefixItems")
            && !map.contains_key("additionalProperties")
            && !map.contains_key("enum")
            && !map.contains_key("const")
    })
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
