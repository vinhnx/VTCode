use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::HashSet;
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};
use vtcode_core::cli::args::{SchemaCommands, SchemaMode, SchemaOutputFormat};
use vtcode_core::config::ToolDocumentationMode;
use vtcode_core::config::types::CapabilityLevel;
use vtcode_core::tools::ToolRegistry;
use vtcode_core::tools::handlers::{SessionSurface, SessionToolsConfig, ToolModelCapabilities};

#[derive(Debug, Clone, Serialize, PartialEq)]
struct ToolSchemaEntry {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct ToolsSchemaDocument {
    version: &'static str,
    generated_at_unix_secs: u64,
    mode: &'static str,
    tools: Vec<ToolSchemaEntry>,
}

pub async fn handle_schema_command(command: SchemaCommands) -> Result<()> {
    match command {
        SchemaCommands::Tools {
            mode,
            format,
            names,
        } => emit_tools_schema(mode, format, &names).await,
    }
}

async fn emit_tools_schema(
    mode: SchemaMode,
    format: SchemaOutputFormat,
    names: &[String],
) -> Result<()> {
    let tool_mode = to_tool_documentation_mode(mode);
    let workspace = env::current_dir().context("failed to resolve current working directory")?;
    let registry = ToolRegistry::new(workspace).await;
    let mut tools: Vec<ToolSchemaEntry> = registry
        .schema_entries(SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            tool_mode,
            ToolModelCapabilities::default(),
        ))
        .await
        .into_iter()
        .map(|entry| ToolSchemaEntry {
            name: entry.name,
            description: entry.description,
            parameters: entry.parameters,
        })
        .collect();

    tools.sort_by(|left, right| left.name.cmp(&right.name));
    tools = filter_tools_by_name(tools, names);

    match format {
        SchemaOutputFormat::Json => {
            let generated_at_unix_secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .context("system clock is before unix epoch")?
                .as_secs();
            let payload = ToolsSchemaDocument {
                version: env!("CARGO_PKG_VERSION"),
                generated_at_unix_secs,
                mode: schema_mode_label(mode),
                tools,
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&payload)
                    .context("failed to serialize tool schema document")?
            );
        }
        SchemaOutputFormat::Ndjson => {
            for tool in tools {
                println!(
                    "{}",
                    serde_json::to_string(&tool).context("failed to serialize tool schema row")?
                );
            }
        }
    }

    Ok(())
}

fn filter_tools_by_name(tools: Vec<ToolSchemaEntry>, names: &[String]) -> Vec<ToolSchemaEntry> {
    if names.is_empty() {
        return tools;
    }

    let allowed: HashSet<&str> = names.iter().map(String::as_str).collect();
    tools
        .into_iter()
        .filter(|tool| allowed.contains(tool.name.as_str()))
        .collect()
}

fn to_tool_documentation_mode(mode: SchemaMode) -> ToolDocumentationMode {
    match mode {
        SchemaMode::Minimal => ToolDocumentationMode::Minimal,
        SchemaMode::Progressive => ToolDocumentationMode::Progressive,
        SchemaMode::Full => ToolDocumentationMode::Full,
    }
}

fn schema_mode_label(mode: SchemaMode) -> &'static str {
    match mode {
        SchemaMode::Minimal => "minimal",
        SchemaMode::Progressive => "progressive",
        SchemaMode::Full => "full",
    }
}

#[cfg(test)]
mod tests {
    use super::{ToolSchemaEntry, filter_tools_by_name, to_tool_documentation_mode};
    use vtcode_core::cli::args::SchemaMode;
    use vtcode_core::config::ToolDocumentationMode;

    #[test]
    fn schema_mode_maps_to_tool_documentation_mode() {
        assert_eq!(
            to_tool_documentation_mode(SchemaMode::Minimal),
            ToolDocumentationMode::Minimal
        );
        assert_eq!(
            to_tool_documentation_mode(SchemaMode::Progressive),
            ToolDocumentationMode::Progressive
        );
        assert_eq!(
            to_tool_documentation_mode(SchemaMode::Full),
            ToolDocumentationMode::Full
        );
    }

    #[test]
    fn filter_tools_keeps_all_when_names_empty() {
        let tools = vec![
            ToolSchemaEntry {
                name: "unified_file".to_string(),
                description: "File operations".to_string(),
                parameters: serde_json::json!({}),
            },
            ToolSchemaEntry {
                name: "unified_exec".to_string(),
                description: "Command execution".to_string(),
                parameters: serde_json::json!({}),
            },
        ];

        let filtered = filter_tools_by_name(tools.clone(), &[]);
        assert_eq!(filtered, tools);
    }

    #[test]
    fn filter_tools_selects_exact_name_matches() {
        let tools = vec![
            ToolSchemaEntry {
                name: "unified_file".to_string(),
                description: "File operations".to_string(),
                parameters: serde_json::json!({}),
            },
            ToolSchemaEntry {
                name: "unified_exec".to_string(),
                description: "Command execution".to_string(),
                parameters: serde_json::json!({}),
            },
        ];

        let filtered = filter_tools_by_name(tools, &[String::from("unified_file")]);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "unified_file");
    }
}
