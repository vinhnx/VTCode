use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::fmt::Write;
use vtcode_commons::llm::FinishReason;
use vtcode_config::constants::context::DEFAULT_COMPACTION_TRIGGER_RATIO;

use crate::config::types::{ReasoningEffortLevel, VerbosityLevel};
use crate::exec::events::CompactionMode;
use crate::llm::provider::{
    LLMProvider, LLMRequest, Message, MessageRole, ResponsesCompactionOptions,
};
use crate::llm::utils::truncate_to_token_limit;

pub mod auto;
pub mod memory_envelope;
pub mod summarizer;

const DEFAULT_COMPACTION_TARGET_THRESHOLD: f64 = 0.50;
const DEFAULT_COMPACTION_KEEP_LAST_MESSAGES: usize = 10;
const DEFAULT_RETAINED_USER_MESSAGE_TOKENS: usize = 20_000;
const DEFAULT_RETAINED_USER_MESSAGES: usize = 6;
const SUMMARY_PREFIX: &str = "Previous conversation summary:\n";
const ABSTRACT_PREFIX: &str = "Earlier context (abstract):\n";
const DETAIL_PREFIX: &str = "Recent context (summary):\n";

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
        }
    }
}

/// Compact conversation history using the configured summarizer.
pub async fn compact_history(
    provider: &dyn LLMProvider,
    model: &str,
    history: &[Message],
    config: &CompactionConfig,
) -> Result<Vec<Message>> {
    if history.is_empty() {
        return Ok(Vec::new());
    }

    if !config.always_summarize && history.len() <= config.keep_last_messages {
        return Ok(history.to_vec());
    }

    if !config.always_summarize && provider.supports_responses_compaction(model) {
        return provider
            .compact_history(model, history)
            .await
            .context("Failed to compact history via Responses compact endpoint");
    }

    let summary_prompt = build_summary_prompt(history, &config.summary_prompt);
    let request = LLMRequest {
        messages: vec![Message::user(summary_prompt)],
        model: model.to_string(),
        ..Default::default()
    };

    let response = provider
        .generate(request)
        .await
        .context("Failed to generate compaction summary")?;

    let summary = response.content.unwrap_or_default().trim().to_string();
    Ok(build_local_compacted_history(
        history,
        &summary,
        config.retained_user_message_tokens,
        config.retained_user_messages,
        // The public `compact_history` entry feeds the fork/branch builder,
        // which produces a minimal resume artifact (envelope + summary +
        // retained users only) — no live continuity tail.
        false,
    ))
}

/// How the manual `/compact` command compacts for a given provider/model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionStrategy {
    /// Provider exposes a standalone on-demand compaction endpoint
    /// (OpenAI `/responses/compact`). Delegates to `LLMProvider::compact_history_with_options`.
    NativeStandalone,
    /// Provider compacts inline via request fields, threshold-triggered
    /// (Anthropic `compact_20260112`). Invoked through `LLMProvider::generate`
    /// with `context_management` set and `pause_after_compaction`.
    NativeInline,
    /// Universal fallback: summarize history via `LLMProvider::generate` and
    /// rebuild as a summary message plus retained recent user messages.
    /// Works for every provider.
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
    fn with_manual_overrides(self, options: &ManualCompactionOptions) -> Self {
        let summary_prompt = options
            .instructions
            .clone()
            .map(|instructions| instructions.trim().to_string())
            .filter(|instructions| !instructions.is_empty())
            .unwrap_or(self.summary_prompt);
        Self {
            summary_prompt,
            ..self
        }
    }
}

/// Compact history for the manual `/compact` command using provider-native
/// compaction when available, falling back to local summarization otherwise.
///
/// Returns the compacted messages and the `CompactionMode` that produced them
/// (`Provider` for native compaction, `Local` for client-side summarization).
pub async fn compact_history_manual(
    provider: &dyn LLMProvider,
    model: &str,
    history: &[Message],
    config: &CompactionConfig,
    options: &ManualCompactionOptions,
) -> Result<(Vec<Message>, CompactionMode)> {
    if history.is_empty() {
        return Ok((Vec::new(), CompactionMode::Local));
    }
    match manual_compaction_strategy(provider, model) {
        CompactionStrategy::NativeStandalone => {
            let responses_options: ResponsesCompactionOptions = options.clone().into();
            let compacted = provider
                .compact_history_with_options(model, history, &responses_options)
                .await
                .context("Failed to compact history via provider-native compaction")?;
            Ok((compacted, CompactionMode::Provider))
        }
        CompactionStrategy::NativeInline => {
            compact_history_native_inline(provider, model, history, config, options).await
        }
        CompactionStrategy::Local => {
            let compacted = summarize_locally(provider, model, history, config, options).await?;
            Ok((compacted, CompactionMode::Local))
        }
    }
}

/// Native inline compaction (Anthropic `compact_20260112`).
///
/// Forces a compaction pass by setting the minimum trigger threshold with
/// `pause_after_compaction: true`, so the response contains only the compaction
/// block. If compaction does not fire (history below the provider's minimum
/// trigger, currently 50k tokens for Anthropic), transparently falls back to
/// local summarization so the manual command always succeeds.
async fn compact_history_native_inline(
    provider: &dyn LLMProvider,
    model: &str,
    history: &[Message],
    config: &CompactionConfig,
    options: &ManualCompactionOptions,
) -> Result<(Vec<Message>, CompactionMode)> {
    const ANTHROPIC_COMPACT_TRIGGER_FLOOR: u64 = 50_000;

    let mut compact_edit = serde_json::Map::new();
    compact_edit.insert("type".to_string(), json!("compact_20260112"));
    compact_edit.insert(
        "trigger".to_string(),
        json!({ "type": "input_tokens", "value": ANTHROPIC_COMPACT_TRIGGER_FLOOR }),
    );
    compact_edit.insert("pause_after_compaction".to_string(), json!(true));
    if let Some(instructions) = options
        .instructions
        .as_ref()
        .map(|instructions| instructions.trim())
        .filter(|instructions| !instructions.is_empty())
    {
        compact_edit.insert("instructions".to_string(), json!(instructions));
    }

    let request = LLMRequest {
        messages: history.to_vec(),
        model: model.to_string(),
        context_management: Some(json!({ "edits": [Value::Object(compact_edit)] })),
        max_tokens: options.max_output_tokens,
        reasoning_effort: options.reasoning_effort,
        verbosity: options.verbosity,
        ..Default::default()
    };

    // The inline compaction request is Anthropic-specific (`compact_20260112`).
    // If the provider does not actually support inline compaction (e.g. it was
    // selected because it exposes a different standalone Responses compact
    // endpoint) the request may be rejected. Per the manual `/compact` contract
    // ("always succeeds"), swallow the inline error and fall back to local
    // summarization rather than aborting the whole command.
    let response = match provider.generate(request).await {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(
                error = %error,
                "provider-native inline compaction request failed; \
                 falling back to local summarization"
            );
            let compacted = summarize_locally(provider, model, history, config, options).await?;
            return Ok((compacted, CompactionMode::Local));
        }
    };

    if response.finish_reason == FinishReason::Pause
        && let Some(summary) = response
            .compaction
            .as_ref()
            .map(|summary| summary.trim())
            .filter(|summary| !summary.is_empty())
    {
        let retained_users = collect_retained_user_messages(
            history,
            config.retained_user_message_tokens,
            config.retained_user_messages,
        );
        let mut compacted = Vec::with_capacity(retained_users.len().saturating_add(1));
        compacted.push(Message::system(format!("{SUMMARY_PREFIX}{summary}")));
        compacted.extend(retained_users);
        return Ok((compacted, CompactionMode::Provider));
    }

    // Compaction did not fire (e.g. history below the minimum trigger threshold);
    // fall back to local summarization so the manual command always succeeds.
    let compacted = summarize_locally(provider, model, history, config, options).await?;
    Ok((compacted, CompactionMode::Local))
}

/// Local (provider-agnostic) summarization compaction.
///
/// Builds a summary prompt from the history, asks the provider to summarize via
/// `generate`, and rebuilds the history as a summary system message plus the
/// retained recent user messages. Applies the manual options to the summary
/// request.
///
/// When `config.hierarchical` is `true`, delegates to
/// [`summarize_locally_hierarchical`] which produces a multi-tier pyramid
/// (abstract + detail + verbatim) instead of a flat summary.
async fn summarize_locally(
    provider: &dyn LLMProvider,
    model: &str,
    history: &[Message],
    config: &CompactionConfig,
    options: &ManualCompactionOptions,
) -> Result<Vec<Message>> {
    if config.hierarchical {
        return summarize_locally_hierarchical(provider, model, history, config, options).await;
    }

    let effective_config = config.clone().with_manual_overrides(options);
    let summary_prompt = build_summary_prompt(history, &effective_config.summary_prompt);
    let request = LLMRequest {
        messages: vec![Message::user(summary_prompt)],
        model: model.to_string(),
        max_tokens: options.max_output_tokens,
        reasoning_effort: options.reasoning_effort,
        verbosity: options.verbosity,
        ..Default::default()
    };

    let response = provider
        .generate(request)
        .await
        .context("Failed to generate compaction summary")?;

    let summary = response.content.unwrap_or_default().trim().to_string();
    Ok(build_local_compacted_history(
        history,
        &summary,
        config.retained_user_message_tokens,
        config.retained_user_messages,
        // Live compaction: retain the most recent turn verbatim for continuity.
        true,
    ))
}

/// Hierarchical local summarization: abstract + detail + verbatim pyramid.
///
/// Splits the history into three bands and summarizes each with a different
/// compression target:
/// - **Abstract** (oldest third): 1-2 sentence overview
/// - **Detail** (middle third): paragraph-level summary
/// - **Verbatim** (newest third): kept as-is plus importance-weighted retained messages
///
/// This follows the hierarchical summarization strategy from the context window
/// management literature: recent turns verbatim, older turns as paragraph
/// summaries, oldest turns as a single abstract.
async fn summarize_locally_hierarchical(
    provider: &dyn LLMProvider,
    model: &str,
    history: &[Message],
    config: &CompactionConfig,
    options: &ManualCompactionOptions,
) -> Result<Vec<Message>> {
    let effective_config = config.clone().with_manual_overrides(options);

    // Split history into three bands at roughly equal thirds.
    let total = history.len();
    let band_size = total / 3;
    let abstract_end = band_size;
    let detail_end = band_size * 2;

    // Band 1 (oldest): compress into 1-2 sentence abstract.
    let abstract_band = &history[..abstract_end];
    let abstract_prompt = format!(
        "In 1-2 sentences, what was the overall goal and major progress in this \
         portion of the conversation?\n\n{}",
        build_summary_prompt(abstract_band, ""),
    );
    let abstract_request = LLMRequest {
        messages: vec![Message::user(abstract_prompt)],
        model: model.to_string(),
        max_tokens: Some(150),
        reasoning_effort: options.reasoning_effort,
        verbosity: options.verbosity,
        ..Default::default()
    };
    let abstract_response = provider
        .generate(abstract_request)
        .await
        .context("Failed to generate abstract summary")?;
    let abstract_summary = abstract_response
        .content
        .unwrap_or_default()
        .trim()
        .to_string();

    // Band 2 (middle): paragraph-level summary using the full summary prompt.
    let detail_band = &history[abstract_end..detail_end];
    let detail_prompt = build_summary_prompt(detail_band, &effective_config.summary_prompt);
    let detail_request = LLMRequest {
        messages: vec![Message::user(detail_prompt)],
        model: model.to_string(),
        max_tokens: options.max_output_tokens,
        reasoning_effort: options.reasoning_effort,
        verbosity: options.verbosity,
        ..Default::default()
    };
    let detail_response = provider
        .generate(detail_request)
        .await
        .context("Failed to generate detail summary")?;
    let detail_summary = detail_response
        .content
        .unwrap_or_default()
        .trim()
        .to_string();

    // Band 3 (newest): retain verbatim via importance-weighted selection.
    let recent_band = &history[detail_end..];
    let retained = collect_retained_user_messages(
        recent_band,
        config.retained_user_message_tokens,
        config.retained_user_messages,
    );

    // Assemble: [abstract, detail, ...retained_recent, ...continuity_tail]
    let mut new_history = Vec::with_capacity(2 + retained.len());
    new_history.push(Message::system(format!(
        "{ABSTRACT_PREFIX}{abstract_summary}"
    )));
    new_history.push(Message::system(format!("{DETAIL_PREFIX}{detail_summary}")));
    new_history.extend(retained);
    // Live compaction: retain the most recent turn verbatim for continuity.
    for message in continuity_tail(history) {
        if !new_history.iter().any(|existing| {
            existing.role == message.role && existing.content.as_text() == message.content.as_text()
        }) {
            new_history.push(message.clone());
        }
    }
    Ok(new_history)
}

fn build_summary_prompt(history: &[Message], instructions: &str) -> String {
    // Pre-size for the header plus every (non-empty) message body, avoiding
    // repeated reallocations while the summary prompt is assembled.
    let estimated_len = instructions.len()
        + history
            .iter()
            .map(|m| m.content.as_text().len())
            .sum::<usize>()
        + history.len() * 16;
    let mut formatted = String::with_capacity(estimated_len);
    let now: DateTime<Utc> = Utc::now();
    let _ = writeln!(
        &mut formatted,
        "Summary requested at {}.\n{}",
        now.to_rfc3339(),
        instructions
    );

    for message in history {
        let role = match message.role {
            MessageRole::System => "system",
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::Tool => "tool",
        };
        let content = message.content.as_text();
        if content.trim().is_empty() {
            continue;
        }
        let _ = writeln!(&mut formatted, "\n[{}]\n{}", role, content.trim());
    }

    formatted
}

fn build_local_compacted_history(
    history: &[Message],
    summary: &str,
    retained_user_message_tokens: usize,
    retained_user_messages: usize,
    include_continuity_tail: bool,
) -> Vec<Message> {
    let retained_users = collect_retained_user_messages(
        history,
        retained_user_message_tokens,
        retained_user_messages,
    );
    let mut new_history = Vec::with_capacity(retained_users.len().saturating_add(1));
    new_history.push(Message::system(format!(
        "{SUMMARY_PREFIX}{}",
        summary.trim()
    )));
    new_history.extend(retained_users);

    // Continuity anchor: always retain the most recent turn verbatim so the
    // model keeps "what it was just doing" — its last assistant action and any
    // in-progress tool calls — rather than losing it inside the summary. Only
    // applied on the live compaction path; the fork builder passes `false`.
    if include_continuity_tail {
        for message in continuity_tail(history) {
            if !new_history.iter().any(|existing| {
                existing.role == message.role
                    && existing.content.as_text() == message.content.as_text()
            }) {
                new_history.push(message.clone());
            }
        }
    }
    new_history
}

/// The trailing run of messages forming the most recent turn. This slice must
/// survive compaction verbatim to preserve conversational continuity.
fn continuity_tail(history: &[Message]) -> &[Message] {
    if history.is_empty() {
        return &[];
    }
    let last_user = history
        .iter()
        .rposition(|message| message.role == MessageRole::User)
        .unwrap_or(0);
    let mut tail = &history[last_user..];
    // Drop a trailing assistant message that still carries pending tool calls
    // with no following tool result. An interrupted turn (e.g. the run loop hit
    // its turn budget or errored before tool results were appended) leaves such
    // a message at the end of history; sending it to a provider is invalid
    // because every tool call must be paired with a tool result. A complete turn
    // ends with a `Tool` message, which is *not* trimmed here.
    while let Some(last) = tail.last() {
        if last.role == MessageRole::Assistant
            && last
                .tool_calls
                .as_ref()
                .is_some_and(|calls| !calls.is_empty())
        {
            tail = &tail[..tail.len() - 1];
        } else {
            break;
        }
    }
    tail
}

fn collect_retained_user_messages(
    history: &[Message],
    token_budget: usize,
    max_messages: usize,
) -> Vec<Message> {
    if token_budget == 0 || max_messages == 0 {
        return Vec::new();
    }

    // Phase 1: select up to `max_messages` user messages, scored by importance.
    let total = history.len();
    let mut user_scored: Vec<(usize, f64, &Message)> = history
        .iter()
        .enumerate()
        .filter(|(_, m)| m.role == MessageRole::User && !m.content.trim().is_empty())
        .map(|(i, m)| {
            let score = score_message(m, i, total);
            (i, score, m)
        })
        .collect();
    user_scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let mut selected: Vec<(usize, Message)> = Vec::with_capacity(max_messages.min(history.len()));
    let mut remaining = token_budget;

    for (original_idx, _score, message) in &user_scored {
        if selected.len() >= max_messages {
            break;
        }
        let estimated = message.estimate_tokens();
        if estimated <= remaining {
            selected.push((*original_idx, (*message).clone()));
            remaining = remaining.saturating_sub(estimated);
            continue;
        }
        if let Some(truncated) = truncate_user_message(message, remaining) {
            selected.push((*original_idx, truncated));
        }
        break;
    }

    // Phase 2: if budget remains, add high-value non-user messages (tool
    // results, assistant tool calls) that fit within the remaining capacity.
    if selected.len() < max_messages && remaining > 0 {
        let mut non_user_scored: Vec<(usize, f64, &Message)> = history
            .iter()
            .enumerate()
            .filter(|(_, m)| {
                m.role != MessageRole::User
                    && m.role != MessageRole::System
                    && is_retainable_message(m)
            })
            .map(|(i, m)| {
                let score = score_message(m, i, total);
                (i, score, m)
            })
            .collect();
        non_user_scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        for (original_idx, _score, message) in &non_user_scored {
            if selected.len() >= max_messages {
                break;
            }
            let estimated = message.estimate_tokens();
            if estimated <= remaining {
                selected.push((*original_idx, (*message).clone()));
                remaining = remaining.saturating_sub(estimated);
            }
        }
    }

    // Re-sort by original conversation order, then enforce tool-call/turn
    // coherence so the compacted history is valid to send back to a provider.
    selected.sort_by_key(|(idx, _)| *idx);
    coherence_tool_call_pairs(history, &selected)
        .into_iter()
        .map(|(_, msg)| msg)
        .collect()
}

/// Keep retained tool-call turns internally consistent.
///
/// A `Tool` message references a tool call the model must have seen, and an
/// `Assistant` message that still carries `tool_calls` must be followed by the
/// results those calls produced. Sending either without its counterpart is
/// invalid: providers reject unmatched tool calls, and orphaned tool results
/// reference a call the model never observed. This pass:
///
/// - **Force-keeps** the `Tool` messages immediately following any retained
///   `Assistant` that carries `tool_calls`, so the model observes each call's
///   return value. A complete turn ends with its results, which survive even
///   if they push past the soft `max_messages` cap.
/// - **Drops** a retained `Tool` message whose calling `Assistant` (the message
///   directly before it in `history`) was *not* retained — an orphaned result
///   the model cannot reconcile.
///
/// Tool results that follow a plain `Assistant` (no `tool_calls`) are ordinary
/// turn output and are kept exactly as selected.
fn coherence_tool_call_pairs(
    history: &[Message],
    selected: &[(usize, Message)],
) -> Vec<(usize, Message)> {
    let mut keep: std::collections::HashSet<usize> = selected.iter().map(|(i, _)| *i).collect();

    for (idx, msg) in selected {
        if msg.role == MessageRole::Assistant
            && msg
                .tool_calls
                .as_ref()
                .is_some_and(|calls| !calls.is_empty())
        {
            let mut j = *idx + 1;
            while let Some(next) = history.get(j) {
                if next.role == MessageRole::Tool {
                    keep.insert(j);
                    j += 1;
                } else {
                    break;
                }
            }
        }
    }

    selected
        .iter()
        .filter(|(idx, msg)| {
            if msg.role == MessageRole::Tool {
                // Walk backward through this contiguous result run to decide
                // coherence against the calling assistant turn.
                let mut cursor = *idx;
                loop {
                    match history.get(cursor) {
                        Some(m) if m.role == MessageRole::Tool => {
                            cursor = match cursor.checked_sub(1) {
                                Some(c) => c,
                                None => break,
                            };
                        }
                        Some(m)
                            if m.role == MessageRole::Assistant
                                && m.tool_calls.as_ref().is_some_and(|c| !c.is_empty()) =>
                        {
                            // Reached the calling assistant: coherent only if it
                            // was retained.
                            return keep.contains(&cursor);
                        }
                        _ => {
                            // Plain assistant or boundary: ordinary output.
                            return keep.contains(idx);
                        }
                    }
                }
                keep.contains(idx)
            } else {
                keep.contains(idx)
            }
        })
        .cloned()
        .collect()
}

/// Score a message for importance-weighted retention during compaction.
///
/// Uses a weighted combination of content importance and recency:
/// - Messages containing errors, corrections, or tool results score higher
/// - Recent messages get a recency bonus
/// - Assistant messages with tool calls are moderately important
fn score_message(message: &Message, index: usize, total: usize) -> f64 {
    let content = message.content.as_text();
    let content_lower = content.to_lowercase();

    // Importance weight based on content signals.
    let importance = match message.role {
        MessageRole::User => {
            if contains_error_signal(&content_lower) {
                3.0
            } else if contains_correction_signal(&content_lower) {
                2.5
            } else {
                1.0
            }
        }
        MessageRole::Tool => {
            // Tool results contain factual data the model may need.
            2.0
        }
        MessageRole::Assistant => {
            if message.tool_calls.is_some() {
                // Assistant messages with tool calls show action taken.
                0.5
            } else {
                0.1
            }
        }
        MessageRole::System => 0.0,
    };

    // Recency bonus: linear from 0.0 (oldest) to 1.0 (newest).
    let recency = if total > 0 {
        index as f64 / total as f64
    } else {
        0.0
    };

    importance + recency
}

/// Check if content contains error or failure signals.
fn contains_error_signal(content: &str) -> bool {
    content.contains("error")
        || content.contains("failed")
        || content.contains("failure")
        || content.contains("panic")
        || content.contains("bug")
        || content.contains("broken")
        || content.contains("regression")
}

/// Check if content contains user correction signals.
fn contains_correction_signal(content: &str) -> bool {
    content.contains("no,")
        || content.contains("wrong")
        || content.contains("actually")
        || content.contains("fix")
        || content.contains("instead")
        || content.contains("should be")
        || content.contains("don't")
}

/// Whether a message is worth retaining during compaction.
fn is_retainable_message(message: &Message) -> bool {
    match message.role {
        MessageRole::User => !message.content.trim().is_empty(),
        MessageRole::Tool => !message.content.trim().is_empty(),
        MessageRole::Assistant => {
            // Retain assistant messages that contain tool calls (action history).
            message.tool_calls.is_some()
        }
        MessageRole::System => false,
    }
}

fn truncate_user_message(message: &Message, token_budget: usize) -> Option<Message> {
    if token_budget <= 4 {
        return None;
    }

    let available_content_tokens = token_budget.saturating_sub(4);
    let truncated =
        truncate_to_token_limit(message.content.as_text().as_ref(), available_content_tokens);
    let trimmed = truncated.trim();
    if trimmed.is_empty() {
        return None;
    }

    Some(Message::user(trimmed.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{
        CompactionConfig, ManualCompactionOptions, compact_history, compact_history_manual,
        continuity_tail, manual_compaction_strategy,
    };
    use crate::config::types::{ReasoningEffortLevel, VerbosityLevel};
    use crate::exec::events::CompactionMode;
    use crate::llm::provider::{
        LLMError, LLMProvider, LLMRequest, LLMResponse, Message, MessageRole,
        ResponsesCompactionOptions,
    };
    use async_trait::async_trait;
    use std::sync::Mutex;
    use vtcode_commons::llm::{FinishReason, ToolCall};

    struct StubProvider;

    struct NativeCompactionProvider;

    /// Provider that opts into the standalone manual-compaction path
    /// (`supports_manual_openai_compaction -> true`), e.g. OpenAI `/responses/compact`.
    struct ManualStandaloneProvider {
        last_options: Mutex<Option<ResponsesCompactionOptions>>,
    }

    /// Inline-compaction-capable provider (`supports_responses_compaction -> true`,
    /// `supports_manual_openai_compaction -> false`), e.g. Anthropic `compact_20260112`.
    /// Returns a `Pause` finish with a compaction block so the inline path succeeds.
    struct InlinePauseProvider {
        last_request: Mutex<Option<LLMRequest>>,
    }

    /// Capturing provider with no native support; used to assert the Local summary
    /// request carries the manual options.
    struct CapturingProvider {
        last_request: Mutex<Option<LLMRequest>>,
    }

    /// Inline-dispatched provider whose inline `generate` rejects the Anthropic
    /// `compact_20260112` edit. Models providers that report
    /// `supports_responses_compaction` but are not Anthropic-style inline
    /// compactors; the dispatch must fall back to Local rather than aborting.
    struct InlineRejectingProvider;

    /// Models an OpenAI-compatible custom endpoint (non-`api.openai.com` host or
    /// `provider_key_override`): it exposes the Responses API
    /// (`supports_responses_compaction == true`) but neither the standalone
    /// `/responses/compact` endpoint nor Anthropic inline compaction. The dispatch
    /// must pick `Local` rather than misrouting it to `NativeInline` (which would
    /// send an Anthropic `compact_20260112` edit only to be rejected).
    struct CompatibleEndpointProvider;

    #[async_trait]
    impl LLMProvider for StubProvider {
        fn name(&self) -> &str {
            "stub"
        }

        async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
            Ok(LLMResponse::new("stub-model", "summary"))
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["stub-model".to_string()]
        }

        fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
            Ok(())
        }
    }

    #[async_trait]
    impl LLMProvider for NativeCompactionProvider {
        fn name(&self) -> &str {
            "native"
        }

        async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
            Ok(LLMResponse::new("stub-model", "summary"))
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["stub-model".to_string()]
        }

        fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
            Ok(())
        }

        fn supports_responses_compaction(&self, _model: &str) -> bool {
            true
        }

        async fn compact_history(
            &self,
            _model: &str,
            _history: &[Message],
        ) -> Result<Vec<Message>, LLMError> {
            Ok(vec![Message::system("provider compacted".to_string())])
        }
    }

    #[async_trait]
    impl LLMProvider for ManualStandaloneProvider {
        fn name(&self) -> &str {
            "manual-standalone"
        }

        async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
            Ok(LLMResponse::new("stub-model", "summary"))
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["stub-model".to_string()]
        }

        fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
            Ok(())
        }

        fn supports_manual_openai_compaction(&self, _model: &str) -> bool {
            true
        }

        async fn compact_history_with_options(
            &self,
            _model: &str,
            _history: &[Message],
            options: &ResponsesCompactionOptions,
        ) -> Result<Vec<Message>, LLMError> {
            *self.last_options.lock().unwrap() = Some(options.clone());
            Ok(vec![Message::system(
                "provider standalone compacted".to_string(),
            )])
        }
    }

    #[async_trait]
    impl LLMProvider for InlinePauseProvider {
        fn name(&self) -> &str {
            "inline-pause"
        }

        async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
            *self.last_request.lock().unwrap() = Some(request);
            let mut response = LLMResponse::new("stub-model", "compacted by provider");
            response.finish_reason = FinishReason::Pause;
            response.compaction = Some("provider compaction summary".to_string());
            Ok(response)
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["stub-model".to_string()]
        }

        fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
            Ok(())
        }

        fn supports_responses_compaction(&self, _model: &str) -> bool {
            true
        }

        fn supports_native_inline_compaction(&self, _model: &str) -> bool {
            true
        }
    }

    #[async_trait]
    impl LLMProvider for CapturingProvider {
        fn name(&self) -> &str {
            "capturing"
        }

        async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
            *self.last_request.lock().unwrap() = Some(request);
            Ok(LLMResponse::new("stub-model", "summary"))
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["stub-model".to_string()]
        }

        fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
            Ok(())
        }
    }

    #[async_trait]
    impl LLMProvider for InlineRejectingProvider {
        fn name(&self) -> &str {
            "inline-rejecting"
        }

        async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
            // Reject only the inline compaction request (carries the Anthropic
            // `compact_20260112` edit); the Local summary request must succeed.
            if request.context_management.is_some() {
                return Err(LLMError::Provider {
                    message: "provider rejected inline compact edit".to_string(),
                    metadata: None,
                });
            }
            Ok(LLMResponse::new("stub-model", "summary"))
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["stub-model".to_string()]
        }

        fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
            Ok(())
        }

        fn supports_responses_compaction(&self, _model: &str) -> bool {
            true
        }

        fn supports_native_inline_compaction(&self, _model: &str) -> bool {
            true
        }
    }

    #[async_trait]
    impl LLMProvider for CompatibleEndpointProvider {
        fn name(&self) -> &str {
            "compatible-endpoint"
        }

        async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
            Ok(LLMResponse::new("stub-model", "summary"))
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["stub-model".to_string()]
        }

        fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
            Ok(())
        }

        // Reports Responses API support but neither standalone nor inline
        // compaction (defaults: supports_manual_openai_compaction and
        // supports_native_inline_compaction are both false).
        fn supports_responses_compaction(&self, _model: &str) -> bool {
            true
        }
    }

    fn sample_history() -> Vec<Message> {
        vec![
            Message::assistant("setup".to_string()),
            Message::user("first request".to_string()),
            Message::assistant("working".to_string()),
            Message::user("second request".to_string()),
        ]
    }

    /// Build an assistant message that carries a (single) pending tool call.
    fn assistant_with_calls(content: &str, call_id: &str) -> Message {
        let mut message = Message::assistant(content.to_string());
        message.tool_calls = Some(vec![ToolCall {
            id: call_id.to_string(),
            call_type: "function".to_string(),
            function: None,
            text: None,
            thought_signature: None,
        }]);
        message
    }

    #[test]
    fn collect_retained_keeps_tool_result_with_its_assistant() {
        // When the assistant tool-call turn is retained, its tool result must
        // survive so the turn stays coherent (the model sees each call's return).
        let history = vec![
            Message::user("u1".to_string()),
            assistant_with_calls("calling tool", "c1"),
            Message::tool_response("c1".to_string(), "r1".to_string()),
            Message::user("u2".to_string()),
        ];
        let retained = super::collect_retained_user_messages(&history, 20_000, 4);
        assert!(retained.iter().any(|m| m.content.as_text().contains("u1")));
        assert!(retained.iter().any(|m| m.content.as_text().contains("u2")));
        assert!(
            retained.iter().any(|m| m.content.as_text().contains("r1")),
            "tool result paired with its retained assistant must survive"
        );
    }

    #[test]
    fn collect_retained_drops_orphaned_tool_result() {
        // If the assistant tool-call turn is dropped (over the retention cap),
        // its tool result is orphaned — the model never saw the call — and must
        // not survive compaction, because an orphaned result is invalid to send
        // to a provider.
        let history = vec![
            Message::user("u1".to_string()),
            assistant_with_calls("calling tool", "c1"),
            Message::tool_response("c1".to_string(), "r1".to_string()),
            Message::user("u2".to_string()),
        ];
        let retained = super::collect_retained_user_messages(&history, 20_000, 3);
        assert!(retained.iter().any(|m| m.content.as_text().contains("u1")));
        assert!(retained.iter().any(|m| m.content.as_text().contains("u2")));
        assert!(
            !retained.iter().any(|m| m.content.as_text().contains("r1")),
            "orphaned tool result must be dropped"
        );
    }

    #[tokio::test]
    async fn manual_compaction_strategy_picks_local_for_plain_provider() {
        assert_eq!(
            manual_compaction_strategy(&StubProvider, "stub-model"),
            super::CompactionStrategy::Local
        );
    }

    #[tokio::test]
    async fn manual_compaction_strategy_picks_native_standalone_for_manual_provider() {
        let provider = ManualStandaloneProvider {
            last_options: Mutex::new(None),
        };
        assert_eq!(
            manual_compaction_strategy(&provider, "stub-model"),
            super::CompactionStrategy::NativeStandalone
        );
    }

    #[tokio::test]
    async fn manual_compaction_strategy_picks_native_inline_for_responses_capable_provider() {
        let provider = InlinePauseProvider {
            last_request: Mutex::new(None),
        };
        assert_eq!(
            manual_compaction_strategy(&provider, "stub-model"),
            super::CompactionStrategy::NativeInline
        );
    }

    #[tokio::test]
    async fn manual_compaction_strategy_picks_local_for_compatible_endpoint() {
        // OpenAI-compatible custom endpoints report `supports_responses_compaction`
        // but cannot serve standalone `/responses/compact` or Anthropic inline
        // compaction; they must route to Local, not NativeInline.
        assert_eq!(
            manual_compaction_strategy(&CompatibleEndpointProvider, "stub-model"),
            super::CompactionStrategy::Local
        );
    }

    #[tokio::test]
    async fn compact_history_manual_uses_local_summary_for_plain_provider() {
        let history = sample_history();
        let config = CompactionConfig {
            always_summarize: true,
            ..CompactionConfig::default()
        };

        let (compacted, mode) = compact_history_manual(
            &StubProvider,
            "stub-model",
            &history,
            &config,
            &ManualCompactionOptions::default(),
        )
        .await
        .expect("manual compaction");

        assert_eq!(mode, CompactionMode::Local);
        assert_eq!(compacted.len(), 3);
        assert_eq!(
            compacted[0].content.as_text(),
            "Previous conversation summary:\nsummary"
        );
        assert_eq!(compacted[1].content.as_text(), "first request");
        assert_eq!(compacted[2].content.as_text(), "second request");
    }

    #[tokio::test]
    async fn compact_history_manual_uses_native_standalone_for_manual_provider() {
        let history = sample_history();
        let config = CompactionConfig::default();
        let provider = ManualStandaloneProvider {
            last_options: Mutex::new(None),
        };

        let (compacted, mode) = compact_history_manual(
            &provider,
            "stub-model",
            &history,
            &config,
            &ManualCompactionOptions::default(),
        )
        .await
        .expect("manual compaction");

        assert_eq!(mode, CompactionMode::Provider);
        assert_eq!(compacted.len(), 1);
        assert_eq!(
            compacted[0].content.as_text(),
            "provider standalone compacted"
        );
    }

    #[tokio::test]
    async fn compact_history_manual_passes_options_to_native_standalone() {
        let history = sample_history();
        let config = CompactionConfig::default();
        let provider = ManualStandaloneProvider {
            last_options: Mutex::new(None),
        };
        let options = ManualCompactionOptions {
            instructions: Some("keep only decisions".to_string()),
            max_output_tokens: Some(256),
            reasoning_effort: Some(ReasoningEffortLevel::Minimal),
            verbosity: Some(VerbosityLevel::High),
        };

        let (_compacted, mode) =
            compact_history_manual(&provider, "stub-model", &history, &config, &options)
                .await
                .expect("manual compaction");

        assert_eq!(mode, CompactionMode::Provider);
        let captured = provider
            .last_options
            .lock()
            .unwrap()
            .clone()
            .expect("captured options");
        assert_eq!(
            captured.instructions.as_deref(),
            Some("keep only decisions")
        );
        assert_eq!(captured.max_output_tokens, Some(256));
        assert_eq!(
            captured.reasoning_effort,
            Some(ReasoningEffortLevel::Minimal)
        );
        assert_eq!(captured.verbosity, Some(VerbosityLevel::High));
    }

    #[tokio::test]
    async fn compact_history_manual_uses_native_inline_when_pause_and_compaction_present() {
        let history = sample_history();
        let config = CompactionConfig::default();
        let provider = InlinePauseProvider {
            last_request: Mutex::new(None),
        };

        let (compacted, mode) = compact_history_manual(
            &provider,
            "stub-model",
            &history,
            &config,
            &ManualCompactionOptions::default(),
        )
        .await
        .expect("manual compaction");

        assert_eq!(mode, CompactionMode::Provider);
        assert_eq!(compacted.len(), 3);
        assert_eq!(
            compacted[0].content.as_text(),
            "Previous conversation summary:\nprovider compaction summary"
        );
        assert_eq!(compacted[1].content.as_text(), "first request");
        assert_eq!(compacted[2].content.as_text(), "second request");

        // The inline request must carry the `compact_20260112` edit with a forced
        // pause so the provider actually performs compaction on demand.
        let captured = provider
            .last_request
            .lock()
            .unwrap()
            .clone()
            .expect("captured inline request");
        let context_management = captured
            .context_management
            .as_ref()
            .expect("context_management set on inline compaction request");
        let edit = &context_management["edits"][0];
        assert_eq!(edit["type"].as_str(), Some("compact_20260112"));
        assert_eq!(edit["pause_after_compaction"].as_bool(), Some(true));
        assert_eq!(edit["trigger"]["value"].as_u64(), Some(50_000));
    }

    #[tokio::test]
    async fn compact_history_manual_inline_request_carries_instructions_when_provided() {
        let history = sample_history();
        let config = CompactionConfig::default();
        let provider = InlinePauseProvider {
            last_request: Mutex::new(None),
        };
        let options = ManualCompactionOptions {
            instructions: Some("  keep only decisions  ".to_string()),
            ..ManualCompactionOptions::default()
        };

        let (_compacted, _mode) =
            compact_history_manual(&provider, "stub-model", &history, &config, &options)
                .await
                .expect("manual compaction");

        let captured = provider
            .last_request
            .lock()
            .unwrap()
            .clone()
            .expect("captured inline request");
        let edit = &captured
            .context_management
            .as_ref()
            .expect("context_management")["edits"][0];
        assert_eq!(edit["instructions"].as_str(), Some("keep only decisions"));
    }

    #[tokio::test]
    async fn compact_history_manual_falls_back_to_local_when_inline_compaction_not_fired() {
        let history = sample_history();
        let config = CompactionConfig::default();

        // NativeCompactionProvider is inline-capable but its `generate` returns a
        // normal `Stop` with no compaction block, so the inline attempt cannot
        // fire and the dispatch must transparently fall back to Local.
        let (compacted, mode) = compact_history_manual(
            &NativeCompactionProvider,
            "stub-model",
            &history,
            &config,
            &ManualCompactionOptions::default(),
        )
        .await
        .expect("manual compaction");

        assert_eq!(mode, CompactionMode::Local);
        assert_eq!(compacted.len(), 3);
        assert_eq!(
            compacted[0].content.as_text(),
            "Previous conversation summary:\nsummary"
        );
    }

    #[tokio::test]
    async fn compact_history_manual_falls_back_to_local_when_inline_request_errors() {
        let history = sample_history();
        let config = CompactionConfig::default();

        // A provider dispatched to NativeInline that rejects the Anthropic
        // `compact_20260112` edit must not abort the whole command; the dispatch
        // falls back to Local summarization (the manual `/compact` contract:
        // always succeeds).
        let (compacted, mode) = compact_history_manual(
            &InlineRejectingProvider,
            "stub-model",
            &history,
            &config,
            &ManualCompactionOptions::default(),
        )
        .await
        .expect("manual compaction should fall back to local");

        assert_eq!(mode, CompactionMode::Local);
        assert_eq!(compacted.len(), 3);
        assert_eq!(
            compacted[0].content.as_text(),
            "Previous conversation summary:\nsummary"
        );
    }

    #[tokio::test]
    async fn compact_history_manual_applies_options_to_local_summary_request() {
        let history = sample_history();
        let config = CompactionConfig {
            always_summarize: true,
            ..CompactionConfig::default()
        };
        let provider = CapturingProvider {
            last_request: Mutex::new(None),
        };
        let options = ManualCompactionOptions {
            instructions: Some("KEEP DECISIONS ONLY".to_string()),
            max_output_tokens: Some(128),
            reasoning_effort: Some(ReasoningEffortLevel::Minimal),
            verbosity: Some(VerbosityLevel::High),
        };

        let (compacted, mode) =
            compact_history_manual(&provider, "stub-model", &history, &config, &options)
                .await
                .expect("manual compaction");

        assert_eq!(mode, CompactionMode::Local);
        let captured = provider
            .last_request
            .lock()
            .unwrap()
            .clone()
            .expect("captured summary request");
        assert_eq!(captured.max_tokens, Some(128));
        assert_eq!(
            captured.reasoning_effort,
            Some(ReasoningEffortLevel::Minimal)
        );
        assert_eq!(captured.verbosity, Some(VerbosityLevel::High));
        // The custom instructions override the default summary prompt.
        let prompt = captured.messages[0].content.as_text();
        assert!(prompt.contains("KEEP DECISIONS ONLY"));
        assert!(!prompt.contains("acceptance criteria"));
        assert_eq!(
            compacted[0].content.as_text(),
            "Previous conversation summary:\nsummary"
        );
    }

    #[tokio::test]
    async fn compact_history_manual_returns_empty_for_empty_history() {
        let (compacted, mode) = compact_history_manual(
            &StubProvider,
            "stub-model",
            &[],
            &CompactionConfig::default(),
            &ManualCompactionOptions::default(),
        )
        .await
        .expect("manual compaction");

        assert!(compacted.is_empty());
        assert_eq!(mode, CompactionMode::Local);
    }

    #[tokio::test]
    async fn compact_history_rebuilds_history_around_summary_and_important_messages() {
        let history = vec![
            Message::assistant("setup".to_string()),
            Message::user("first request".to_string()),
            Message::assistant("working".to_string()),
            Message::tool_response("call-1".to_string(), "done".to_string()),
            Message::user("second request".to_string()),
            Message::assistant("final reply".to_string()),
        ];
        let config = CompactionConfig {
            always_summarize: true,
            ..CompactionConfig::default()
        };

        let compacted = compact_history(&StubProvider, "stub-model", &history, &config)
            .await
            .expect("compacted history");

        // Summary + importance-weighted retained messages (user messages + tool
        // response). The public `compact_history` entry is the fork/branch
        // builder, which intentionally omits the live continuity tail.
        assert_eq!(compacted.len(), 4);
        assert_eq!(
            compacted[0].content.as_text(),
            "Previous conversation summary:\nsummary"
        );
        // Messages are in original conversation order.
        assert_eq!(compacted[1].content.as_text(), "first request");
        assert_eq!(compacted[2].content.as_text(), "done");
        assert_eq!(compacted[3].content.as_text(), "second request");
    }

    #[tokio::test]
    async fn compact_history_truncates_oldest_retained_user_message_to_budget() {
        let history = vec![
            Message::user("alpha beta gamma delta epsilon zeta".to_string()),
            Message::assistant("ack".to_string()),
            Message::user("newest request".to_string()),
        ];
        let config = CompactionConfig {
            always_summarize: true,
            retained_user_message_tokens: 8,
            ..CompactionConfig::default()
        };

        let compacted = compact_history(&StubProvider, "stub-model", &history, &config)
            .await
            .expect("compacted history");

        assert_eq!(compacted.len(), 2);
        assert_eq!(compacted[1].content.as_text(), "newest request");
    }

    #[tokio::test]
    async fn compact_history_caps_retained_user_message_count() {
        let history = vec![
            Message::user("first request".to_string()),
            Message::assistant("ack".to_string()),
            Message::user("second request".to_string()),
            Message::assistant("ack".to_string()),
            Message::user("third request".to_string()),
            Message::assistant("ack".to_string()),
            Message::user("fourth request".to_string()),
            Message::assistant("ack".to_string()),
            Message::user("fifth request".to_string()),
        ];
        let config = CompactionConfig {
            always_summarize: true,
            retained_user_messages: 4,
            ..CompactionConfig::default()
        };

        let compacted = compact_history(&StubProvider, "stub-model", &history, &config)
            .await
            .expect("compacted history");

        let retained = compacted
            .iter()
            .skip(1)
            .map(|message| message.content.as_text().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            retained,
            vec![
                "second request".to_string(),
                "third request".to_string(),
                "fourth request".to_string(),
                "fifth request".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn compact_history_forces_local_summary_when_always_summarize_is_enabled() {
        let history = vec![
            Message::user("first request".to_string()),
            Message::assistant("working".to_string()),
            Message::user("second request".to_string()),
        ];
        let config = CompactionConfig {
            always_summarize: true,
            ..CompactionConfig::default()
        };

        let compacted = compact_history(&NativeCompactionProvider, "stub-model", &history, &config)
            .await
            .expect("compacted history");

        assert_eq!(compacted.len(), 3);
        assert_eq!(
            compacted[0].content.as_text(),
            "Previous conversation summary:\nsummary"
        );
        assert_eq!(compacted[1].content.as_text(), "first request");
        assert_eq!(compacted[2].content.as_text(), "second request");
    }

    #[test]
    fn default_summary_prompt_preserves_required_compaction_context() {
        let prompt = CompactionConfig::default().summary_prompt;

        assert!(prompt.contains("acceptance criteria"));
        assert!(prompt.contains("file paths that were read or modified"));
        assert!(prompt.contains("test results and error messages"));
        assert!(prompt.contains("decisions with their reasoning"));
    }

    #[test]
    fn continuity_tail_keeps_complete_turn_but_drops_unmatched_tool_call() {
        // A completed turn: user -> assistant(tool call) -> tool result. The
        // tail must keep the whole turn intact (the tool result makes the
        // trailing assistant tool call valid to send).
        let complete = vec![
            Message::user("do the thing".into()),
            {
                let mut m = Message::assistant("calling".into());
                m.tool_calls = Some(vec![ToolCall::function(
                    "c1".into(),
                    "run".into(),
                    "{}".into(),
                )]);
                m
            },
            Message::tool_response("c1".into(), "ran".into()),
        ];
        assert_eq!(continuity_tail(&complete).len(), 3);

        // An interrupted turn: user -> assistant(tool call) with no tool
        // result. Sending the trailing assistant message to a provider is
        // invalid, so the tail must drop it and keep only the user message.
        let interrupted = vec![Message::user("do the thing".into()), {
            let mut m = Message::assistant("calling".into());
            m.tool_calls = Some(vec![ToolCall::function(
                "c1".into(),
                "run".into(),
                "{}".into(),
            )]);
            m
        }];
        let tail = continuity_tail(&interrupted);
        assert_eq!(tail.len(), 1);
        assert_eq!(tail[0].role, MessageRole::User);
    }
}
