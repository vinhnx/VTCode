#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SectionBoundaryMode {
    BracketOnly,
    BracketOrMarkdown,
}

/// Identifies which layer of the system prompt a `PromptSection` belongs to.
///
/// Variants mirror the layers `compose_system_instruction_text` actually
/// assembles today. Agent identity is not a separate variant: it is applied
/// as an in-place text substitution on the base contract (title/intro lines)
/// rather than an appended section, so it is folded into [`Self::BaseContract`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SectionKind {
    /// Canonical contract + operating profile (with any workspace prompt-layer
    /// override/append and agent-identity substitution already applied).
    /// Always present and never trimmed to satisfy the token budget.
    BaseContract,
    /// Optional `<analysis>/<plan>/<uncertainty>/<verification>` tagging
    /// guidance. Advisory; trimmed first when over budget.
    StructuredReasoning,
    /// Lean "## Skills" routing section rendered from available skill
    /// metadata. Advisory; trimmed alongside structured reasoning.
    Skills,
    /// "## Environment" addenda (languages, interaction mode, MCP sources,
    /// temporal context, working directory).
    EnvironmentAddenda,
    /// "## Active Tools" dynamic tool guidance derived from the active tool
    /// catalog.
    ToolGuidelines,
    /// "## Shell Profile" guidance for the current command environment.
    ShellProfile,
}

impl SectionKind {
    /// Static section name used in [`crate::prompts::SystemPromptReport::trimmed_sections`].
    pub const fn name(self) -> &'static str {
        match self {
            Self::BaseContract => "base_contract",
            Self::StructuredReasoning => "structured_reasoning",
            Self::Skills => "skills",
            Self::EnvironmentAddenda => "environment_addenda",
            Self::ToolGuidelines => "tool_guidelines",
            Self::ShellProfile => "shell_profile",
        }
    }

    /// Trim order: lower values are dropped first. `None` means the section
    /// is never dropped to satisfy the token budget.
    pub const fn trim_priority(self) -> Option<u8> {
        match self {
            Self::StructuredReasoning => Some(0),
            Self::Skills => Some(1),
            Self::EnvironmentAddenda => Some(2),
            Self::ShellProfile => Some(3),
            Self::ToolGuidelines => Some(4),
            Self::BaseContract => None,
        }
    }
}

/// A single layer of the composed system prompt, carrying its logical kind
/// and rendered text.
#[allow(dead_code)]
pub(crate) struct PromptSection {
    pub(crate) kind: SectionKind,
    pub(crate) text: String,
}

pub(crate) fn find_prompt_section_bounds(
    prompt: &str,
    section_header: &str,
    boundary_mode: SectionBoundaryMode,
) -> Option<(usize, usize)> {
    fn is_section_header_line(line: &str, boundary_mode: SectionBoundaryMode) -> bool {
        let trimmed = line.trim();
        (trimmed.starts_with('[') && trimmed.ends_with(']'))
            || matches!(boundary_mode, SectionBoundaryMode::BracketOrMarkdown) && trimmed.starts_with("## ")
    }

    let mut offset = 0usize;
    let mut section_start = None;

    for line in prompt.split_inclusive('\n') {
        let trimmed = line.trim();
        if section_start.is_none() && trimmed == section_header {
            section_start = Some(offset);
            offset += line.len();
            continue;
        }

        if let Some(start) = section_start
            && is_section_header_line(line, boundary_mode)
        {
            return Some((start, offset));
        }

        offset += line.len();
    }

    section_start.map(|start| (start, prompt.len()))
}

#[cfg(test)]
mod tests {
    use super::{SectionBoundaryMode, find_prompt_section_bounds};

    #[test]
    fn markdown_boundary_mode_stops_before_next_markdown_heading() {
        let prompt = "Base\n## Active Tools\n- a\n## Environment\n- b\n";
        let bounds = find_prompt_section_bounds(prompt, "## Active Tools", SectionBoundaryMode::BracketOrMarkdown)
            .expect("section bounds");

        assert_eq!(&prompt[bounds.0..bounds.1], "## Active Tools\n- a\n");
    }

    #[test]
    fn bracket_only_mode_ignores_markdown_headings() {
        let prompt = "Base\n[Harness Limits]\n- a\n## Environment\n- b\n";
        let bounds = find_prompt_section_bounds(prompt, "[Harness Limits]", SectionBoundaryMode::BracketOnly)
            .expect("section bounds");

        assert_eq!(&prompt[bounds.0..bounds.1], "[Harness Limits]\n- a\n## Environment\n- b\n");
    }
}
