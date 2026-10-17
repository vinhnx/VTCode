//! Compaction strategy selection and manual-compaction options.

use super::native_inline::compact_history_native_inline;
use super::*;

/// How the manual `/compact` command compacts for a given provider/model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionStrategy {
    /// Provider exposes a standalone on-demand compaction endpoint
    /// (OpenAI `/responses/compact`). Delegates to `LLMProvider::compact_history_with_options`.
    NativeStandalone,
    /// Provider compacts inline via request fields, threshold-triggered
    /// (Anthropic `compact_20260112`). Invoked through the capability-aware
    /// one-shot response collector with `context_management` set and
    /// `pause_after_compaction`.
    NativeInline,
    /// Universal fallback: summarize history via the capability-aware one-shot
    /// response collector and rebuild as a summary message plus retained recent
    /// user messages. Works for every provider.
    Local,
}

/// Select the manual-compaction strategy for a provider/model.
///
/// `NativeStandalone` when the provider opts in via `supports_manual_openai_compaction`
/// (e.g. OpenAI `/responses/compact`), `NativeInline` when the provider reports
/// inline compaction support via `supports_native_inline_compaction` (e.g.
/// Anthropic `compact_20260112`), otherwise `Local`.
///
/// Note: `supports_responses_compaction` is intentionally *not* the discriminator
/// for `NativeInline`. It is overloaded — true for both OpenAI-compatible
/// standalone compaction and Anthropic inline compaction — so OpenAI-compatible
/// custom endpoints (which report it but cannot serve an Anthropic
/// `compact_20260112` edit) would otherwise be misrouted to `NativeInline` and
/// waste a rejected `generate` call before falling back to `Local`.
pub fn manual_compaction_strategy(provider: &dyn LLMProvider, model: &str) -> CompactionStrategy {
    if provider.supports_manual_openai_compaction(model) {
        CompactionStrategy::NativeStandalone
    } else if provider.supports_native_inline_compaction(model) {
        CompactionStrategy::NativeInline
    } else {
        CompactionStrategy::Local
    }
}

/// Whether a compaction result is worth keeping.
///
/// Local compaction always appends framing around the summarized history (a
/// summary message plus the persisted session-memory envelope), so a result
/// whose message count is not strictly smaller than the input grows the
/// conversation instead of compacting it. This happens when the continuity
/// tail already covers the whole history (small, tool-heavy sessions where a
/// handful of user turns anchor dozens of assistant/tool messages): the
/// rebuild keeps every message and adds framing on top (observed as
/// `86 -> 88`). Such results must be discarded in favor of the already-compact
/// path so callers never report growth as a successful compaction.
///
/// The check is intentionally message-count based: that is the unit surfaced
/// in user-facing progress (`86 -> 12`) and harness `compact_boundary`
/// events, so the persisted history must never exceed it.
#[must_use]
pub fn compacted_history_shrinks(original_len: usize, compacted_len: usize, mode: CompactionMode) -> bool {
    // Local mode injects one envelope message into the returned history during
    // persistence; account for it here so the *final* history is what shrinks.
    // Provider-native windows (and the `Unknown` catch-all, which never arises
    // from a live compaction pass) are canonical replay state with no envelope
    // injected, so the raw lengths compare directly.
    let envelope_messages = match mode {
        CompactionMode::Local => 1,
        CompactionMode::Provider | CompactionMode::Unknown => 0,
    };
    compacted_len.saturating_add(envelope_messages) < original_len
}

/// Universally meaningful manual-compaction options.
///
/// Provider-specific extras (OpenAI `service_tier` / `prompt_cache_key` / `store` /
/// `include`) are intentionally absent: the manual `/compact` command exposes only
/// the options that apply across every provider.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManualCompactionOptions {
    /// Overrides the default summary/compaction prompt when set.
    pub instructions: Option<String>,
    /// Caps the summary/compaction output length on every provider.
    pub max_output_tokens: Option<u32>,
    /// Optional reasoning effort override for the compaction pass.
    pub reasoning_effort: Option<ReasoningEffortLevel>,
    /// Permit an explicit lower supported effort when the requested level is
    /// unavailable on the selected provider/model route. The default is
    /// strict blocking so compaction never silently changes reasoning
    /// fidelity.
    pub allow_reasoning_effort_downgrade: bool,
    /// Optional verbosity override for the compaction output.
    pub verbosity: Option<VerbosityLevel>,
}

impl From<ManualCompactionOptions> for ResponsesCompactionOptions {
    fn from(options: ManualCompactionOptions) -> Self {
        Self {
            instructions: options.instructions,
            max_output_tokens: options.max_output_tokens,
            reasoning_effort: options.reasoning_effort,
            verbosity: options.verbosity,
            responses_include: None,
            response_store: None,
            service_tier: None,
            prompt_cache_key: None,
        }
    }
}

impl CompactionConfig {
    /// Return a config with the manual options' instructions applied as the
    /// summary prompt override. The remaining option fields
    /// (`max_output_tokens`, `reasoning_effort`, `verbosity`) are applied to the
    /// summary `LLMRequest` directly by `summarize_locally`, not stored here.
    pub(crate) fn with_manual_overrides(self, options: &ManualCompactionOptions) -> Self {
        let summary_prompt = options
            .instructions
            .clone()
            .map(|instructions| instructions.trim().to_string())
            .filter(|instructions| !instructions.is_empty())
            .unwrap_or(self.summary_prompt);
        Self { summary_prompt, ..self }
    }
}

/// Compact history for the manual `/compact` command using provider-native
/// compaction when available, falling back to local summarization otherwise.
///
/// Returns the compacted messages and the `CompactionMode` that produced them
/// (`Provider` for native compaction, `Local` for client-side summarization).
#[cfg_attr(feature = "profiling", hotpath::measure)]
pub async fn compact_history_manual(
    provider: &dyn LLMProvider,
    model: &str,
    history: &[Message],
    config: &CompactionConfig,
    options: &ManualCompactionOptions,
) -> Result<(Vec<Message>, CompactionMode)> {
    compact_history_manual_with_budget(provider, model, history, config, options, None).await
}

/// Manual compaction with the resolved session context budget applied to native
/// request inputs and locally rebuilt summaries. Native provider responses are
/// returned unchanged so opaque continuation state remains valid.
pub async fn compact_history_manual_with_budget(
    provider: &dyn LLMProvider,
    model: &str,
    history: &[Message],
    config: &CompactionConfig,
    options: &ManualCompactionOptions,
    context_budget: Option<usize>,
) -> Result<(Vec<Message>, CompactionMode)> {
    compact_history_manual_with_parent_context(provider, model, history, config, options, context_budget, None).await
}

/// Manual compaction that reuses the parent segment's cached prefix.
///
/// Pass the live request envelope's system prompt + ordered tools as `parent`
/// so the local-summary fork (`summarize_locally`) hits the provider prompt
/// cache instead of re-paying full input cost for the entire history.
pub async fn compact_history_manual_with_parent_context(
    provider: &dyn LLMProvider,
    model: &str,
    history: &[Message],
    config: &CompactionConfig,
    options: &ManualCompactionOptions,
    context_budget: Option<usize>,
    parent: Option<&CompactionParentContext>,
) -> Result<(Vec<Message>, CompactionMode)> {
    if history.is_empty() {
        return Ok((Vec::new(), CompactionMode::Local));
    }
    let options = resolve_manual_compaction_options(provider, model, options)?;
    match manual_compaction_strategy(provider, model) {
        CompactionStrategy::NativeStandalone => {
            let responses_options: ResponsesCompactionOptions = options.clone().into();
            // Bound the native input the same way as the local fork: a
            // near-full history would otherwise exceed the summarizer window
            // and fail the whole `/compact` command.
            let native_source = bound_history_for_native_compaction(
                history,
                options.instructions.as_deref().unwrap_or(""),
                compaction_history_budget(provider, model, context_budget),
            );
            let compacted = provider
                .compact_history_with_options(model, &native_source, &responses_options)
                .await
                .context("Failed to compact history via provider-native compaction")?;
            // A standalone compaction response is the provider's canonical
            // next context window. Keep its retained items and opaque
            // compaction items intact; the next provider request must receive
            // this window as returned rather than a locally pruned variant.
            Ok((compacted, CompactionMode::Provider))
        }
        CompactionStrategy::NativeInline => {
            compact_history_native_inline(provider, model, history, config, &options, context_budget, parent).await
        }
        CompactionStrategy::Local => {
            let compacted =
                summarize_locally(provider, model, history, config, &options, context_budget, parent).await?;
            Ok((compacted, CompactionMode::Local))
        }
    }
}

/// Resolve a manual compaction effort before selecting a provider strategy.
/// Every strategy eventually emits an `LLMRequest` or provider compaction
/// options, so validating once at this boundary prevents native, inline, and
/// hierarchical paths from silently coercing unsupported levels.
fn resolve_manual_compaction_options(
    provider: &dyn LLMProvider,
    model: &str,
    options: &ManualCompactionOptions,
) -> Result<ManualCompactionOptions> {
    let Some(requested) = options.reasoning_effort else {
        return Ok(options.clone());
    };

    let mapping = ReasoningEffortMapper::resolve(provider, model, requested, options.allow_reasoning_effort_downgrade)
        .with_context(|| {
            format!("Failed to resolve compaction reasoning effort for {} / {}", provider.name(), model)
        })?;

    if mapping.degraded() {
        tracing::warn!(
            provider = provider.name(),
            model,
            requested = %mapping.requested,
            effective = %mapping.effective,
            "Compaction reasoning effort explicitly downgraded"
        );
    }

    let mut resolved = options.clone();
    resolved.reasoning_effort = Some(mapping.effective);
    Ok(resolved)
}
