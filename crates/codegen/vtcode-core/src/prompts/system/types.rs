//! System-prompt public types: config, section model, and budget report.

use crate::prompts::system::tokens::estimate_token_count;

/// System instruction configuration
#[derive(Debug, Clone, Default)]
pub struct SystemPromptConfig;

/// A named layer of the composed system prompt.
///
/// The token-budget trimmer (see [`SectionKind::trim_priority`]) drops whole
/// sections rather than truncating text mid-layer, so each section's text is
/// stored verbatim (including any leading/trailing whitespace baked into its
/// source constant) exactly as it would have been appended by the legacy
/// single-string builder.
pub(super) struct PromptSection {
    pub(super) kind: SectionKind,
    pub(super) text: String,
}

/// Identifies which layer of the system prompt a `PromptSection` belongs to.
///
/// Variants mirror the layers `compose_system_instruction_text` actually
/// assembles today. Agent identity is not a separate variant: it is applied
/// as an in-place text substitution on the base contract (title/intro lines)
/// rather than an appended section, so it is folded into [`Self::BaseContract`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SectionKind {
    /// Compiled runtime guidance, canonical contract + operating profile (with
    /// any workspace prompt-layer override/append and agent-identity
    /// substitution already applied).
    /// Always present and never trimmed to satisfy the token budget.
    BaseContract,
    /// Optional `<analysis>/<reasoning_plan>/<uncertainty>/<verification>` tagging
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
    /// Static section name used in [`SystemPromptReport::trimmed_sections`].
    pub(super) const fn name(self) -> &'static str {
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
    pub(super) const fn trim_priority(self) -> Option<u8> {
        match self {
            Self::StructuredReasoning => Some(0),
            Self::Skills => Some(1),
            Self::EnvironmentAddenda => Some(2),
            // Shell safety guidance and the active-tool contract are required
            // for the model to use the available tools safely. They must never
            // disappear as a side effect of prompt budgeting.
            Self::ShellProfile | Self::ToolGuidelines => None,
            Self::BaseContract => None,
        }
    }

    /// Whether this section belongs to the stable instruction prefix.
    ///
    /// Runtime tool catalogs and environment observations are deliberately
    /// dynamic. Keeping this classification beside the section definition
    /// avoids relying on an earliest-header heuristic, which can accidentally
    /// make a later safety section part of (or outside) a cache boundary.
    pub(super) const fn is_cache_stable(self) -> bool {
        matches!(self, Self::BaseContract | Self::StructuredReasoning | Self::Skills | Self::ShellProfile)
    }
}

/// Result of measuring a composed system prompt against the configured token
/// budget (`agent.max_system_prompt_tokens`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SystemPromptReport {
    /// `estimate_token_count` of the final composed text (after trimming, if
    /// trimming occurred).
    pub token_estimate: u64,
    /// Whether `token_estimate` exceeds `agent.max_system_prompt_tokens`.
    pub over_budget: bool,
    /// Names of sections dropped to satisfy the budget, in drop order. Empty
    /// unless `agent.trim_system_prompt` is enabled and trimming occurred.
    pub trimmed_sections: Vec<&'static str>,
}

impl SystemPromptReport {
    /// Measure `text` against `max_tokens` with no trimming applied.
    ///
    /// Useful when a system prompt was assembled or overridden outside the
    /// normal section-based pipeline (e.g. downstream embedders calling
    /// `AgentRunner::set_system_prompt`, or appendix text appended after
    /// [`crate::prompts::system::compose_system_instruction_with_report`] already measured the
    /// sectioned prompt).
    #[must_use]
    pub fn measure(text: &str, max_tokens: u64) -> Self {
        let token_estimate = estimate_token_count(text);
        Self {
            token_estimate,
            over_budget: token_estimate > max_tokens,
            trimmed_sections: Vec::new(),
        }
    }
}
