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
    /// `estimate_token_count` of the user/workspace prompt-layer text spliced
    /// into the prompt (see [`crate::prompts::resources::SystemPromptLayers::applied_text`]),
    /// measured before trimming. Layers are never trimmed, so this figure stays
    /// valid after trimming; the built-in share is `token_estimate - layer_token_estimate`
    /// modulo estimator rounding. Zero when the prompt was assembled outside
    /// the layer pipeline.
    pub layer_token_estimate: u64,
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
            layer_token_estimate: 0,
        }
    }

    /// One-line over-budget warning shared by the headless (`task_setup.rs`)
    /// and interactive (`header_context.rs`) paths so both sessions surface
    /// the same signal. Returns `None` when within budget; callers gate on
    /// `agent.system_prompt_budget_warning` themselves.
    #[must_use]
    pub fn over_budget_warning(&self, max_tokens: u64) -> Option<String> {
        if !self.over_budget {
            return None;
        }
        let layer_note = match self.layer_token_estimate {
            0 => String::new(),
            layer_tokens => format!(" User/workspace prompt layers contribute ~{layer_tokens} of those tokens."),
        };
        Some(format!(
            "Base system prompt is ~{} tokens (budget {max_tokens}); later appendices (session context, runtime line, subagents roster) add more. Consider a leaner system prompt mode or enable agent.trim_system_prompt.{layer_note}",
            self.token_estimate,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn over_budget_warning_names_layer_share() {
        let within = SystemPromptReport {
            token_estimate: 100,
            over_budget: false,
            trimmed_sections: Vec::new(),
            layer_token_estimate: 0,
        };
        assert!(within.over_budget_warning(1000).is_none());

        let bare = SystemPromptReport {
            token_estimate: 1500,
            over_budget: true,
            trimmed_sections: Vec::new(),
            layer_token_estimate: 0,
        };
        assert_eq!(
            bare.over_budget_warning(1000).expect("warning"),
            "Base system prompt is ~1500 tokens (budget 1000); later appendices (session context, runtime line, subagents roster) add more. Consider a leaner system prompt mode or enable agent.trim_system_prompt."
        );

        let layered = SystemPromptReport {
            token_estimate: 1500,
            over_budget: true,
            trimmed_sections: Vec::new(),
            layer_token_estimate: 400,
        };
        let warning = layered.over_budget_warning(1000).expect("warning");
        assert!(warning.ends_with("trim_system_prompt. User/workspace prompt layers contribute ~400 of those tokens."));
    }
}
