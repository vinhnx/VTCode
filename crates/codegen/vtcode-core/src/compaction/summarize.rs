//! Local summarization: prompts, retries, and hierarchical bands.

use super::*;

/// Keep the existing safety headroom while reserving the frozen request prefix
/// and any explicit summary output limit. Unknown capacity keeps legacy bounds.
fn local_summary_history_budget(
    provider: &dyn LLMProvider,
    model: &str,
    context_budget: Option<usize>,
    instructions: &str,
    max_output_tokens: Option<u32>,
    parent: Option<&CompactionParentContext>,
) -> Result<Option<usize>> {
    let context_size =
        effective_session_context_budget(provider.effective_context_size(model), context_budget.unwrap_or(0));
    if context_size == 0 {
        return Ok(None);
    }
    let prefix_tokens = parent.map_or(0, |parent| {
        let system_tokens = parent
            .system_prompt
            .as_ref()
            .map_or(0, |prompt| crate::prompts::system::estimate_token_count(prompt) as usize);
        let tool_tokens = parent
            .tools
            .as_ref()
            .map_or(0, |tools| crate::llm::usage_cost::estimate_tool_definition_tokens(tools) as usize);
        system_tokens.saturating_add(tool_tokens)
    });
    let output_reserve = max_output_tokens.map_or(0, |tokens| tokens as usize);
    let history_budget = compaction_history_budget(provider, model, Some(context_size))
        .unwrap_or(0)
        .saturating_sub(prefix_tokens)
        .saturating_sub(output_reserve);
    // A positive budget must fit the instruction plus one minimal history
    // message. Otherwise the bounder's minimum (or zero-as-unknown sentinel)
    // would turn an exhausted budget back into an oversized request.
    let minimum_tokens = Message::user(instructions.to_owned()).estimate_tokens().saturating_add(4);
    anyhow::ensure!(
        history_budget >= minimum_tokens,
        "local summary request budget exhausted for {} / {} (context_size={context_size}, prefix_tokens={prefix_tokens}, output_reserve={output_reserve}, minimum_tokens={minimum_tokens})",
        provider.name(),
        model,
    );
    Ok(Some(history_budget))
}

fn bound_local_summary_history(history: &[Message], instructions: &str, budget: Option<usize>) -> Result<Vec<Message>> {
    let bounded = bound_history_for_summarization(history, instructions, budget);
    if let Some(budget) = budget {
        let request_tokens = bounded
            .iter()
            .map(Message::estimate_tokens)
            .fold(Message::user(instructions.to_owned()).estimate_tokens(), usize::saturating_add);
        anyhow::ensure!(
            request_tokens <= budget,
            "local summary request budget exhausted by minimum protocol history (request_tokens={request_tokens}, budget={budget})",
        );
    }
    Ok(bounded)
}

/// Build a cache-safe local-summary request that reuses the parent prefix.
/// `tool_choice` is forced to `none` so the summarizer cannot spend the
/// compaction pass on tool calls while the system/tools/messages prefix
/// stays identical to the parent's last request.
///
/// `supports_turn_scoped` mirrors the live turn path: canonical history keeps
/// the typed `clear_at` marker, but routes without native support receive it
/// as an ordinary system directive (text preserved, `clear_at` stripped) so
/// `merge-gateway` and other non-Anthropic wires do not fail validation.
pub(crate) fn compaction_summary_request(
    model: &str,
    history: &[Message],
    instructions: &str,
    max_output_tokens: Option<u32>,
    reasoning_effort: Option<ReasoningEffortLevel>,
    verbosity: Option<VerbosityLevel>,
    supports_turn_scoped: bool,
    parent: Option<&CompactionParentContext>,
) -> LLMRequest {
    let mut forked = build_cache_safe_compaction_history(history, instructions);
    if !supports_turn_scoped {
        for message in forked.iter_mut() {
            if message.clear_at.is_some() {
                message.clear_at = None;
            }
        }
    }
    LLMRequest {
        messages: Arc::new(forked),
        model: model.to_string(),
        system_prompt: parent.and_then(|parent| parent.system_prompt.clone()),
        tools: parent.and_then(|parent| parent.tools.clone()).filter(|tools| !tools.is_empty()),
        tool_choice: Some(ToolChoice::none()),
        max_tokens: max_output_tokens,
        reasoning_effort,
        verbosity,
        ..Default::default()
    }
}

/// Compact conversation history using the configured summarizer.
#[cfg_attr(feature = "profiling", hotpath::measure)]
pub async fn compact_history(
    provider: &dyn LLMProvider,
    model: &str,
    history: &[Message],
    config: &CompactionConfig,
) -> Result<Vec<Message>> {
    compact_history_with_budget(provider, model, history, config, None).await
}

/// Compact conversation history using a caller-resolved context budget for
/// input bounding and locally rebuilt output. Standalone native responses are
/// returned as the provider's canonical next context window.
pub async fn compact_history_with_budget(
    provider: &dyn LLMProvider,
    model: &str,
    history: &[Message],
    config: &CompactionConfig,
    context_budget: Option<usize>,
) -> Result<Vec<Message>> {
    if history.is_empty() {
        return Ok(Vec::new());
    }

    if !config.always_summarize && history.len() <= config.keep_last_messages {
        // Message-count retention is only a fast path. A single large tool
        // result can exceed the resolved token budget even when the history
        // contains fewer messages than `keep_last_messages`.
        return Ok(bound_compacted_history_to_context(history.to_vec(), provider, model, context_budget));
    }

    // `supports_responses_compaction` is shared by standalone Responses
    // endpoints and Anthropic's inline context-management path. This legacy
    // function calls `compact_history` directly, so only the narrower
    // standalone capability is safe here; inline providers use the manual
    // strategy dispatcher below (or the local fallback on this legacy path).
    if !config.always_summarize && provider.supports_manual_openai_compaction(model) {
        let native_source = bound_history_for_native_compaction(
            history,
            "",
            compaction_history_budget(provider, model, context_budget),
        );
        let compacted = provider
            .compact_history(model, &native_source)
            .await
            .context("Failed to compact history via Responses compact endpoint")?;
        // A standalone compaction response is the provider's canonical next
        // context window. Do not append a locally selected continuity tail or
        // discard opaque provider items from it. The provider has already
        // produced the exact window that must be replayed on the next turn.
        return Ok(compacted);
    }

    let effective_config = context_bounded_compaction_config(provider, model, history, config, context_budget);
    // Cache-safe forking: prepend the parent's full conversation prefix and
    // append the compaction instruction, instead of reformatting the entire
    // history into a single new user message (which would pay the full
    // uncached input rate). No parent system/tools are available on this
    // legacy path; callers with a request envelope should prefer
    // `compact_history_manual_with_parent_context`.
    // Bound the fork so the summary request itself fits the summarizer window;
    // an unbounded fork is what made `/compact` fail on near-full or
    // smaller-window (model-switch) histories.
    let history_budget =
        local_summary_history_budget(provider, model, context_budget, &effective_config.summary_prompt, None, None)?;
    let tail_target = route_tail_target_tokens(provider, model, context_budget);
    // Trim oversized tool outputs before bounding so large dumps do not evict
    // whole protocol groups from the summarizer fork.
    let pruned_history = prune_oversized_tool_outputs(history);
    let summary_source =
        bound_local_summary_history(&pruned_history, &effective_config.summary_prompt, history_budget)?;
    let summary = generate_local_summary_with_retry(
        provider,
        model,
        &pruned_history,
        &summary_source,
        &effective_config.summary_prompt,
        history_budget,
        &ManualCompactionOptions::default(),
        None,
    )
    .await?;

    Ok(bound_compacted_history_to_context(
        build_local_compacted_history(
            history,
            &summary,
            effective_config.retained_user_message_tokens,
            effective_config.retained_user_messages,
            // Keep the same protocol-safe continuity tail as the live/manual
            // paths. Forked histories must not lose the newest working turn.
            true,
            tail_target,
        ),
        provider,
        model,
        context_budget,
    ))
}

/// Local (provider-agnostic) summarization compaction.
///
/// Forks the parent conversation cache-safely: same history prefix plus the
/// compaction instruction appended, with the parent system/tools prefix when
/// supplied (`None` keeps the legacy standalone shape). Rebuilds the history
/// as a summary system message plus the retained recent user messages.
/// Applies the manual options to the summary request.
///
/// When `config.hierarchical` is `true`, delegates to
/// [`summarize_locally_hierarchical`] which produces a multi-tier pyramid
/// (abstract + detail + verbatim) instead of a flat summary./// Local (provider-agnostic) summarization compaction.
///
/// Forks the parent conversation cache-safely: same history prefix plus the
/// compaction instruction appended, with the parent system/tools prefix when
/// supplied (`None` keeps the legacy standalone shape). Rebuilds the history
/// as a summary system message plus the retained recent user messages.
/// Applies the manual options to the summary request.
///
/// When `config.hierarchical` is `true`, delegates to
/// [`summarize_locally_hierarchical`] which produces a multi-tier pyramid
/// (abstract + detail + verbatim) instead of a flat summary.
pub(crate) async fn summarize_locally(
    provider: &dyn LLMProvider,
    model: &str,
    history: &[Message],
    config: &CompactionConfig,
    options: &ManualCompactionOptions,
    context_budget: Option<usize>,
    parent: Option<&CompactionParentContext>,
) -> Result<Vec<Message>> {
    if config.hierarchical {
        return summarize_locally_hierarchical(provider, model, history, config, options, context_budget, parent).await;
    }

    let effective_config = context_bounded_compaction_config(
        provider,
        model,
        history,
        &config.clone().with_manual_overrides(options),
        context_budget,
    );
    let history_budget = local_summary_history_budget(
        provider,
        model,
        context_budget,
        &effective_config.summary_prompt,
        options.max_output_tokens,
        parent,
    )?;
    let tail_target = route_tail_target_tokens(provider, model, context_budget);
    // Bound the fork so the summary request itself fits the summarizer window.
    // `/compact` is normally run when the context is already near full, so an
    // unbounded fork is rejected by the provider and the whole command fails.
    // Oversized tool outputs are pruned first so large dumps do not evict
    // whole protocol groups from the fork.
    let pruned_history = prune_oversized_tool_outputs(history);
    let summary_source =
        bound_local_summary_history(&pruned_history, &effective_config.summary_prompt, history_budget)?;
    let summary = generate_local_summary_with_retry(
        provider,
        model,
        &pruned_history,
        &summary_source,
        &effective_config.summary_prompt,
        history_budget,
        options,
        parent,
    )
    .await?;

    Ok(bound_compacted_history_to_context(
        build_summary_compacted_history(history, summary, &effective_config, true, tail_target),
        provider,
        model,
        context_budget,
    ))
}

/// Generate a summary request, retrying with progressively halved input budgets
/// when the provider rejects the request as over context capacity.
///
/// Token estimates are heuristic, so a bounded input can still overflow the
/// summarizer window. Bounded retries recover instead of failing the whole
/// `/compact` command; any other error (or exhausted retries) propagates.
/// `make_request` rebuilds the request shape (cache-safe fork or single-shot
/// band prompt) from the given source messages so every local summarization
/// path shares one retry contract.
pub(crate) async fn generate_summary_with_capacity_retry(
    provider: &dyn LLMProvider,
    model: &str,
    history: &[Message],
    instructions: &str,
    first_source: &[Message],
    history_budget: Option<usize>,
    max_overflow_retries: u32,
    error_context: &'static str,
    make_request: impl Fn(&[Message]) -> LLMRequest,
) -> Result<String> {
    let first_tokens = first_source.iter().map(Message::estimate_tokens).sum::<usize>();
    let mut current_source: &[Message] = first_source;
    let mut current_tokens = first_tokens;
    let mut current_budget = history_budget;
    let mut remaining_retries = max_overflow_retries;
    let mut retry_storage: Vec<Message>;
    loop {
        let mut request = make_request(current_source);
        // Compaction forks can start from legacy or partially recorded
        // history, outside the normal turn request builder. Repair tool-call
        // pairs at this request boundary so compatible gateways never see an
        // assistant call without its result. The shared normalizer is
        // copy-on-write, so clean histories retain their existing allocation
        // and failed compaction does not mutate session history.
        request.messages =
            crate::core::agent::state::normalize_history_for_request_shared(Arc::clone(&request.messages));
        match collect_single_response(provider, request).await {
            Ok(response) => {
                let summary = response.content.unwrap_or_default().trim().to_string();
                if summary.is_empty() {
                    return Err(anyhow::anyhow!(
                        "{error_context} for {} / {}: provider returned an empty summary (input_tokens={current_tokens}, budget={current_budget:?})",
                        provider.name(),
                        model,
                    ));
                }
                return Ok(summary);
            }
            Err(error) if is_context_capacity_message(&error.to_string()) && remaining_retries > 0 => {
                // With an unknown budget the bound is verbatim, so halving
                // `None` would resend the identical fork. Derive a fallback
                // from the failing attempt so the retry can still shrink.
                current_budget = match current_budget {
                    Some(budget) => Some((budget / 2).max(4)),
                    None => Some((current_tokens / 2).max(4)),
                };
                retry_storage = bound_history_for_summarization(history, instructions, current_budget);
                // A retry only helps when it actually shrinks the input: with
                // an already-fitting history it would resend the identical
                // request, so propagate the original failure instead.
                let retry_tokens = retry_storage.iter().map(Message::estimate_tokens).sum::<usize>();
                if retry_tokens >= current_tokens {
                    return Err(anyhow::Error::from(error).context(format!(
                        "{error_context} for {} / {} (input_tokens={current_tokens}, budget={current_budget:?})",
                        provider.name(),
                        model,
                    )));
                }
                tracing::warn!(
                    provider = provider.name(),
                    model,
                    input_tokens = current_tokens,
                    retry_tokens,
                    budget = ?current_budget,
                    error = ?error,
                    "{error_context}: input exceeded context capacity; retrying with a halved input budget"
                );
                current_source = &retry_storage;
                current_tokens = retry_tokens;
                remaining_retries -= 1;
            }
            Err(error) => {
                return Err(anyhow::Error::from(error).context(format!(
                    "{error_context} for {} / {} (input_tokens={current_tokens}, budget={current_budget:?})",
                    provider.name(),
                    model,
                )));
            }
        }
    }
}

/// Generate a cache-safe fork summary with the shared capacity-retry contract.
async fn generate_local_summary_with_retry(
    provider: &dyn LLMProvider,
    model: &str,
    history: &[Message],
    summary_source: &[Message],
    instructions: &str,
    history_budget: Option<usize>,
    options: &ManualCompactionOptions,
    parent: Option<&CompactionParentContext>,
) -> Result<String> {
    let supports_turn_scoped = provider.supports_turn_scoped_system_messages(model);
    generate_summary_with_capacity_retry(
        provider,
        model,
        history,
        instructions,
        summary_source,
        history_budget,
        CompactionRoutePolicy::resolve(provider.name(), model).max_overflow_retries,
        "Failed to generate compaction summary",
        |source| {
            compaction_summary_request(
                model,
                source,
                instructions,
                options.max_output_tokens,
                options.reasoning_effort,
                options.verbosity,
                supports_turn_scoped,
                parent,
            )
        },
    )
    .await
}

/// Intro framing for the hierarchical abstract band (oldest third).
const ABSTRACT_BAND_INTRO: &str =
    "In 1-2 sentences, what was the overall goal and major progress in this portion of the conversation?\n\n";

/// Build a single-shot band summary request: one user message carrying the
/// pre-rendered band prompt, with the parent system/tools prefix reused.
fn band_summary_request(
    model: &str,
    prompt: String,
    max_tokens: Option<u32>,
    options: &ManualCompactionOptions,
    parent: Option<&CompactionParentContext>,
) -> LLMRequest {
    LLMRequest {
        messages: Arc::new(vec![Message::user(prompt)]),
        model: model.to_string(),
        system_prompt: parent.and_then(|parent| parent.system_prompt.clone()),
        tools: parent.and_then(|parent| parent.tools.clone()).filter(|tools| !tools.is_empty()),
        tool_choice: Some(ToolChoice::none()),
        max_tokens,
        reasoning_effort: options.reasoning_effort,
        verbosity: options.verbosity,
        ..Default::default()
    }
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
///
/// Band requests carry only their band's messages (not the full history
/// prefix), so message-prefix reuse does not apply; the parent system/tools
/// prefix is still reused, and `tool_choice` stays `none` so neither pass can
/// spend the compaction budget on tool calls.
async fn summarize_locally_hierarchical(
    provider: &dyn LLMProvider,
    model: &str,
    history: &[Message],
    config: &CompactionConfig,
    options: &ManualCompactionOptions,
    context_budget: Option<usize>,
    parent: Option<&CompactionParentContext>,
) -> Result<Vec<Message>> {
    let effective_config = context_bounded_compaction_config(
        provider,
        model,
        history,
        &config.clone().with_manual_overrides(options),
        context_budget,
    );
    let (summary_history, _) =
        split_continuity_history_with_target(history, route_tail_target_tokens(provider, model, context_budget));

    // Split history into three bands at roughly equal thirds. Each band is
    // bounded to the summarizer window so a near-full context cannot overflow
    // the band request itself (same class of bug as the flat-path fork).
    // Oversized tool outputs are pruned first so large dumps do not evict
    // whole protocol groups from a band.
    let total = summary_history.len();
    let band_size = total / 3;
    let abstract_end = band_size;
    let detail_end = band_size * 2;
    let max_overflow_retries = CompactionRoutePolicy::resolve(provider.name(), model).max_overflow_retries;

    // Band 1 (oldest): compress into 1-2 sentence abstract.
    let abstract_budget =
        local_summary_history_budget(provider, model, context_budget, ABSTRACT_BAND_INTRO, Some(150), parent)?;
    // Validate both prefixes and bounded histories before spending either call.
    let detail_budget = local_summary_history_budget(
        provider,
        model,
        context_budget,
        &effective_config.summary_prompt,
        options.max_output_tokens,
        parent,
    )?;
    let pruned_abstract = prune_oversized_tool_outputs(&summary_history[..abstract_end]);
    let abstract_band = bound_local_summary_history(&pruned_abstract, ABSTRACT_BAND_INTRO, abstract_budget)?;
    let pruned_detail = prune_oversized_tool_outputs(&summary_history[abstract_end..detail_end]);
    let detail_band = bound_local_summary_history(&pruned_detail, &effective_config.summary_prompt, detail_budget)?;
    let abstract_summary = generate_summary_with_capacity_retry(
        provider,
        model,
        &pruned_abstract,
        ABSTRACT_BAND_INTRO,
        &abstract_band,
        abstract_budget,
        max_overflow_retries,
        "Failed to generate abstract summary",
        |band| {
            band_summary_request(
                model,
                format!("{ABSTRACT_BAND_INTRO}{}", build_summary_prompt(band, "")),
                Some(150),
                options,
                parent,
            )
        },
    )
    .await?;

    // Band 2 (middle): paragraph-level summary using the full summary prompt.
    let detail_summary = generate_summary_with_capacity_retry(
        provider,
        model,
        &pruned_detail,
        &effective_config.summary_prompt,
        &detail_band,
        detail_budget,
        max_overflow_retries,
        "Failed to generate detail summary",
        |band| {
            band_summary_request(
                model,
                build_summary_prompt(band, &effective_config.summary_prompt),
                options.max_output_tokens,
                options,
                parent,
            )
        },
    )
    .await?;

    // Band 3 (newest): retain verbatim via the bounded protocol tail.
    let recent_band = &summary_history[detail_end..];
    let retained = collect_retained_user_messages(
        recent_band,
        effective_config.retained_user_message_tokens,
        effective_config.retained_user_messages,
    );

    // Assemble: [abstract, detail, ...retained_recent, ...continuity_tail]
    let mut new_history = Vec::with_capacity(2 + retained.len());
    new_history.push(Message::system(format!("{ABSTRACT_PREFIX}{abstract_summary}")));
    new_history.push(Message::system(format!("{DETAIL_PREFIX}{detail_summary}")));
    new_history.extend(retained);
    // Live compaction: retain the most recent turn verbatim for continuity.
    for message in continuity_tail_with_target(history, route_tail_target_tokens(provider, model, context_budget)) {
        new_history.push(message.clone());
    }
    Ok(bound_compacted_history_to_context(new_history, provider, model, context_budget))
}
