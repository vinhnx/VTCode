use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

use vtcode_config::models::openrouter_generated::{ENTRIES, VENDOR_MODELS};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../vtcode-config/build_data/openrouter_models.json");

    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
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
