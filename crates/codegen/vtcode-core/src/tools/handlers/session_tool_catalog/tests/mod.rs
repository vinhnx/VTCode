pub(super) use super::*;
pub(super) use crate::config::VTCodeConfig;
pub(super) use crate::tools::constants::empty_object_schema;
pub(super) use crate::tools::registry::ToolRegistration;
pub(super) use crate::tools::request_user_input::RequestUserInputTool;
pub(super) use crate::tools::tool_intent::{ToolBehavior, ToolMutationModel};
pub(super) use crate::tools::traits::Tool;
pub(super) use serde_json::json;

fn registration(name: &'static str) -> ToolRegistration {
    ToolRegistration::new(name, CapabilityLevel::CodeSearch, false, |_, _| Box::pin(async { Ok(Value::Null) }))
}

fn contains_json_key(value: &Value, key: &str) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(name, value)| name == key || contains_json_key(value, key)),
        Value::Array(values) => values.iter().any(|value| contains_json_key(value, key)),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => false,
    }
}

/// Serialize the on-wire tool schemas (the definitions the model actually
/// receives) and estimate their token cost at ~4 chars/token, matching the
/// convention in `estimate_schema_tokens` and the first-request budget test
/// in `tools/registry/builtins.rs`. Deferred tools (`defer_loading ==
/// Some(true)`) are omitted under the client-local policy, so they never
/// reach the wire payload and are excluded here.
fn on_wire_schema_tokens(catalog: &SessionToolCatalog, config: SessionToolsConfig) -> usize {
    #[derive(Serialize)]
    struct Estimate<'a> {
        name: &'a str,
        description: &'a str,
        parameters: &'a Value,
    }
    let on_wire: FxHashSet<String> = catalog
        .model_tools(config.clone())
        .into_iter()
        .filter(|tool| tool.defer_loading != Some(true))
        .map(|tool| tool.function_name().to_string())
        .collect();
    catalog
        .schema_entries(config)
        .into_iter()
        .filter(|entry| on_wire.contains(&entry.name))
        .map(|entry| {
            serde_json::to_string(&Estimate {
                name: &entry.name,
                description: &entry.description,
                parameters: &entry.parameters,
            })
            .map(|s| s.len() / 4)
            .unwrap_or(0)
        })
        .sum()
}

/// Build a simulated MCP tool registration for `server`/`tool` with a
/// realistic one-line description and a two-parameter schema, so the eager
/// schema tax is pronounced enough to assert on.
fn mcp_server_tool_registration(server: &str, tool: &str, description: &str) -> ToolRegistration {
    let name: &'static str = Box::leak(format!("mcp::{server}::{tool}").into_boxed_str());
    let alias = format!("mcp__{server}__{tool}");
    registration(name)
        .with_catalog_source(ToolCatalogSource::Mcp)
        .with_llm_visibility(false)
        .with_description(description.to_string())
        .with_parameter_schema(json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "The lookup query." },
                "limit": { "type": "integer", "description": "Max results to return." }
            },
            "required": ["query"]
        }))
        .with_aliases([alias])
}

mod advanced_acp;
mod deferral_payloads;
mod namespaces_tokens;
mod profiles;
mod provider_policies;
mod serialization;
mod visibility;
