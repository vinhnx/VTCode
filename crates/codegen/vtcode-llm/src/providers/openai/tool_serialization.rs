//! Tool serialization helpers for OpenAI payloads.
//!
//! Keeps tool JSON shaping isolated from the main provider logic.
//! This is a standalone copy of the core tool_serialization module,
//! adapted to compile within vtcode-llm without depending on vtcode-core.

use crate::provider;
use hashbrown::HashSet;
use serde_json::{Value, json};
use vtcode_config::constants::tools;
use vtcode_config::core::{
    OpenAIHostedShellConfig, OpenAIHostedShellDomainSecret, OpenAIHostedShellEnvironment,
    OpenAIHostedShellNetworkPolicy, OpenAIHostedShellNetworkPolicyType, OpenAIHostedSkill, OpenAIHostedSkillVersion,
};
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

fn serialize_responses_function_tool(func: &provider::FunctionDefinition, defer_loading: bool) -> Value {
    let mut value = json!({
        "type": "function",
        "strict": false,
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
        tools::EXEC_COMMAND
            | tools::APPLY_PATCH
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
            | tools::WRITE_STDIN
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
            | tools::SEARCH_REPLACE
            | tools::FILE_OP
            | tools::MOVE_FILE
            | tools::COPY_FILE
            | tools::GET_ERRORS
            | tools::REQUEST_USER_INPUT
            | tools::MEMORY
            | tools::ASK_QUESTIONS
            | tools::ASK_USER_QUESTION
            | tools::CRON
            | tools::CRON_CREATE
            | tools::CRON_LIST
            | tools::CRON_DELETE
            | tools::MCP
            | tools::MCP_SEARCH_TOOLS
            | tools::MCP_GET_TOOL_DETAILS
            | tools::MCP_LIST_SERVERS
            | tools::START_PLANNING
            | tools::TASK_TRACKER
            | tools::AGENT
            | tools::SPAWN_AGENT
            | tools::SPAWN_BACKGROUND_SUBPROCESS
            | tools::SEND_INPUT
            | tools::WAIT_AGENT
            | tools::RESUME_AGENT
            | tools::CLOSE_AGENT
    )
}

pub(crate) fn sanitize_openai_function_parameters(value: Value, strip_any_of: bool) -> Value {
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
                    *schema = sanitize_openai_schema_node(parse_tool_input_schema(schema), strip_any_of);
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
                    other => sanitize_openai_schema_node(parse_tool_input_schema(&other), strip_any_of),
                };
            }

            if let Some(additional_properties) = map.get_mut("additionalProperties") {
                if matches!(additional_properties, Value::Bool(true)) {
                    *additional_properties = json!({ "type": "string" });
                } else if !matches!(additional_properties, Value::Bool(_)) {
                    *additional_properties =
                        sanitize_openai_schema_node(parse_tool_input_schema(additional_properties), strip_any_of);
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

            if map.get("type").and_then(Value::as_str) == Some("object") {
                map.entry("properties".to_string()).or_insert_with(|| json!({}));
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

fn serialize_openai_hosted_shell_network_policy(policy: &OpenAIHostedShellNetworkPolicy) -> Option<Value> {
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
            if let Some(network_policy) = serialize_openai_hosted_shell_network_policy(&config.network_policy) {
                environment.insert("network_policy".to_string(), network_policy);
            }

            let file_ids: Vec<String> =
                config.file_ids.iter().filter_map(|value| trim_non_empty_owned(value)).collect();
            if !file_ids.is_empty() {
                environment.insert("file_ids".to_string(), json!(file_ids));
            }

            let skills: Vec<Value> = config.skills.iter().filter_map(serialize_hosted_skill).collect();
            if !skills.is_empty() {
                environment.insert("skills".to_string(), Value::Array(skills));
            }
        }
        OpenAIHostedShellEnvironment::ContainerReference => {
            let container_id = config.container_id.as_deref().and_then(trim_non_empty_owned)?;
            environment.insert("container_id".to_string(), json!(container_id));
        }
    }

    Some(json!({
        "type": "shell",
        "environment": Value::Object(environment),
    }))
}

/// NOTE: unlike `serialize_tools_for_responses` below, the Chat path keeps
/// `defer_loading` markers as-is; only the Responses path strips them when no
/// `tool_search` is present. Keep this divergence — do not unify the two
/// without covering both wire contracts.
pub(crate) fn serialize_tools(tools: &[provider::ToolDefinition], model: &str) -> Option<Value> {
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
                    let parameters = sanitize_openai_function_parameters(
                        func.parameters.clone(),
                        should_strip_any_of_for_builtin_tool(name),
                    );
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
                        let parameters = sanitize_openai_function_parameters(
                            func.parameters.clone(),
                            should_strip_any_of_for_builtin_tool(&func.name),
                        );
                        json!({
                            "type": "function",
                            "function": {
                                "name": func.name,
                                "description": func.description,
                                "parameters": parameters
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

pub(crate) fn serialize_tools_for_responses(
    tools: &[provider::ToolDefinition],
    hosted_shell: Option<&OpenAIHostedShellConfig>,
) -> Option<Value> {
    if tools.is_empty() {
        return None;
    }

    let mut seen_names = HashSet::new();
    let mut serialized_tools = tools
        .iter()
        .filter_map(|tool| {
            let serialized = match tool.tool_type.as_str() {
                "function" => {
                    let func = tool.function.as_ref()?;
                    if func.name == tools::SHELL {
                        hosted_shell
                            .and_then(serialize_openai_hosted_shell)
                            .or_else(|| Some(serialize_responses_function_tool(func, tool.defer_loading == Some(true))))
                    } else {
                        Some(serialize_responses_function_tool(func, tool.defer_loading == Some(true)))
                    }
                }
                tools::APPLY_PATCH => {
                    if let Some(func) = tool.function.as_ref() {
                        Some(serialize_responses_function_tool(func, false))
                    } else {
                        Some(serialize_responses_function_tool(
                            &provider::FunctionDefinition {
                                name: tools::APPLY_PATCH.to_string(),
                                description: vtcode_utility_tool_specs::with_semantic_anchor_guidance(
                                    vtcode_utility_tool_specs::DEFAULT_APPLY_PATCH_INPUT_DESCRIPTION,
                                ),
                                parameters: vtcode_utility_tool_specs::apply_patch_parameter_schema(
                                    vtcode_utility_tool_specs::DEFAULT_APPLY_PATCH_INPUT_DESCRIPTION,
                                ),
                            },
                            false,
                        ))
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
                        "description": format!(
                            "Use the `apply_patch` tool to edit files. {} This is a FREEFORM tool.",
                            vtcode_utility_tool_specs::with_semantic_anchor_guidance(
                                vtcode_utility_tool_specs::DEFAULT_APPLY_PATCH_INPUT_DESCRIPTION
                            )
                        ),
                        "format": {
                            "type": "grammar",
                            "syntax": &grammar.syntax,
                            "definition": &grammar.definition
                        }
                    })
                }),
                "tool_search" => Some(json!({ "type": "tool_search" })),
                "web_search" => serialize_responses_hosted_tool("web_search", tool.web_search.as_ref()),
                "file_search" | "mcp" => {
                    serialize_responses_hosted_tool(tool.tool_type.as_str(), tool.hosted_tool_config.as_ref())
                }
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

    // `defer_loading` markers are only valid alongside a `tool_search`
    // discovery tool: backends reject deferred tools without one (400
    // "Deferred tools require tools.tool_search", and the deferred names
    // are then also unreferenceable from `allowed_tools` tool_choice).
    // Without a search tool on the wire there is no discovery mechanism,
    // so send eager definitions and keep every listed tool directly
    // callable. Core-side deferral state is untouched; this only shapes
    // the wire payload.
    let has_search_tool = serialized_tools
        .iter()
        .any(|tool| tool.get("type").and_then(Value::as_str) == Some("tool_search"));
    if !has_search_tool {
        for tool in &mut serialized_tools {
            if let Some(obj) = tool.as_object_mut() {
                obj.remove("defer_loading");
            }
        }
    }

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

#[cfg(test)]
mod tests {
    use super::serialize_tools_for_responses;
    use crate::provider::ToolDefinition;
    use serde_json::{Value, json};
    use vtcode_config::constants::tools;

    #[test]
    fn responses_functions_explicitly_opt_out_of_strict_normalization() {
        let function = ToolDefinition::function(
            "task_tracker".into(),
            "Tracker".into(),
            json!({"type":"object","properties":{"action":{"type":"string"},"index":{"type":"integer"}},"required":["action"]}),
        );
        let mut shell = function.clone();
        shell.function.as_mut().unwrap().name = tools::SHELL.into();
        let mut fallback = function.clone();
        fallback.tool_type = "legacy_tool".into();
        fallback.function.as_mut().unwrap().name = "legacy".into();
        let patch: ToolDefinition = serde_json::from_value(json!({"type":tools::APPLY_PATCH})).unwrap();
        let payload = serialize_tools_for_responses(&[function.clone(), shell, fallback, patch], None).unwrap();
        assert_eq!(payload.as_array().unwrap().len(), 4);
        for tool in payload.as_array().unwrap() {
            assert_eq!(tool["type"], "function");
            assert_eq!(tool["strict"], false);
        }
        assert_eq!(payload[0]["parameters"]["required"], json!(["action"]));
        let chat = super::serialize_tools(&[function], "gpt-6").unwrap();
        assert!(chat[0].get("strict").is_none());
        assert!(chat[0]["function"].get("strict").is_none());
    }

    #[test]
    fn responses_strictness_does_not_change_custom_or_hosted_contracts() {
        let mut custom =
            ToolDefinition::function("custom_input".into(), "Custom".into(), json!({"format":{"type":"text"}}));
        custom.tool_type = "custom".into();
        let hosted: ToolDefinition = serde_json::from_value(json!({"type":"web_search","web_search":{}})).unwrap();
        let payload = serialize_tools_for_responses(
            &[
                custom,
                hosted,
                ToolDefinition::grammar("lark".into(), "start: \"patch\"".into()),
            ],
            None,
        )
        .unwrap();
        assert_eq!(payload.as_array().unwrap().len(), 3);
        for tool in payload.as_array().unwrap() {
            assert!(tool.get("strict").is_none());
        }
        assert_eq!(payload[0]["type"], "custom");
        assert_eq!(payload[1]["type"], "web_search");
        assert_eq!(payload[2]["type"], "custom");
    }

    #[test]
    fn apply_patch_fallback_schema_requires_workspace_relative_paths() {
        let tool: ToolDefinition = serde_json::from_value(json!({
            "type": tools::APPLY_PATCH
        }))
        .expect("apply_patch tool definition");
        let serialized = serialize_tools_for_responses(&[tool], None).expect("serialized tool list");
        let patch_tool = &serialized[0];

        let description = patch_tool["description"].as_str().expect("tool description");
        let input_description = patch_tool["parameters"]["properties"]["input"]["description"]
            .as_str()
            .expect("input parameter description");

        for text in [description, input_description] {
            assert!(text.contains("workspace-relative"));
            assert!(text.contains("absolute paths"));
            assert!(text.contains("`..`"));
            assert!(text.contains("traversal-like forms"));
        }
    }

    #[test]
    fn apply_patch_grammar_description_requires_workspace_relative_paths() {
        let tool = ToolDefinition::grammar("lark".to_owned(), "start: \"patch\"".to_owned());
        let serialized = serialize_tools_for_responses(&[tool], None).expect("serialized tool list");
        let grammar_tool = &serialized[0];
        let description = grammar_tool["description"].as_str().expect("grammar description");

        assert!(description.contains("workspace-relative"));
        assert!(description.contains("absolute paths"));
        assert!(description.contains("`..`"));
        assert!(description.contains("traversal-like forms"));
    }

    #[test]
    fn deferred_markers_require_a_search_tool_on_the_wire() {
        let deferred = ToolDefinition::function(
            "search_docs".to_owned(),
            "Search internal docs".to_owned(),
            json!({"type":"object","properties":{"query":{"type":"string"}},"required":["query"],"additionalProperties":false}),
        )
        .with_defer_loading(true);

        // Without a `tool_search` discovery tool, backends reject deferred
        // tools (400 "Deferred tools require tools.tool_search"), so the
        // marker is stripped while the eager definition stays callable.
        let other =
            ToolDefinition::function("other_tool".to_owned(), "Another tool".to_owned(), json!({"type": "object"}))
                .with_defer_loading(true);
        let payload = serialize_tools_for_responses(&[deferred.clone(), other], None).expect("serialized tool list");
        let arr = payload.as_array().expect("tool array");
        assert_eq!(arr.len(), 2, "both definitions stay on the wire");
        for tool in arr {
            assert!(tool.get("defer_loading").is_none(), "defer_loading must be stripped without a search tool");
        }
        let tool = arr
            .iter()
            .find(|t| t.get("name").and_then(Value::as_str) == Some("search_docs"))
            .expect("stripped tool stays present");
        assert_eq!(tool["name"].as_str(), Some("search_docs"));
        // The eager definition must stay fully callable: description and
        // parameters survive stripping.
        assert_eq!(tool["description"].as_str(), Some("Search internal docs"));
        assert_eq!(
            tool["parameters"]["properties"]["query"]["type"].as_str(),
            Some("string"),
            "stripping must not touch the schema body"
        );

        // With a `tool_search` tool present, the deferral protocol is
        // intact and the marker is preserved.
        let payload = serialize_tools_for_responses(&[ToolDefinition::hosted_tool_search(), deferred], None)
            .expect("serialized tool list");
        let arr = payload.as_array().expect("tool array");
        assert!(arr.iter().any(|t| t.get("type").and_then(Value::as_str) == Some("tool_search")));
        let deferred = arr
            .iter()
            .find(|t| t.get("name").and_then(Value::as_str) == Some("search_docs"))
            .expect("deferred function should be present");
        assert_eq!(deferred["defer_loading"], json!(true));
    }
}
