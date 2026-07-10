use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::HashSet;
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};
use vtcode_core::cli::args::{SchemaCommands, SchemaMode, SchemaOutputFormat};
use vtcode_core::config::types::CapabilityLevel;
use vtcode_core::config::{ToolDocumentationMode, VTCodeConfig};
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

pub async fn handle_schema_command(
    command: SchemaCommands,
    config: &VTCodeConfig,
) -> Result<String> {
    match command {
        SchemaCommands::Tools {
            mode,
            format,
            names,
        } => render_tools_schema(mode, format, &names, config).await,
    }
}

async fn render_tools_schema(
    mode: SchemaMode,
    format: SchemaOutputFormat,
    names: &[String],
    config: &VTCodeConfig,
) -> Result<String> {
    let tools = collect_tools_schema(mode, names, config).await?;

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
            let payload = serde_json::to_string_pretty(&payload)
                .context("failed to serialize tool schema document")?;
            Ok(format!("{payload}\n"))
        }
        SchemaOutputFormat::Ndjson => {
            let mut output = String::new();
            for tool in tools {
                let row =
                    serde_json::to_string(&tool).context("failed to serialize tool schema row")?;
                output.push_str(&row);
                output.push('\n');
            }
            Ok(output)
        }
    }
}

async fn collect_tools_schema(
    mode: SchemaMode,
    names: &[String],
    config: &VTCodeConfig,
) -> Result<Vec<ToolSchemaEntry>> {
    let workspace = env::current_dir().context("failed to resolve current working directory")?;
    let registry = ToolRegistry::new(workspace).await;
    let mut tools: Vec<ToolSchemaEntry> = registry
        .schema_entries(
            SessionToolsConfig::full_public(
                SessionSurface::Interactive,
                CapabilityLevel::CodeSearch,
                to_tool_documentation_mode(mode),
                ToolModelCapabilities::default(),
            )
            .with_tool_profile(config.tools.profile),
        )
        .await
        .into_iter()
        .map(|entry| ToolSchemaEntry {
            name: entry.name,
            description: entry.description,
            parameters: entry.parameters,
        })
        .collect();

    tools.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(filter_tools_by_name(tools, names))
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
    use super::{
        ToolSchemaEntry, collect_tools_schema, filter_tools_by_name, to_tool_documentation_mode,
    };
    use vtcode_core::cli::args::SchemaMode;
    use vtcode_core::config::{ToolDocumentationMode, ToolProfile, VTCodeConfig};

    #[tokio::test]
    async fn tools_schema_uses_effective_tool_profile() {
        let names = vec![vtcode_core::config::constants::tools::CODE_SEARCH.to_string()];
        let baseline = collect_tools_schema(SchemaMode::Minimal, &names, &VTCodeConfig::default())
            .await
            .expect("baseline schema");
        let mut advanced_config = VTCodeConfig::default();
        advanced_config.tools.profile = ToolProfile::AdvancedVtCode;
        let advanced = collect_tools_schema(SchemaMode::Minimal, &names, &advanced_config)
            .await
            .expect("advanced schema");

        assert!(baseline.is_empty());
        assert_eq!(advanced.len(), 1);
        assert_eq!(advanced[0].name, names[0]);
    }

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
                name: "apply_patch".to_string(),
                description: "Patch editing".to_string(),
                parameters: serde_json::json!({}),
            },
            ToolSchemaEntry {
                name: "exec_command".to_string(),
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
                name: "apply_patch".to_string(),
                description: "Patch editing".to_string(),
                parameters: serde_json::json!({}),
            },
            ToolSchemaEntry {
                name: "exec_command".to_string(),
                description: "Command execution".to_string(),
                parameters: serde_json::json!({}),
            },
        ];

        let filtered = filter_tools_by_name(tools, &[String::from("apply_patch")]);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "apply_patch");
    }
}
