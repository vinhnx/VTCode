use anyhow::{Context as _, Result};
use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
};

mod build_codegen;

const EMBEDDED_OPENROUTER_MODELS: &str = include_str!("build_data/openrouter_models.json");

fn main() {
    let is_docsrs = env::var_os("DOCS_RS").is_some();

    // Force rebuild when embedded OpenRouter models change
    println!("cargo:rerun-if-changed=build_data/openrouter_models.json");

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
        out_dir.join("openrouter_constants.rs"),
        "    // Placeholder for docs.rs build\n    pub const DEFAULT_MODEL: &str = \"openrouter/auto\";\n    pub const SUPPORTED_MODELS: &[&str] = &[];\n    pub const REASONING_MODELS: &[&str] = &[];\n    pub const TOOL_UNAVAILABLE_MODELS: &[&str] = &[];\n    pub mod vendor {\n        pub mod openrouter {\n            pub const MODELS: &[&str] = &[];\n        }\n    }\n",
    ) {
        eprintln!("warning: failed to write placeholder constants: {error}");
    }
    if let Err(error) = fs::write(
        out_dir.join("openrouter_metadata.rs"),
        "// Placeholder for docs.rs build\n#[derive(Clone, Copy)]\npub struct Entry {\n    pub variant: super::ModelId,\n    pub id: &'static str,\n    pub vendor: &'static str,\n    pub display: &'static str,\n    pub description: &'static str,\n    pub efficient: bool,\n    pub top_tier: bool,\n    pub generation: &'static str,\n    pub reasoning: bool,\n    pub tool_call: bool,\n}\n\npub const ENTRIES: &[Entry] = &[];\n\n#[derive(Clone, Copy)]\npub struct VendorModels {\n    pub vendor: &'static str,\n    pub models: &'static [super::ModelId],\n}\n\npub const VENDOR_MODELS: &[VendorModels] = &[];\n\npub fn metadata_for(_model: super::ModelId) -> Option<super::OpenRouterMetadata> { None }\n\npub fn parse_model(_value: &str) -> Option<super::ModelId> { None }\n\npub fn vendor_groups() -> &'static [VendorModels] { VENDOR_MODELS }\n",
    ) {
        eprintln!("warning: failed to write placeholder metadata: {error}");
    }
    if let Err(error) = fs::write(
        out_dir.join("model_capabilities.rs"),
        "// Placeholder for docs.rs build\n#[derive(Clone, Copy)]\npub struct Pricing {\n    pub input: Option<f64>,\n    pub output: Option<f64>,\n    pub cache_read: Option<f64>,\n    pub cache_write: Option<f64>,\n}\n\n#[derive(Clone, Copy)]\npub struct Entry {\n    pub provider: &'static str,\n    pub id: &'static str,\n    pub display_name: &'static str,\n    pub description: &'static str,\n    pub context_window: usize,\n    pub max_output_tokens: Option<usize>,\n    pub reasoning: bool,\n    pub tool_call: bool,\n    pub vision: bool,\n    pub input_modalities: &'static [&'static str],\n    pub caching: bool,\n    pub structured_output: bool,\n    pub pricing: Pricing,\n}\n\npub const ENTRIES: &[Entry] = &[];\npub const PROVIDERS: &[&str] = &[];\n\npub fn metadata_for(_provider: &str, _id: &str) -> Option<Entry> { None }\npub fn models_for_provider(_provider: &str) -> Option<&'static [&'static str]> { None }\n",
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

    // Debug: count entries and check for deprecated models
    for entry in &entries {
        if entry.id.contains("claude-sonnet-4.5") || entry.id.contains("deepseek-chat-v3.1") {
            println!(
                "cargo:warning=DEPRECATED MODEL STILL IN BUILD DATA: {}",
                entry.id
            );
        }
    }

    write_constants(&out_dir, &provider, &entries)?;
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
}

struct EntryData {
    variant: String,
    const_name: String,
    id: String,
    vendor: String,
    display: String,
    description: String,
    efficient: bool,
    top_tier: bool,
    generation: String,
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
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    context: usize,
    #[serde(default, alias = "output_tokens")]
    max_output_tokens: Option<usize>,
    #[serde(default)]
    reasoning: bool,
    #[serde(default = "default_tool_call_true")]
    tool_call: bool,
    #[serde(default)]
    modalities: CapabilityModalities,
    #[serde(default)]
    capabilities: CapabilityFlags,
    #[serde(default)]
    cost: Option<PricingSpec>,
}

#[derive(Default, Deserialize)]
struct CapabilityModalities {
    #[serde(default)]
    input: Vec<String>,
}

#[derive(Default, Deserialize)]
struct CapabilityFlags {
    #[serde(default)]
    caching: bool,
    #[serde(default)]
    context_caching: bool,
    #[serde(default)]
    structured_output: bool,
}

#[derive(Clone, Copy, Default, Deserialize)]
struct PricingSpec {
    #[serde(default)]
    input: Option<f64>,
    #[serde(default)]
    output: Option<f64>,
    #[serde(default)]
    cache_read: Option<f64>,
    #[serde(default)]
    cache_write: Option<f64>,
}

struct CapabilityEntry {
    provider: String,
    id: String,
    display_name: String,
    description: String,
    context_window: usize,
    max_output_tokens: Option<usize>,
    reasoning: bool,
    tool_call: bool,
    vision: bool,
    input_modalities: Vec<String>,
    caching: bool,
    structured_output: bool,
    pricing: PricingSpec,
}

impl Provider {
    fn collect_entries(&self) -> Result<Vec<EntryData>> {
        let mut seen_constants = HashMap::new();
        let mut entries = Vec::with_capacity(self.models.len());

        for (model_id, spec) in &self.models {
            let Some(vtcode) = spec.vtcode.as_ref() else {
                println!(
                    "cargo:warning=Skipping openrouter model '{model_id}' without vtcode metadata"
                );
                continue;
            };

            let const_name = vtcode.constant.trim().to_string();
            if const_name.is_empty() {
                anyhow::bail!("vtcode constant name missing for model '{model_id}'");
            }
            if let Some(existing_id) = seen_constants.insert(const_name.clone(), model_id) {
                anyhow::bail!(
                    "Duplicate constant '{const_name}' for models '{existing_id}' and '{model_id}'"
                );
            }

            entries.push(EntryData {
                variant: vtcode.variant.clone(),
                const_name: const_name.clone(),
                id: spec.id.clone(),
                vendor: vtcode.vendor.to_lowercase(),
                display: vtcode.display.clone(),
                description: vtcode.description.clone(),
                efficient: vtcode.efficient,
                top_tier: vtcode.top_tier,
                generation: vtcode.generation.clone(),
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
            let vision = spec
                .modalities
                .input
                .iter()
                .any(|modality| matches!(modality.as_str(), "image" | "video"));
            entries.push(CapabilityEntry {
                provider: provider_key.to_string(),
                id: spec.id,
                display_name: spec.name,
                description: spec.description,
                context_window: spec.context,
                max_output_tokens: spec.max_output_tokens,
                reasoning: spec.reasoning,
                tool_call: spec.tool_call,
                input_modalities: spec.modalities.input,
                vision,
                caching: spec.capabilities.caching || spec.capabilities.context_caching,
                structured_output: spec.capabilities.structured_output,
                pricing: spec.cost.unwrap_or_default(),
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

fn write_constants(out_dir: &Path, provider: &Provider, entries: &[EntryData]) -> Result<()> {
    let content = build_codegen::generate_openrouter_constants(entries, provider)?;
    fs::write(out_dir.join("openrouter_constants.rs"), content)
        .context("Failed to write generated OpenRouter constants")
}

fn write_metadata(out_dir: &Path, entries: &[EntryData]) -> Result<()> {
    let content = build_codegen::generate_openrouter_metadata(entries);
    fs::write(out_dir.join("openrouter_metadata.rs"), content)
        .context("Failed to write generated OpenRouter metadata")
}

fn write_model_capabilities(out_dir: &Path, entries: &[CapabilityEntry]) -> Result<()> {
    let content = build_codegen::generate_model_capabilities(entries);
    fs::write(out_dir.join("model_capabilities.rs"), content)
        .context("Failed to write generated model capability metadata")
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
