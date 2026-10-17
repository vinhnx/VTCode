//! Session onboarding and welcome-message configuration.

use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentOnboardingConfig {
    /// Toggle onboarding message rendering
    #[serde(default = "default_onboarding_enabled")]
    pub enabled: bool,

    /// Introductory text shown at session start
    #[serde(default = "default_intro_text")]
    intro_text: String,

    /// Whether to include project overview in onboarding message
    #[serde(default = "default_show_project_overview")]
    include_project_overview: bool,

    /// Whether to include language summary in onboarding message
    #[serde(default = "default_show_language_summary")]
    pub include_language_summary: bool,

    /// Whether to include AGENTS.md/CLAUDE.md highlights in onboarding message
    #[serde(default = "default_show_guideline_highlights")]
    pub include_guideline_highlights: bool,

    /// Whether to surface usage tips inside the welcome text banner
    #[serde(default = "default_show_usage_tips_in_welcome")]
    pub include_usage_tips_in_welcome: bool,

    /// Whether to surface suggested actions inside the welcome text banner
    #[serde(default = "default_show_recommended_actions_in_welcome")]
    pub include_recommended_actions_in_welcome: bool,

    /// Maximum number of guideline bullets to surface
    #[serde(default = "default_guideline_highlight_limit")]
    pub guideline_highlight_limit: usize,

    /// Tips for collaborating with the agent effectively
    #[serde(default = "default_usage_tips")]
    pub usage_tips: Vec<String>,

    /// Recommended follow-up actions to display
    #[serde(default = "default_recommended_actions")]
    pub recommended_actions: Vec<String>,

    /// Placeholder suggestion for the chat input bar
    #[serde(default)]
    pub chat_placeholder: Option<String>,
}

impl Default for AgentOnboardingConfig {
    fn default() -> Self {
        Self {
            enabled: default_onboarding_enabled(),
            intro_text: default_intro_text(),
            include_project_overview: default_show_project_overview(),
            include_language_summary: default_show_language_summary(),
            include_guideline_highlights: default_show_guideline_highlights(),
            include_usage_tips_in_welcome: default_show_usage_tips_in_welcome(),
            include_recommended_actions_in_welcome: default_show_recommended_actions_in_welcome(),
            guideline_highlight_limit: default_guideline_highlight_limit(),
            usage_tips: default_usage_tips(),
            recommended_actions: default_recommended_actions(),
            chat_placeholder: None,
        }
    }
}

#[inline]
const fn default_onboarding_enabled() -> bool {
    true
}

const DEFAULT_INTRO_TEXT: &str = "Let's get oriented. I preloaded workspace context so we can move fast.";

#[inline]
fn default_intro_text() -> String {
    DEFAULT_INTRO_TEXT.into()
}

#[inline]
const fn default_show_project_overview() -> bool {
    true
}

#[inline]
const fn default_show_language_summary() -> bool {
    false
}

#[inline]
const fn default_show_guideline_highlights() -> bool {
    true
}

#[inline]
const fn default_show_usage_tips_in_welcome() -> bool {
    false
}

#[inline]
const fn default_show_recommended_actions_in_welcome() -> bool {
    false
}

#[inline]
const fn default_guideline_highlight_limit() -> usize {
    3
}

const DEFAULT_USAGE_TIPS: &[&str] = &[
    "Describe your current coding goal or ask for a quick status overview.",
    "Reference AGENTS.md/CLAUDE.md guidelines when proposing changes.",
    "Prefer asking for targeted file reads or diffs before editing.",
];

const DEFAULT_RECOMMENDED_ACTIONS: &[&str] = &[
    "Review the highlighted guidelines and share the task you want to tackle.",
    "Ask for a workspace tour if you need more context.",
];

fn default_usage_tips() -> Vec<String> {
    DEFAULT_USAGE_TIPS.iter().map(|s| (*s).into()).collect()
}

fn default_recommended_actions() -> Vec<String> {
    DEFAULT_RECOMMENDED_ACTIONS.iter().map(|s| (*s).into()).collect()
}
