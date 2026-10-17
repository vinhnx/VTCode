//! Compaction configuration, route policy, and parent context.

use super::*;

const DEFAULT_COMPACTION_TARGET_THRESHOLD: f64 = 0.50;
const DEFAULT_COMPACTION_KEEP_LAST_MESSAGES: usize = 10;
const DEFAULT_RETAINED_USER_MESSAGE_TOKENS: usize = 20_000;
const DEFAULT_RETAINED_USER_MESSAGES: usize = 6;
/// Internal continuity budget. This is deliberately not configuration: changing
/// it changes the shape of every compacted request and therefore provider cache
/// behavior.
pub(crate) const CONTINUITY_TAIL_TARGET_TOKENS: usize = 20_000;
/// Keep history below the model window after reserving space for the system
/// prompt, memory envelope, summary framing, and the next response.
pub(crate) const COMPACTION_CONTEXT_OVERHEAD_FRACTION_DENOMINATOR: usize = 8;
pub(crate) const COMPACTION_CONTEXT_FIXED_OVERHEAD_TOKENS: usize = 512;
pub(crate) const SUMMARY_PREFIX: &str = "Previous conversation summary:\n";
pub(crate) const ABSTRACT_PREFIX: &str = "Earlier context (abstract):\n";
pub(crate) const DETAIL_PREFIX: &str = "Recent context (summary):\n";

/// Default summarization prompt. Structures the summary for continuity: after
/// reading it, the next context must feel like a seamless continuation, not a
/// fresh start. Kept as a `const` so `CompactionConfig::default` does not
/// re-allocate a ~1KB literal on every construction.
const DEFAULT_SUMMARY_PROMPT: &str = "Summarize the conversation so far using this exact structure. The goal is continuity: after reading this summary, the next context must feel like a seamless continuation of the same task, not a fresh start.\n\n## Goal\n[What the user is trying to accomplish]\n\n## Constraints & Preferences\n- [Requirements, preferences, or constraints from the user]\n\n## What I Was Just Doing\n[The single most recent action in progress: what step the agent was executing, which tool or edit was underway, and where it left off. This is the continuity anchor.]\n\n## Last Action & Result\n[The last completed action and its outcome (success, error, or partial). Include the exact error or status if relevant.]\n\n## Progress\n### Done\n- [Completed work]\n\n### In Progress\n- [Current work]\n\n### Blocked\n- [Blocking issues, if any]\n\n## Key Decisions\n- **[Decision]**: [Reason]\n\n## Next Steps\n1. [Most important next step]\n\n## Critical Context\n- [Facts needed to continue]\n\nKeep it concise and actionable. Always preserve the current task objective and acceptance criteria, file paths that were read or modified, test results and error messages, and decisions with their reasoning.";

/// Compaction configuration for context window management.
#[derive(Debug, Clone)]
pub struct CompactionConfig {
    /// Threshold (0.0-1.0) at which to trigger compaction.
    pub trigger_threshold: f64,
    /// Target usage ratio (0.0-1.0) after compaction.
    pub target_threshold: f64,
    /// Prompt for summarization.
    pub summary_prompt: String,
    /// Legacy short-circuit used to skip local compaction for tiny histories.
    pub keep_last_messages: usize,
    /// Total token budget reserved for retaining real user messages verbatim.
    pub retained_user_message_tokens: usize,
    /// Maximum number of recent user messages to retain verbatim.
    pub retained_user_messages: usize,
    /// Force local summarization even for short histories and providers with native compaction.
    pub always_summarize: bool,
    /// Enable hierarchical summarization (multi-level pyramid).
    ///
    /// When `true`, compaction produces three tiers instead of a flat summary:
    /// - **Abstract**: oldest turns compressed into 1-2 sentences
    /// - **Detail**: middle turns summarized into a paragraph
    /// - **Verbatim**: most recent turns kept as-is
    ///
    /// When `false` (default), all old turns become a single flat summary.
    pub hierarchical: bool,
    /// Auto-compaction suppression state. `SUPPRESS_NONE` (0) means compaction
    /// is allowed; any other value gates automatic compaction until the
    /// appropriate clearing event (success, model switch, etc.).
    pub auto_compact_suppressed: u8,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            trigger_threshold: DEFAULT_COMPACTION_TRIGGER_RATIO,
            target_threshold: DEFAULT_COMPACTION_TARGET_THRESHOLD,
            summary_prompt: DEFAULT_SUMMARY_PROMPT.to_string(),
            keep_last_messages: DEFAULT_COMPACTION_KEEP_LAST_MESSAGES,
            retained_user_message_tokens: DEFAULT_RETAINED_USER_MESSAGE_TOKENS,
            retained_user_messages: DEFAULT_RETAINED_USER_MESSAGES,
            always_summarize: false,
            hierarchical: false,
            auto_compact_suppressed: SUPPRESS_NONE,
        }
    }
}

/// Per-route compaction policy in the DeepSeek-harness shape.
///
/// `threshold_ratio` caps the auto-compaction trigger as a fraction of the
/// route window (fail-safe: it can only fire *earlier*). `retain_ratio` sizes
/// the verbatim continuity tail as a fraction of the route window so small
/// windows keep a usable summary instead of a tail-only history.
/// `max_overflow_retries` bounds extra bounded-fork attempts after a
/// context-capacity rejection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompactionRoutePolicy {
    /// Trigger ceiling as a fraction of the route window. `1.0` preserves the
    /// absolute-tokens trigger unchanged.
    pub threshold_ratio: f64,
    /// Verbatim-tail share of the route window.
    pub retain_ratio: f64,
    /// Extra attempts after a context-capacity rejection (progressive halving).
    pub max_overflow_retries: u32,
}

/// Floor keeping the continuity anchor meaningful on tiny windows.
const MIN_TAIL_TARGET_TOKENS: usize = 1_024;

impl Default for CompactionRoutePolicy {
    fn default() -> Self {
        Self {
            threshold_ratio: 1.0,
            retain_ratio: 0.16,
            max_overflow_retries: 1,
        }
    }
}

/// Per-route overrides, matched as `(provider_substring, model_substring)`.
/// Empty until a route demonstrates a need: the window-derived defaults
/// already adapt retention and budgets to every route's resolved capacity.
const ROUTE_POLICIES: &[(&str, &str, CompactionRoutePolicy)] = &[];

impl CompactionRoutePolicy {
    /// Resolve the policy for a provider/model route (first table hit wins,
    /// otherwise the default).
    #[must_use]
    pub fn resolve(provider_name: &str, model: &str) -> Self {
        ROUTE_POLICIES
            .iter()
            .find(|(provider, model_match, _)| provider_name.contains(provider) && model.contains(model_match))
            .map(|(_, _, policy)| *policy)
            .unwrap_or_default()
    }

    /// Verbatim-tail budget for a route window. Unknown windows (`0`) keep the
    /// legacy constant; otherwise the tail scales with the window so small
    /// routes keep a usable summary instead of a tail-only history.
    #[allow(
        clippy::cast_sign_loss,
        reason = "The ratio is clamped to [0.0, 1.0] and the window is non-negative, so the product cannot be negative."
    )]
    #[must_use]
    pub fn tail_target_tokens(&self, window_tokens: usize) -> usize {
        if window_tokens == 0 {
            return CONTINUITY_TAIL_TARGET_TOKENS;
        }
        ((window_tokens as f64 * self.retain_ratio.clamp(0.0, 1.0)) as usize)
            .clamp(MIN_TAIL_TARGET_TOKENS, CONTINUITY_TAIL_TARGET_TOKENS)
    }

    /// Fail-safe trigger cap: only fires compaction *earlier*, never later.
    /// A `1.0` ratio (the default) leaves the absolute-tokens trigger
    /// unchanged.
    #[allow(
        clippy::cast_sign_loss,
        reason = "The ratio is clamped to [0.0, 1.0] and the window is non-negative, so the product cannot be negative."
    )]
    #[must_use]
    pub fn apply_threshold_cap(&self, threshold_tokens: usize, window_tokens: usize) -> usize {
        if self.threshold_ratio < 1.0 && window_tokens > 0 {
            threshold_tokens.min((window_tokens as f64 * self.threshold_ratio.clamp(0.0, 1.0)) as usize)
        } else {
            threshold_tokens
        }
    }
}

/// Resolve the route tail budget from the session budget (already intersected
/// with the provider window) or the provider window when unset.
#[must_use]
pub(crate) fn route_tail_target_tokens(
    provider: &dyn LLMProvider,
    model: &str,
    context_budget: Option<usize>,
) -> usize {
    let policy = CompactionRoutePolicy::resolve(provider.name(), model);
    let window = context_budget
        .filter(|value| *value > 0)
        .unwrap_or_else(|| provider.effective_context_size(model));
    policy.tail_target_tokens(window)
}

/// Parent request prefix for cache-safe compaction forking.
///
/// Prompt caching is a prefix match: a compaction request only reuses the
/// parent conversation's cached prefix when it carries the *exact same*
/// system prompt, tool definitions, and history prefix, with the compaction
/// instruction appended as the final new turn. A standalone single-message
/// summary prompt pays the full uncached input rate for the entire history.
#[derive(Debug, Clone, Default)]
pub struct CompactionParentContext {
    /// Exact system prompt of the parent segment (`request_envelope.system_prompt()`).
    pub system_prompt: Option<Arc<str>>,
    /// Exact ordered tool catalog of the parent segment
    /// (`request_envelope.ordered_tools()`). Empty catalogs should be `None`.
    pub tools: Option<Arc<Vec<ToolDefinition>>>,
}

impl CompactionParentContext {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.system_prompt.is_none() && self.tools.is_none()
    }
}

/// Build a cache-safe compaction history: the parent's full conversation
/// prefix verbatim, plus the compaction instruction as the only new turn.
/// From the provider's perspective this looks nearly identical to the
/// parent's last request, so the cached prefix is reused.
#[must_use]
pub fn build_cache_safe_compaction_history(history: &[Message], compaction_prompt: &str) -> Vec<Message> {
    let mut forked = Vec::with_capacity(history.len().saturating_add(1));
    forked.extend(history.iter().cloned());
    forked.push(Message::user(compaction_prompt.to_string()));
    forked
}
