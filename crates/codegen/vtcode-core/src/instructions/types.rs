use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Defines the scope from which an instruction source originates.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum InstructionScope {
    /// User-level instructions from the canonical config root or a compatibility root.
    User,
    /// Workspace-level instructions from the project root or subdirectories.
    Workspace,
    /// Custom instructions from explicit glob patterns.
    Custom,
}

/// The kind of instruction file discovered on disk.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstructionSourceKind {
    /// An `AGENTS.md` or `CLAUDE.md` file.
    Agents,
    /// A rule file from `.vtcode/rules/` with optional path matching.
    Rule,
    /// An extra instruction file matched by a custom glob pattern.
    Extra,
}

/// A discovered instruction file with its path, scope, and kind metadata.
#[derive(Debug, Clone, Serialize)]
pub struct InstructionSource {
    /// Canonical path to the instruction file on disk.
    pub path: PathBuf,
    /// Scope from which this source was discovered.
    pub scope: InstructionScope,
    /// Kind of instruction file (agents, rule, or extra).
    pub kind: InstructionSourceKind,
    /// Whether a conditional rule matched the current file context.
    pub matched: bool,
}

/// A single instruction segment pairing a source with its expanded text content.
#[derive(Debug, Clone, Serialize)]
pub struct InstructionSegment {
    /// Metadata about where this segment was loaded from.
    pub source: InstructionSource,
    /// The instruction text after frontmatter stripping and import expansion.
    pub contents: String,
}

/// A complete bundle of instruction segments ready for prompt assembly.
#[derive(Debug, Clone, Serialize)]
pub struct InstructionBundle {
    /// Ordered list of instruction segments (lowest to highest precedence).
    pub segments: Vec<InstructionSegment>,
    /// Whether the bundle was truncated due to the byte budget.
    pub truncated: bool,
    /// Total bytes read from disk across all segments.
    pub bytes_read: usize,
}

impl InstructionBundle {
    /// Returns `true` if the bundle contains no instruction segments.
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// Concatenates all segment contents into a single string separated by blank lines.
    pub fn combined_text(&self) -> String {
        let capacity = self
            .segments
            .iter()
            .map(|segment| segment.contents.len())
            .sum::<usize>()
            .saturating_add(self.segments.len().saturating_sub(1) * 2);
        let mut output = String::with_capacity(capacity);
        for (index, segment) in self.segments.iter().enumerate() {
            if index > 0 {
                output.push_str("\n\n");
            }

            output.push_str(&segment.contents);
        }
        output
    }

    /// Extracts up to `limit` key-point highlights from the bundle's segments.
    pub fn highlights(&self, limit: usize) -> Vec<String> {
        super::render::extract_instruction_highlights(&self.segments, limit)
    }
}

#[derive(Debug, Clone)]
pub struct InstructionDiscoveryOptions<'a> {
    pub current_dir: &'a Path,
    pub project_root: &'a Path,
    pub home_dir: Option<&'a Path>,
    pub extra_patterns: &'a [String],
    pub fallback_filenames: &'a [String],
    pub exclude_patterns: &'a [String],
    pub match_paths: &'a [PathBuf],
    pub import_max_depth: usize,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(super) struct RuleFrontmatter {
    #[serde(default)]
    pub(super) paths: Vec<String>,
}

#[derive(Debug, Clone)]
pub(super) struct MatchCandidate {
    pub(super) relative_path: String,
    pub(super) is_dir: bool,
}
