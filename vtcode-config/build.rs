use anyhow::{Context, Result};
use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
};

const EMBEDDED_OPENROUTER_MODELS: &str = include_str!("build_data/openrouter_models.json");

fn main() {
    if let Err(error) = generate_artifacts() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn generate_artifacts() -> Result<()> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let provider = load_provider_metadata(&manifest_dir)?;

    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let entries = provider.collect_entries()?;

    write_variants(&out_dir, &entries)?;
    write_constants(&out_dir, &provider, &entries)?;
    write_aliases(&out_dir, &entries)?;
    write_metadata(&out_dir, &entries)?;

    Ok(())
}

fn load_provider_metadata(manifest_dir: &Path) -> Result<Provider> {
    let docs_path = manifest_dir.join("../docs/models.json");
    if docs_path.exists() {
        println!("cargo:rerun-if-changed={}", docs_path.display());
        let models_source = fs::read_to_string(&docs_path)
            .with_context(|| format!("Failed to read {}", docs_path.display()))?;

        let root: Value = serde_json::from_str(&models_source)
            .context("Failed to parse docs/models.json as JSON")?;
        let openrouter_value = root
            .get("openrouter")
            .cloned()
            .context("docs/models.json is missing the openrouter provider section")?;

        serde_json::from_value(openrouter_value)
            .context("Failed to deserialize openrouter provider metadata")
    } else {
        serde_json::from_str(EMBEDDED_OPENROUTER_MODELS)
            .context("Failed to parse embedded OpenRouter model metadata")
    }
}

#[derive(Deserialize)]
struct Provider {
    #[serde(default)]
    default_model: Option<String>,
    models: IndexMap<String, ModelSpec>,
}

#[derive(Deserialize)]
struct ModelSpec {
    id: String,
    #[serde(default)]
    reasoning: bool,
    #[serde(default = "default_tool_call_true")]
    tool_call: bool,
    vtcode: Option<VtcodeSpec>,
}

fn default_tool_call_true() -> bool {
    true
}

#[derive(Deserialize)]
struct VtcodeSpec {
    variant: String,
    constant: String,
    vendor: String,
    display: String,
    description: String,
    efficient: bool,
    top_tier: bool,
    generation: String,
    #[serde(default)]
    doc_comment: Option<String>,
}

struct EntryData {
    variant: String,
    const_name: String,
    alias_name: String,
    id: String,
    vendor: String,
    display: String,
    description: String,
    efficient: bool,
    top_tier: bool,
    generation: String,
    doc_comment: String,
    reasoning: bool,
    tool_call: bool,
}

impl Provider {
    fn collect_entries(&self) -> Result<Vec<EntryData>> {
        let mut seen_constants = HashMap::new();
        let mut entries = Vec::with_capacity(self.models.len());

        for (model_id, spec) in &self.models {
            let vtcode = spec.vtcode.as_ref().with_context(|| {
                format!("Missing vtcode metadata for openrouter model '{model_id}'")
            })?;

            let const_name = vtcode.constant.trim().to_string();
            if const_name.is_empty() {
                anyhow::bail!("vtcode constant name missing for model '{model_id}'");
            }
            if let Some(existing_id) = seen_constants.insert(const_name.clone(), model_id) {
                anyhow::bail!(
                    "Duplicate constant '{const_name}' for models '{existing_id}' and '{model_id}'"
                );
            }

            let doc_comment =
                vtcode
                    .doc_comment
                    .as_deref()
                    .unwrap_or(if vtcode.description.is_empty() {
                        vtcode.display.as_str()
                    } else {
                        ""
                    });

            let computed_comment = if doc_comment.is_empty() {
                format!("{} - {}", vtcode.display, vtcode.description)
            } else {
                doc_comment.to_string()
            };

            entries.push(EntryData {
                variant: vtcode.variant.clone(),
                const_name: const_name.clone(),
                alias_name: format!("OPENROUTER_{const_name}"),
                id: spec.id.clone(),
                vendor: vtcode.vendor.to_lowercase(),
                display: vtcode.display.clone(),
                description: vtcode.description.clone(),
                efficient: vtcode.efficient,
                top_tier: vtcode.top_tier,
                generation: vtcode.generation.clone(),
                doc_comment: computed_comment,
                reasoning: spec.reasoning,
                tool_call: spec.tool_call,
            });
        }

        Ok(entries)
    }
}

fn write_variants(out_dir: &Path, entries: &[EntryData]) -> Result<()> {
    let mut content = String::new();
    for entry in entries {
        content.push_str("    /// ");
        content.push_str(&sanitize_doc_comment(&entry.doc_comment));
        content.push('\n');
        content.push_str("    ");
        content.push_str(&entry.variant);
        content.push_str(",\n");
    }
    fs::write(out_dir.join("openrouter_model_variants.rs"), content)
        .context("Failed to write generated OpenRouter model variants")
}

fn write_constants(out_dir: &Path, provider: &Provider, entries: &[EntryData]) -> Result<()> {
    let mut constants = String::new();
    for entry in entries {
        constants.push_str("    pub const ");
        constants.push_str(&entry.const_name);
        constants.push(':');
        constants.push_str(" &str = \"");
        constants.push_str(&entry.id);
        constants.push_str("\";\n");
    }
    constants.push('\n');

    let default_id = provider
        .default_model
        .as_ref()
        .context("openrouter.default_model is missing in docs/models.json")?;
    let default_const = entries
        .iter()
        .find(|entry| &entry.id == default_id)
        .with_context(|| {
            format!("Default OpenRouter model '{default_id}' is not declared in vtcode metadata")
        })?;
    constants.push_str("    pub const DEFAULT_MODEL: &str = ");
    constants.push_str(&default_const.const_name);
    constants.push_str(";\n\n");

    constants.push_str("    pub const SUPPORTED_MODELS: &[&str] = &[\n");
    for entry in entries {
        constants.push_str("        ");
        constants.push_str(&entry.const_name);
        constants.push_str(",\n");
    }
    constants.push_str("    ];\n\n");

    constants.push_str("    pub const REASONING_MODELS: &[&str] = &[\n");
    for entry in entries.iter().filter(|entry| entry.reasoning) {
        constants.push_str("        ");
        constants.push_str(&entry.const_name);
        constants.push_str(",\n");
    }
    constants.push_str("    ];\n\n");

    constants.push_str("    pub const TOOL_UNAVAILABLE_MODELS: &[&str] = &[\n");
    for entry in entries.iter().filter(|entry| !entry.tool_call) {
        constants.push_str("        ");
        constants.push_str(&entry.const_name);
        constants.push_str(",\n");
    }
    constants.push_str("    ];\n\n");

    let mut vendor_map: IndexMap<String, Vec<&EntryData>> = IndexMap::new();
    for entry in entries {
        vendor_map
            .entry(entry.vendor.clone())
            .or_default()
            .push(entry);
    }

    constants.push_str("    pub mod vendor {\n");
    for (vendor, vendor_entries) in vendor_map.iter() {
        let module_name = to_module_name(vendor);
        constants.push_str("        pub mod ");
        constants.push_str(&module_name);
        constants.push_str(" {\n");
        constants.push_str("            pub const MODELS: &[&str] = &[\n");
        for entry in vendor_entries {
            constants.push_str("                super::super::");
            constants.push_str(&entry.const_name);
            constants.push_str(",\n");
        }
        constants.push_str("            ];\n");
        constants.push_str("        }\n\n");
    }
    constants.push_str("    }\n");

    fs::write(out_dir.join("openrouter_constants.rs"), constants)
        .context("Failed to write generated OpenRouter constants")
}

fn write_aliases(out_dir: &Path, entries: &[EntryData]) -> Result<()> {
    let mut aliases = String::new();
    for entry in entries {
        aliases.push_str("    pub const ");
        aliases.push_str(&entry.alias_name);
        aliases.push_str(": &str = openrouter::");
        aliases.push_str(&entry.const_name);
        aliases.push_str(";\n");
    }
    fs::write(out_dir.join("openrouter_aliases.rs"), aliases)
        .context("Failed to write generated OpenRouter aliases")
}

fn write_metadata(out_dir: &Path, entries: &[EntryData]) -> Result<()> {
    let mut metadata = String::new();
    metadata.push_str("#[derive(Clone, Copy)]\n");
    metadata.push_str("pub struct Entry {\n");
    metadata.push_str("    pub variant: super::ModelId,\n");
    metadata.push_str("    pub id: &'static str,\n");
    metadata.push_str("    pub vendor: &'static str,\n");
    metadata.push_str("    pub display: &'static str,\n");
    metadata.push_str("    pub description: &'static str,\n");
    metadata.push_str("    pub efficient: bool,\n");
    metadata.push_str("    pub top_tier: bool,\n");
    metadata.push_str("    pub generation: &'static str,\n");
    metadata.push_str("    pub reasoning: bool,\n");
    metadata.push_str("    pub tool_call: bool,\n");
    metadata.push_str("}\n\n");

    metadata.push_str("pub const ENTRIES: &[Entry] = &[\n");
    for entry in entries {
        metadata.push_str("    Entry {\n");
        metadata.push_str("        variant: super::ModelId::");
        metadata.push_str(&entry.variant);
        metadata.push_str(",\n");
        metadata.push_str("        id: crate::constants::models::openrouter::");
        metadata.push_str(&entry.const_name);
        metadata.push_str(",\n");
        metadata.push_str("        vendor: \"");
        metadata.push_str(&entry.vendor);
        metadata.push_str("\",\n");
        metadata.push_str("        display: \"");
        metadata.push_str(&escape_rust_string(&entry.display));
        metadata.push_str("\",\n");
        metadata.push_str("        description: \"");
        metadata.push_str(&escape_rust_string(&entry.description));
        metadata.push_str("\",\n");
        metadata.push_str("        efficient: ");
        metadata.push_str(if entry.efficient { "true" } else { "false" });
        metadata.push_str(",\n");
        metadata.push_str("        top_tier: ");
        metadata.push_str(if entry.top_tier { "true" } else { "false" });
        metadata.push_str(",\n");
        metadata.push_str("        generation: \"");
        metadata.push_str(&escape_rust_string(&entry.generation));
        metadata.push_str("\",\n");
        metadata.push_str("        reasoning: ");
        metadata.push_str(if entry.reasoning { "true" } else { "false" });
        metadata.push_str(",\n");
        metadata.push_str("        tool_call: ");
        metadata.push_str(if entry.tool_call { "true" } else { "false" });
        metadata.push_str(",\n");
        metadata.push_str("    },\n");
    }
    metadata.push_str(
        "];

#[derive(Clone, Copy)]
pub struct VendorModels {
    pub vendor: &'static str,
    pub models: &'static [super::ModelId],
}

pub const VENDOR_MODELS: &[VendorModels] = &[
",
    );

    let mut vendor_map: IndexMap<String, Vec<&EntryData>> = IndexMap::new();
    for entry in entries {
        vendor_map
            .entry(entry.vendor.clone())
            .or_default()
            .push(entry);
    }

    for (vendor, vendor_entries) in vendor_map.iter() {
        metadata.push_str("    VendorModels {\n");
        metadata.push_str("        vendor: \"");
        metadata.push_str(vendor);
        metadata.push_str("\",\n");
        metadata.push_str("        models: &[\n");
        for entry in vendor_entries {
            metadata.push_str("            super::ModelId::");
            metadata.push_str(&entry.variant);
            metadata.push_str(",\n");
        }
        metadata.push_str("        ],\n    },\n");
    }
    metadata.push_str(
        "];

pub fn metadata_for(model: super::ModelId) -> Option<super::OpenRouterMetadata> {
    ENTRIES.iter().find(|entry| entry.variant == model).map(|entry| super::OpenRouterMetadata {
        id: entry.id,
        vendor: entry.vendor,
        display: entry.display,
        description: entry.description,
        efficient: entry.efficient,
        top_tier: entry.top_tier,
        generation: entry.generation,
        reasoning: entry.reasoning,
        tool_call: entry.tool_call,
    })
}

pub fn parse_model(value: &str) -> Option<super::ModelId> {
    ENTRIES.iter().find(|entry| entry.id == value).map(|entry| entry.variant)
}

pub fn vendor_groups() -> &'static [VendorModels] {
    VENDOR_MODELS
}
",
    );

    fs::write(out_dir.join("openrouter_metadata.rs"), metadata)
        .context("Failed to write generated OpenRouter metadata")
}

fn sanitize_doc_comment(input: &str) -> String {
    input.replace('\n', " ")
}

fn escape_rust_string(input: &str) -> String {
    input
        .chars()
        .flat_map(|ch| match ch {
            '\\' => "\\\\".chars().collect::<Vec<_>>(),
            '"' => "\\\"".chars().collect::<Vec<_>>(),
            '\n' => "\\n".chars().collect::<Vec<_>>(),
            '\r' => "\\r".chars().collect::<Vec<_>>(),
            '\t' => "\\t".chars().collect::<Vec<_>>(),
            other => vec![other],
        })
        .collect()
}

fn to_module_name(vendor: &str) -> String {
    let mut output = String::with_capacity(vendor.len());
    for ch in vendor.chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch.to_ascii_lowercase());
        } else {
            output.push('_');
        }
    }
    if output.is_empty() {
        return "vendor".to_string();
    }
    if output.chars().next().unwrap().is_ascii_digit() {
        format!("vendor_{output}")
    } else {
        output
    }
}
