use std::env;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use vtcode_config::models::openrouter_generated::{ENTRIES, VENDOR_MODELS};

const EMBEDDED_ASSETS: &[(&str, &str)] = &[
    ("prompts/custom/vtcode.md", "prompts/custom/vtcode.md"),
    (
        "prompts/custom/generate-agent-file.md",
        "prompts/custom/generate-agent-file.md",
    ),
    ("docs/vtcode_docs_map.md", "docs/vtcode_docs_map.md"),
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../vtcode-config/build_data/openrouter_models.json");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let workspace_dir = manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| manifest_dir.clone());
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let assets_out_dir = out_dir.join("embedded_assets");
    fs::create_dir_all(&assets_out_dir)?;

    let mut resolved_assets = Vec::new();
    for (relative, dest_relative) in EMBEDDED_ASSETS {
        let source = locate_asset(&manifest_dir, &workspace_dir, relative)?;
        println!("cargo:rerun-if-changed={}", source.display());

        let fallback = fallback_path(&manifest_dir, relative);
        if fallback.exists() && fallback != source {
            println!("cargo:rerun-if-changed={}", fallback.display());
        }

        resolved_assets.push((source, dest_relative));
    }

    for (source, dest_relative) in resolved_assets {
        let destination = assets_out_dir.join(dest_relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&source, &destination)?;
    }

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
    for entry in ENTRIES {
        let variant = format!("{:?}", entry.variant);
        writeln!(metadata, "    Entry {{")?;
        writeln!(metadata, "        variant: super::ModelId::{variant},")?;
        writeln!(metadata, "        id: {:?},", entry.id)?;
        writeln!(metadata, "        vendor: {:?},", entry.vendor)?;
        writeln!(metadata, "        display: {:?},", entry.display)?;
        writeln!(metadata, "        description: {:?},", entry.description)?;
        writeln!(metadata, "        efficient: {},", entry.efficient)?;
        writeln!(metadata, "        top_tier: {},", entry.top_tier)?;
        writeln!(metadata, "        generation: {:?},", entry.generation)?;
        writeln!(metadata, "        reasoning: {},", entry.reasoning)?;
        writeln!(metadata, "        tool_call: {},", entry.tool_call)?;
        metadata.push_str("    },\n");
    }
    metadata.push_str("];\n\n");

    metadata.push_str("#[derive(Clone, Copy)]\n");
    metadata.push_str("pub struct VendorModels {\n");
    metadata.push_str("    pub vendor: &'static str,\n");
    metadata.push_str("    pub models: &'static [super::ModelId],\n");
    metadata.push_str("}\n\n");

    metadata.push_str("pub const VENDOR_MODELS: &[VendorModels] = &[\n");
    for group in VENDOR_MODELS {
        writeln!(metadata, "    VendorModels {{")?;
        writeln!(metadata, "        vendor: {:?},", group.vendor)?;
        metadata.push_str("        models: &[\n");
        for model in group.models {
            let variant = format!("{:?}", model);
            writeln!(metadata, "            super::ModelId::{variant},")?;
        }
        metadata.push_str("        ],\n    },\n");
    }
    metadata.push_str("];\n\n");

    metadata.push_str(
        "pub fn metadata_for(model: super::ModelId) -> Option<super::OpenRouterMetadata> {\n",
    );
    metadata.push_str(
        "    ENTRIES.iter().find(|entry| entry.variant == model).map(|entry| super::OpenRouterMetadata {\n",
    );
    metadata.push_str("        id: entry.id,\n");
    metadata.push_str("        vendor: entry.vendor,\n");
    metadata.push_str("        display: entry.display,\n");
    metadata.push_str("        description: entry.description,\n");
    metadata.push_str("        efficient: entry.efficient,\n");
    metadata.push_str("        top_tier: entry.top_tier,\n");
    metadata.push_str("        generation: entry.generation,\n");
    metadata.push_str("        reasoning: entry.reasoning,\n");
    metadata.push_str("        tool_call: entry.tool_call,\n");
    metadata.push_str("    })\n}");

    metadata.push_str("\npub fn parse_model(value: &str) -> Option<super::ModelId> {\n");
    metadata.push_str(
        "    ENTRIES.iter().find(|entry| entry.id == value).map(|entry| entry.variant)\n}",
    );

    metadata
        .push_str("\npub fn vendor_groups() -> &'static [VendorModels] {\n    VENDOR_MODELS\n}");

    fs::write(out_dir.join("openrouter_metadata.rs"), metadata)?;

    Ok(())
}

fn locate_asset(manifest_dir: &Path, workspace_dir: &Path, relative: &str) -> io::Result<PathBuf> {
    let workspace_candidate = workspace_dir.join(relative);
    if workspace_candidate.exists() {
        ensure_fallback_in_sync(manifest_dir, relative, &workspace_candidate)?;
        return Ok(workspace_candidate);
    }

    let fallback = fallback_path(manifest_dir, relative);
    if fallback.exists() {
        return Ok(fallback);
    }

    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!(
            "failed to locate embedded asset `{}` (looked in `{}` and `{}`)",
            relative,
            workspace_candidate.display(),
            fallback.display()
        ),
    ))
}

fn ensure_fallback_in_sync(
    manifest_dir: &Path,
    relative: &str,
    canonical: &Path,
) -> io::Result<()> {
    let fallback = fallback_path(manifest_dir, relative);
    if fallback.exists() {
        let canonical_bytes = fs::read(canonical)?;
        let fallback_bytes = fs::read(&fallback)?;
        if canonical_bytes != fallback_bytes {
            return Err(io::Error::other(format!(
                "embedded asset `{}` is out of sync. Update `{}` to match `{}`",
                relative,
                fallback.display(),
                canonical.display()
            )));
        }
    }
    Ok(())
}

fn fallback_path(manifest_dir: &Path, relative: &str) -> PathBuf {
    manifest_dir.join("embedded_assets_source").join(relative)
}
