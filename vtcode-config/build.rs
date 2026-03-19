use anyhow::{Context as _, Result};
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
    let is_docsrs = env::var_os("DOCS_RS").is_some();

    if is_docsrs {
        // When building on docs.rs, generate empty placeholder files to prevent compilation errors
        println!("cargo:warning=docs.rs build detected, generating placeholder files");
        generate_placeholder_artifacts();
    } else if let Err(error) = generate_artifacts() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn generate_placeholder_artifacts() {
    use std::path::PathBuf;

    let out_dir = match env::var("OUT_DIR") {
        Ok(path) => PathBuf::from(path),
        Err(error) => {
            eprintln!("warning: OUT_DIR not set during docs.rs placeholder generation: {error}");
            return;
        }
    };

    // Write empty files to prevent "file not found" errors during docs.rs build
    if let Err(error) = fs::write(
        out_dir.join("openrouter_model_variants.rs"),
        "// Placeholder for docs.rs build\n",
    ) {
        eprintln!("warning: failed to write placeholder variants: {error}");
    }
    if let Err(error) = fs::write(
        out_dir.join("openrouter_constants.rs"),
        "    // Placeholder for docs.rs build\n    pub const DEFAULT_MODEL: &str = \"openrouter/auto\";\n    pub const SUPPORTED_MODELS: &[&str] = &[];\n    pub const REASONING_MODELS: &[&str] = &[];\n    pub const TOOL_UNAVAILABLE_MODELS: &[&str] = &[];\n    pub mod vendor {\n        pub mod openrouter {\n            pub const MODELS: &[&str] = &[];\n        }\n    }\n",
    ) {
        eprintln!("warning: failed to write placeholder constants: {error}");
    }
    if let Err(error) = fs::write(
        out_dir.join("openrouter_aliases.rs"),
        "// Placeholder for docs.rs build\n",
    ) {
        eprintln!("warning: failed to write placeholder aliases: {error}");
    }
    if let Err(error) = fs::write(
        out_dir.join("openrouter_metadata.rs"),
        "// Placeholder for docs.rs build\n#[derive(Clone, Copy)]\npub struct Entry {\n    pub variant: super::ModelId,\n    pub id: &'static str,\n    pub vendor: &'static str,\n    pub display: &'static str,\n    pub description: &'static str,\n    pub efficient: bool,\n    pub top_tier: bool,\n    pub generation: &'static str,\n    pub reasoning: bool,\n    pub tool_call: bool,\n}\n\npub const ENTRIES: &[Entry] = &[];\n\n#[derive(Clone, Copy)]\npub struct VendorModels {\n    pub vendor: &'static str,\n    pub models: &'static [super::ModelId],\n}\n\npub const VENDOR_MODELS: &[VendorModels] = &[];\n\npub fn metadata_for(_model: super::ModelId) -> Option<super::OpenRouterMetadata> { None }\n\npub fn parse_model(_value: &str) -> Option<super::ModelId> { None }\n\npub fn vendor_groups() -> &'static [VendorModels] { VENDOR_MODELS }\n",
    ) {
        eprintln!("warning: failed to write placeholder metadata: {error}");
    }
    if let Err(error) = fs::write(
        out_dir.join("model_capabilities.rs"),
        "// Placeholder for docs.rs build\n#[derive(Clone, Copy)]\npub struct Entry {\n    pub provider: &'static str,\n    pub id: &'static str,\n    pub tool_call: bool,\n    pub input_modalities: &'static [&'static str],\n}\n\npub const ENTRIES: &[Entry] = &[];\n\npub fn metadata_for(_provider: &str, _id: &str) -> Option<Entry> { None }\n",
    ) {
        eprintln!("warning: failed to write capability metadata: {error}");
    }
}

fn generate_artifacts() -> Result<()> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let provider = load_provider_metadata(&manifest_dir)?;
    let capability_entries = load_model_capability_entries(&manifest_dir)?;

    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let entries = provider.collect_entries()?;

    write_variants(&out_dir, &entries)?;
    write_constants(&out_dir, &provider, &entries)?;
    write_aliases(&out_dir, &entries)?;
    write_metadata(&out_dir, &entries)?;
    write_model_capabilities(&out_dir, &capability_entries)?;

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
        // Fallback to embedded models if docs/models.json is unavailable.
        // If docs/models.json exists but contains entries that we don't have enum variants for
        // (e.g., experimental listings), prefer the embedded set by returning an error early.
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

#[derive(Deserialize)]
struct ProviderCatalog {
    #[serde(default)]
    models: IndexMap<String, CapabilityModelSpec>,
}

#[derive(Deserialize)]
struct CapabilityModelSpec {
    id: String,
    #[serde(default)]
    context: usize,
    #[serde(default = "default_tool_call_true")]
    tool_call: bool,
    #[serde(default)]
    modalities: CapabilityModalities,
}

#[derive(Default, Deserialize)]
struct CapabilityModalities {
    #[serde(default)]
    input: Vec<String>,
}

struct CapabilityEntry {
    provider: String,
    id: String,
    context_window: usize,
    tool_call: bool,
    input_modalities: Vec<String>,
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

fn load_model_capability_entries(manifest_dir: &Path) -> Result<Vec<CapabilityEntry>> {
    let docs_path = manifest_dir.join("../docs/models.json");
    if !docs_path.exists() {
        return Ok(Vec::new());
    }

    println!("cargo:rerun-if-changed={}", docs_path.display());
    let models_source = fs::read_to_string(&docs_path)
        .with_context(|| format!("Failed to read {}", docs_path.display()))?;
    let providers: IndexMap<String, ProviderCatalog> = serde_json::from_str(&models_source)
        .context("Failed to deserialize docs/models.json providers")?;

    let mut entries = Vec::new();
    for (provider_key, provider) in providers {
        let provider_key = canonical_provider_key(&provider_key);
        for spec in provider.models.into_values() {
            entries.push(CapabilityEntry {
                provider: provider_key.to_string(),
                id: spec.id,
                context_window: spec.context,
                tool_call: spec.tool_call,
                input_modalities: spec.modalities.input,
            });
        }
    }

    entries.sort_by(|left, right| {
        left.provider
            .cmp(&right.provider)
            .then(left.id.cmp(&right.id))
    });

    Ok(entries)
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

    metadata.push_str("#[allow(dead_code)]\npub const ENTRIES: &[Entry] = &[\n");
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
    match model {
",
    );
    for entry in entries {
        metadata.push_str("        super::ModelId::");
        metadata.push_str(&entry.variant);
        metadata.push_str(" => Some(super::OpenRouterMetadata {\n");
        metadata.push_str("            id: crate::constants::models::openrouter::");
        metadata.push_str(&entry.const_name);
        metadata.push_str(",\n");
        metadata.push_str("            vendor: \"");
        metadata.push_str(&entry.vendor);
        metadata.push_str("\",\n");
        metadata.push_str("            display: \"");
        metadata.push_str(&escape_rust_string(&entry.display));
        metadata.push_str("\",\n");
        metadata.push_str("            description: \"");
        metadata.push_str(&escape_rust_string(&entry.description));
        metadata.push_str("\",\n");
        metadata.push_str("            efficient: ");
        metadata.push_str(if entry.efficient { "true" } else { "false" });
        metadata.push_str(",\n");
        metadata.push_str("            top_tier: ");
        metadata.push_str(if entry.top_tier { "true" } else { "false" });
        metadata.push_str(",\n");
        metadata.push_str("            generation: \"");
        metadata.push_str(&escape_rust_string(&entry.generation));
        metadata.push_str("\",\n");
        metadata.push_str("            reasoning: ");
        metadata.push_str(if entry.reasoning { "true" } else { "false" });
        metadata.push_str(",\n");
        metadata.push_str("            tool_call: ");
        metadata.push_str(if entry.tool_call { "true" } else { "false" });
        metadata.push_str(",\n");
        metadata.push_str("        }),\n");
    }
    metadata.push_str(
        "        _ => None,
    }
}

pub fn parse_model(value: &str) -> Option<super::ModelId> {
    match value {
",
    );
    for entry in entries {
        metadata.push_str("        crate::constants::models::openrouter::");
        metadata.push_str(&entry.const_name);
        metadata.push_str(" => Some(super::ModelId::");
        metadata.push_str(&entry.variant);
        metadata.push_str("),\n");
    }
    metadata.push_str(
        "        _ => None,
    }
}

pub fn vendor_groups() -> &'static [VendorModels] {
    VENDOR_MODELS
}
",
    );

    fs::write(out_dir.join("openrouter_metadata.rs"), metadata)
        .context("Failed to write generated OpenRouter metadata")
}

fn write_model_capabilities(out_dir: &Path, entries: &[CapabilityEntry]) -> Result<()> {
    let mut metadata = String::new();
    metadata.push_str("#[derive(Clone, Copy)]\n");
    metadata.push_str("pub struct Entry {\n");
    metadata.push_str("    pub provider: &'static str,\n");
    metadata.push_str("    pub id: &'static str,\n");
    metadata.push_str("    pub context_window: usize,\n");
    metadata.push_str("    pub tool_call: bool,\n");
    metadata.push_str("    pub input_modalities: &'static [&'static str],\n");
    metadata.push_str("}\n\n");

    metadata.push_str("#[allow(dead_code)]\npub const ENTRIES: &[Entry] = &[\n");
    for entry in entries {
        metadata.push_str("    Entry {\n");
        metadata.push_str("        provider: \"");
        metadata.push_str(&escape_rust_string(&entry.provider));
        metadata.push_str("\",\n");
        metadata.push_str("        id: \"");
        metadata.push_str(&escape_rust_string(&entry.id));
        metadata.push_str("\",\n");
        metadata.push_str("        context_window: ");
        metadata.push_str(&entry.context_window.to_string());
        metadata.push_str(",\n");
        metadata.push_str("        tool_call: ");
        metadata.push_str(if entry.tool_call { "true" } else { "false" });
        metadata.push_str(",\n");
        metadata.push_str("        input_modalities: &[\n");
        for modality in &entry.input_modalities {
            metadata.push_str("            \"");
            metadata.push_str(&escape_rust_string(modality));
            metadata.push_str("\",\n");
        }
        metadata.push_str("        ],\n");
        metadata.push_str("    },\n");
    }
    let mut provider_map: IndexMap<&str, Vec<&CapabilityEntry>> = IndexMap::new();
    for entry in entries {
        provider_map.entry(&entry.provider).or_default().push(entry);
    }

    metadata.push_str("];\n\npub const PROVIDERS: &[&str] = &[\n");
    for provider in provider_map.keys() {
        metadata.push_str("    \"");
        metadata.push_str(provider);
        metadata.push_str("\",\n");
    }
    metadata.push_str("];\n\n");

    metadata.push_str(
        "pub fn metadata_for(provider: &str, id: &str) -> Option<Entry> {\n    match provider {\n",
    );
    for (provider, provider_entries) in &provider_map {
        metadata.push_str("        \"");
        metadata.push_str(provider);
        metadata.push_str("\" => match id {\n");
        for entry in provider_entries {
            metadata.push_str("            \"");
            metadata.push_str(&escape_rust_string(&entry.id));
            metadata.push_str("\" => Some(Entry {\n");
            metadata.push_str("                provider: \"");
            metadata.push_str(provider);
            metadata.push_str("\",\n");
            metadata.push_str("                id: \"");
            metadata.push_str(&escape_rust_string(&entry.id));
            metadata.push_str("\",\n");
            metadata.push_str("                context_window: ");
            metadata.push_str(&entry.context_window.to_string());
            metadata.push_str(",\n");
            metadata.push_str("                tool_call: ");
            metadata.push_str(if entry.tool_call { "true" } else { "false" });
            metadata.push_str(",\n");
            metadata.push_str("                input_modalities: &[\n");
            for modality in &entry.input_modalities {
                metadata.push_str("                    \"");
                metadata.push_str(&escape_rust_string(modality));
                metadata.push_str("\",\n");
            }
            metadata.push_str("                ],\n");
            metadata.push_str("            }),\n");
        }
        metadata.push_str("            _ => None,\n");
        metadata.push_str("        },\n");
    }
    metadata.push_str("        _ => None,\n    }\n}\n\n");

    metadata.push_str("pub fn models_for_provider(provider: &str) -> Option<&'static [&'static str]> {\n    match provider {\n");
    for (provider, provider_entries) in &provider_map {
        metadata.push_str("        \"");
        metadata.push_str(provider);
        metadata.push_str("\" => Some(&[\n");
        for entry in provider_entries {
            metadata.push_str("            \"");
            metadata.push_str(&escape_rust_string(&entry.id));
            metadata.push_str("\",\n");
        }
        metadata.push_str("        ]),\n");
    }
    metadata.push_str("        _ => None,\n    }\n}\n");

    fs::write(out_dir.join("model_capabilities.rs"), metadata)
        .context("Failed to write generated model capability metadata")
}

fn sanitize_doc_comment(input: &str) -> String {
    input.replace('\n', " ")
}

fn escape_rust_string(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\\' => output.push_str("\\\\"),
            '"' => output.push_str("\\\""),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            other => output.push(other),
        }
    }
    output
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
    if output.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        format!("vendor_{output}")
    } else {
        output
    }
}

fn canonical_provider_key(provider: &str) -> &str {
    match provider {
        "google" => "gemini",
        other => other,
    }
}
