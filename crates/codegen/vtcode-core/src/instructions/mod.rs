mod discovery;
mod expansion;
mod labels;
mod matching;
mod render;
mod types;

pub use discovery::discover_instruction_sources;
pub use expansion::read_instruction_bundle;
pub use labels::{format_instruction_path, instruction_scope_label, instruction_source_label};
pub use render::{extract_instruction_highlights, render_instruction_markdown};
pub use types::{
    InstructionBundle, InstructionDiscoveryOptions, InstructionScope, InstructionSegment, InstructionSource,
    InstructionSourceKind,
};

const AGENTS_FILENAME: &str = "AGENTS.md";
const AGENTS_OVERRIDE_FILENAME: &str = "AGENTS.override.md";
const CLAUDE_FILENAME: &str = "CLAUDE.md";
const GLOBAL_CONFIG_DIRECTORY: &str = ".config/vtcode";
const RULES_DIRECTORY: &str = ".vtcode/rules";
const IMPORT_PROBE_NAME: &str = "__vtcode_instruction_probe__";

#[cfg(test)]
mod tests;
