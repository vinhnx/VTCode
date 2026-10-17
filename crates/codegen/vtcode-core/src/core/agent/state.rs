use crate::llm::provider::Message;
use hashbrown::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use vtcode_macros::StringNewtype;

// ============================================================================
// Context Manager: Call/Output Pairing Invariants (OpenAI Codex pattern)
// ============================================================================

/// Unique identifier for a tool call.
#[derive(Debug, Clone, PartialEq, Eq, Hash, StringNewtype)]
pub struct ToolCallId(String);

/// Status of a tool execution
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputStatus {
    Success,
    Failed,
    Canceled,
    Timeout,
}

impl OutputStatus {
    /// Convert to string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
            Self::Timeout => "timeout",
        }
    }
}

/// Items that participate in call/output pairing for validation
#[derive(Debug, Clone)]
pub enum PairableHistoryItem {
    /// Tool call without output (yet)
    ToolCall { call_id: ToolCallId, tool_name: String },
    /// Tool output for a previous call
    ToolOutput { call_id: ToolCallId, status: OutputStatus },
}

/// Record of a missing output in conversation history
#[derive(Debug, Clone)]
pub struct MissingOutput {
    pub call_id: ToolCallId,
    pub tool_name: String,
}

/// Validation report for conversation history state
#[derive(Debug, Default, Clone)]
pub struct HistoryValidationReport {
    /// Tool calls without corresponding outputs
    pub missing_outputs: Vec<MissingOutput>,
    /// Outputs without corresponding calls (orphans)
    pub orphan_outputs: Vec<ToolCallId>,
}

impl HistoryValidationReport {
    /// Check if history is in a valid state
    pub fn is_valid(&self) -> bool {
        self.missing_outputs.is_empty() && self.orphan_outputs.is_empty()
    }

    /// Get a human-readable summary
    pub fn summary(&self) -> String {
        if self.is_valid() {
            "History invariants are valid".to_string()
        } else {
            format!("{} missing outputs, {} orphan outputs", self.missing_outputs.len(), self.orphan_outputs.len())
        }
    }

    /// Filter out missing outputs that correspond to pending actions.
    ///
    /// A "missing output" is expected when the corresponding tool call is still
    /// in flight (pending action). Removing these from the report prevents
    /// false positives during crash recovery that would insert synthetic
    /// outputs for tools that are legitimately still running.
    pub fn exclude_pending(&mut self, is_pending: impl Fn(&str) -> bool) {
        self.missing_outputs.retain(|m| !is_pending(m.call_id.as_str()));
    }
}

#[cfg(test)]
#[inline]
pub(crate) fn record_turn_duration(
    turn_durations: &mut Vec<u128>,
    turn_total_ms: &mut u128,
    turn_max_ms: &mut u128,
    turn_count: &mut usize,
    recorded: &mut bool,
    start: &std::time::Instant,
) {
    if !*recorded {
        let duration_ms = start.elapsed().as_millis();
        turn_durations.push(duration_ms);
        *turn_total_ms += duration_ms;
        if duration_ms > *turn_max_ms {
            *turn_max_ms = duration_ms;
        }
        *turn_count += 1;
        *recorded = true;
    }
}

/// API failure tracking for exponential backoff
pub struct ApiFailureTracker {
    pub consecutive_failures: u32,
    pub last_failure: Option<std::time::Instant>,
}

impl Default for ApiFailureTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl ApiFailureTracker {
    pub fn new() -> Self {
        Self { consecutive_failures: 0, last_failure: None }
    }

    pub fn record_failure(&mut self) {
        self.consecutive_failures += 1;
        self.last_failure = Some(std::time::Instant::now());
    }

    pub fn reset(&mut self) {
        self.consecutive_failures = 0;
        self.last_failure = None;
    }

    pub fn should_circuit_break(&self) -> bool {
        self.consecutive_failures >= 3
    }

    pub fn backoff_duration(&self) -> Duration {
        let base_ms = 1000;
        let max_ms = 30000;
        let backoff_ms = base_ms * 2_u64.pow(self.consecutive_failures.saturating_sub(1));
        Duration::from_millis(backoff_ms.min(max_ms))
    }
}

pub fn summarize_list(items: &[String]) -> String {
    const MAX_ITEMS: usize = 5;
    if items.is_empty() {
        return "none".into();
    }
    let shown: Vec<&str> = items.iter().take(MAX_ITEMS).map(|s| s.as_str()).collect();
    if items.len() > MAX_ITEMS {
        format!("{} [+{} more]", shown.join(", "), items.len() - MAX_ITEMS)
    } else {
        shown.join(", ")
    }
}

// ============================================================================
// Standalone History Invariant Functions
// ============================================================================

/// Validate that conversation history maintains call/output invariants.
pub fn validate_history_invariants(messages: &[Message]) -> HistoryValidationReport {
    let mut call_map: HashMap<String, String> = HashMap::new();
    let mut output_ids: HashSet<String> = HashSet::new();

    // Scan messages to find tool calls and responses
    for msg in messages {
        // Tool calls: assistant messages with tool_calls field
        if let Some(tool_calls) = &msg.tool_calls {
            for tool_call in tool_calls {
                call_map.insert(tool_call.id.clone(), msg.role.to_string());
            }
        }

        // Tool responses: messages with tool_call_id set
        if let Some(tool_call_id) = &msg.tool_call_id {
            output_ids.insert(tool_call_id.clone());
        }
    }

    // Find missing outputs (calls without corresponding responses)
    let missing_outputs: Vec<_> = call_map
        .keys()
        .filter(|call_id| !output_ids.contains(*call_id))
        .map(|call_id| MissingOutput {
            call_id: ToolCallId::new(call_id.clone()),
            tool_name: "unknown".to_string(),
        })
        .collect();

    // Find orphan outputs (responses without matching calls)
    let orphan_outputs: Vec<_> = output_ids
        .iter()
        .filter(|output_id| !call_map.contains_key(*output_id))
        .map(|output_id| ToolCallId::new(output_id.clone()))
        .collect();

    HistoryValidationReport { missing_outputs, orphan_outputs }
}

const REQUEST_HISTORY_CANCELLATION_RESULT: &str =
    "canceled: no tool result was recorded; this bounded placeholder preserves the tool-call protocol.";

#[derive(Debug)]
struct RequestToolBatch {
    assistant_index: usize,
    matched_calls: Vec<bool>,
    result_indices: Vec<usize>,
}

#[derive(Debug, Default)]
struct RequestHistoryAnalysis {
    batches: Vec<RequestToolBatch>,
    batch_by_assistant_index: HashMap<usize, usize>,
    result_batch_by_index: HashMap<usize, usize>,
    invalid_result_indices: HashSet<usize>,
}

impl RequestHistoryAnalysis {
    fn needs_repair(&self) -> bool {
        if !self.invalid_result_indices.is_empty() {
            return true;
        }

        self.batches.iter().any(|batch| {
            batch.matched_calls.iter().any(|matched| !matched)
                || batch
                    .result_indices
                    .iter()
                    .enumerate()
                    .any(|(offset, &result_index)| result_index != batch.assistant_index + offset + 1)
        })
    }
}

fn analyze_request_history(messages: &[Message]) -> RequestHistoryAnalysis {
    let mut analysis = RequestHistoryAnalysis::default();
    let mut calls_by_id: HashMap<&str, Vec<(usize, usize)>> = HashMap::new();

    for (message_index, message) in messages.iter().enumerate() {
        if message.role != crate::llm::provider::MessageRole::Assistant {
            continue;
        }

        let Some(tool_calls) = message.tool_calls.as_ref().filter(|calls| !calls.is_empty()) else {
            continue;
        };

        let batch_index = analysis.batches.len();
        let matched_calls = vec![false; tool_calls.len()];
        analysis.batch_by_assistant_index.insert(message_index, batch_index);
        analysis.batches.push(RequestToolBatch {
            assistant_index: message_index,
            matched_calls,
            result_indices: Vec::new(),
        });

        for (call_index, call) in tool_calls.iter().enumerate() {
            calls_by_id.entry(call.id.as_str()).or_default().push((batch_index, call_index));
        }
    }

    for (message_index, message) in messages.iter().enumerate() {
        let has_tool_call_id = message.tool_call_id.is_some();
        let is_tool_result = message.role == crate::llm::provider::MessageRole::Tool;
        if !is_tool_result && !has_tool_call_id {
            continue;
        }

        let Some(tool_call_id) = message.tool_call_id.as_deref().filter(|id| !id.is_empty()) else {
            analysis.invalid_result_indices.insert(message_index);
            continue;
        };

        if !is_tool_result {
            analysis.invalid_result_indices.insert(message_index);
            continue;
        }

        let Some(candidates) = calls_by_id.get_mut(tool_call_id) else {
            analysis.invalid_result_indices.insert(message_index);
            continue;
        };

        let Some(&(batch_index, call_index)) = candidates.iter().rev().find(|&&(batch_index, call_index)| {
            analysis.batches[batch_index].assistant_index < message_index
                && !analysis.batches[batch_index].matched_calls[call_index]
        }) else {
            // This includes results that arrived before their call and duplicate
            // results after the call has already been matched.
            analysis.invalid_result_indices.insert(message_index);
            continue;
        };

        analysis.batches[batch_index].matched_calls[call_index] = true;
        analysis.batches[batch_index].result_indices.push(message_index);
        analysis.result_batch_by_index.insert(message_index, batch_index);
    }

    analysis
}

/// Return whether the provider-facing message view needs tool-history repair.
///
/// This deliberately analyzes only the request view. Durable session history
/// is repaired by its own crash-recovery path and is never changed here.
pub fn request_history_needs_normalization(messages: &[Message]) -> bool {
    analyze_request_history(messages).needs_repair()
}

/// Rebuild a shared provider-facing message view with each assistant
/// tool-call batch followed immediately by its matching results.
///
/// The rebuild is request-scoped and idempotent: orphaned, causally early, and
/// duplicate results are omitted; missing calls receive a bounded cancellation
/// result; and messages interleaved between a call batch and its results move
/// after the complete batch. A clean input is returned unchanged; repairs use
/// copy-on-write and never mutate the source history.
pub fn normalize_history_for_request_shared(messages: Arc<Vec<Message>>) -> Arc<Vec<Message>> {
    let analysis = analyze_request_history(&messages);
    if !analysis.needs_repair() {
        return messages;
    }

    let mut normalized = Vec::with_capacity(messages.len());
    for (message_index, message) in messages.iter().enumerate() {
        if analysis.invalid_result_indices.contains(&message_index)
            || analysis.result_batch_by_index.contains_key(&message_index)
        {
            continue;
        }

        let Some(&batch_index) = analysis.batch_by_assistant_index.get(&message_index) else {
            normalized.push(message.clone());
            continue;
        };

        let batch = &analysis.batches[batch_index];
        normalized.push(message.clone());
        for &result_index in &batch.result_indices {
            normalized.push(messages[result_index].clone());
        }
        for (call, matched) in message.tool_calls.iter().flatten().zip(&batch.matched_calls) {
            if !matched {
                normalized
                    .push(Message::tool_response(call.id.clone(), REQUEST_HISTORY_CANCELLATION_RESULT.to_owned()));
            }
        }
    }

    Arc::new(normalized)
}

/// Compatibility wrapper for callers that own a slice rather than shared
/// request history. The shared implementation avoids cloning clean message
/// histories; this wrapper retains the historical
/// owned return type for small slice-based callers and tests.
pub fn normalize_history_for_request(messages: &[Message]) -> Vec<Message> {
    Arc::unwrap_or_clone(normalize_history_for_request_shared(Arc::new(messages.to_vec())))
}

/// Placeholder body for a cleared tool result. Mirrors Anthropic
/// `clear_tool_uses` semantics as request-only shaping: the durable history
/// and `ThreadEvent` log keep the original payload.
const CLEARED_TOOL_RESULT_NOTE: &str = "Older tool result cleared to bound context growth. Full output remains in session logs; re-run the tool if raw bytes are needed.";

/// Cap local clearing at 60% of the history capacity left after the request
/// prefix and output reserve. Zero capacity is unknown; zero trigger or an
/// explicit opt-out keeps clearing disabled.
#[must_use]
pub fn resolve_tool_result_clearing_trigger(
    config: &crate::config::VTCodeConfig,
    provider: &dyn crate::llm::provider::LLMProvider,
    model: &str,
    prompt_overhead_tokens: usize,
    reserved_output_tokens: usize,
) -> u64 {
    let clearing = &config.agent.harness.tool_result_clearing;
    if !clearing.enabled || clearing.trigger_tokens == 0 {
        return 0;
    }
    let capacity = crate::compaction::effective_context_budget(Some(config), provider, model);
    if capacity == 0 {
        return clearing.trigger_tokens;
    }
    let history_budget = capacity
        .saturating_sub(reserved_output_tokens)
        .saturating_sub(prompt_overhead_tokens) as u64;
    // Divide before multiplying, retaining the remainder to avoid overflow
    // even at the largest representable context budget.
    let cap = (history_budget / 5)
        .saturating_mul(3)
        .saturating_add((history_budget % 5) * 3 / 5)
        .max(1);
    clearing.trigger_tokens.min(cap)
}

/// Request-only local stand-in for Anthropic `clear_tool_uses_20250919`.
///
/// Providers without `context_management.edits` never get native tool-result
/// clearing, so long histories keep paying full price for every old tool body
/// on each request. Once estimated history tokens exceed `trigger_tokens`, this
/// rewrites old tool-result *request* messages outside a bounded recent
/// evidence window and the newest `keep_tool_uses` originating batches to a
/// bounded stub. Small results stay intact when a stub would not save tokens.
/// `clear_at_least_tokens` is a floor on reclaimed tokens (not a ceiling):
/// clearing must stub all eligible non-retained results so
/// re-running on full durable history cannot leave a permanently growing tail.
///
/// Contract (same as [`normalize_history_for_request`]):
/// - never mutates durable session history or `ThreadEvent`s;
/// - preserves `role`, `tool_call_id`, `origin_tool`, and message order so
///   provider tool-pairing validation still passes;
/// - idempotent: re-running on already-stubbed output is a no-op below the
///   trigger and cannot re-clear stubs usefully.
///
/// When `clear_tool_inputs` is set, assistant `tool_calls[].function.arguments`
/// and freeform `text` larger than 1 KiB for cleared results are replaced with
/// a JSON placeholder. Small arguments retain usable command/path identity.
pub fn clear_old_tool_results(
    messages: &[Message],
    trigger_tokens: u64,
    keep_tool_uses: u32,
    clear_at_least_tokens: u64,
    clear_tool_inputs: bool,
) -> Vec<Message> {
    clear_old_tool_results_with_recent_batches(
        messages,
        trigger_tokens,
        keep_tool_uses,
        0,
        clear_at_least_tokens,
        clear_tool_inputs,
    )
}

/// Request-only clearing that also protects the newest assistant tool-call
/// batches by originating message order. Count-based protection is additive;
/// unmatched results retain the compatibility wrapper's count-based fallback.
pub fn clear_old_tool_results_with_recent_batches(
    messages: &[Message],
    trigger_tokens: u64,
    keep_tool_uses: u32,
    keep_recent_tool_batches: u32,
    clear_at_least_tokens: u64,
    clear_tool_inputs: bool,
) -> Vec<Message> {
    if trigger_tokens == 0 || messages.is_empty() {
        return messages.to_vec();
    }

    let estimated_tokens: u64 = messages
        .iter()
        .map(|message| message.estimate_tokens() as u64)
        .fold(0u64, u64::saturating_add);
    if estimated_tokens < trigger_tokens {
        return messages.to_vec();
    }

    let tool_indices: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, message)| message.role == crate::llm::provider::MessageRole::Tool)
        .map(|(index, _)| index)
        .collect();
    let keep = keep_tool_uses as usize;
    if tool_indices.len() <= keep {
        return messages.to_vec();
    }

    let mut cleared_tokens = 0u64;
    let analysis = analyze_request_history(messages);
    let mut retained_batches: HashSet<usize> = tool_indices
        .iter()
        .rev()
        .take(keep)
        .filter_map(|index| analysis.result_batch_by_index.get(index).copied())
        .collect();
    // Analysis records assistant messages in source order, independently of
    // completion order and reused call IDs.
    retained_batches.extend((0..analysis.batches.len()).rev().take(keep_recent_tool_batches as usize));
    // Two batches can be only two short searches. Clearing everything before
    // them discards the source needed for the pending edit, provoking rereads
    // that the navigation guards correctly bound. Reserve half of the
    // already capacity-capped trigger for recent complete evidence batches.
    // This window stays bounded when rebuilding from full durable history.
    let evidence_budget = trigger_tokens / 2;
    let mut batch_tokens: Vec<u64> = analysis
        .batches
        .iter()
        .map(|batch| messages[batch.assistant_index].estimate_tokens() as u64)
        .collect();
    for (&result_index, &batch_index) in &analysis.result_batch_by_index {
        batch_tokens[batch_index] =
            batch_tokens[batch_index].saturating_add(messages[result_index].estimate_tokens() as u64);
    }
    let mut retained_tokens = 0u64;
    for (batch_index, &tokens) in batch_tokens.iter().enumerate().rev() {
        let next_tokens = retained_tokens.saturating_add(tokens);
        if next_tokens > evidence_budget {
            break;
        }
        retained_tokens = next_tokens;
        retained_batches.insert(batch_index);
    }
    let mut cleared_result_count = 0usize;
    let mut cleared_calls_by_assistant: HashMap<usize, HashSet<&str>> = HashMap::new();
    // Stub every eligible non-kept tool result (oldest-first). `clear_at_least_tokens`
    // is a floor on reclaimed tokens; stopping early would leave a permanently
    // growing tail on the next request that re-shapes full durable history.
    let mut out = messages.to_vec();
    for &tool_index in &tool_indices[..tool_indices.len() - keep] {
        let batch_index = analysis.result_batch_by_index.get(&tool_index);
        if batch_index.is_some_and(|index| retained_batches.contains(index)) {
            continue;
        }
        let original = &messages[tool_index];
        let original_tokens = original.estimate_tokens() as u64;
        let stub = build_cleared_tool_result_stub(original);
        let stub_tokens = stub.estimate_tokens() as u64;
        if stub_tokens >= original_tokens {
            continue;
        }
        if let Some(&batch_index) = batch_index
            && let Some(call_id) = original.tool_call_id.as_deref()
        {
            cleared_calls_by_assistant
                .entry(analysis.batches[batch_index].assistant_index)
                .or_default()
                .insert(call_id);
        }
        out[tool_index] = stub;
        cleared_result_count += 1;
        cleared_tokens = cleared_tokens.saturating_add(original_tokens.saturating_sub(stub_tokens));
    }

    // The floor is advisory: every eligible result is already stubbed. A
    // protected batch or small results can leave less reclaimable than the
    // configured floor.
    if cleared_tokens < clear_at_least_tokens {
        tracing::debug!(
            cleared_tokens,
            clear_at_least_tokens,
            "tool-result clearing reclaimed less than the configured floor"
        );
    }

    if clear_tool_inputs {
        for (assistant_index, original_message) in out.iter_mut().enumerate() {
            let Some(cleared_call_ids) = cleared_calls_by_assistant.remove(&assistant_index) else {
                continue;
            };
            let mut message = original_message.clone();
            let Some(tool_calls) = message.tool_calls.as_mut() else {
                continue;
            };
            for call in tool_calls.iter_mut() {
                if cleared_call_ids.contains(call.id.as_str()) {
                    if let Some(function) = call.function.as_mut()
                        && function.arguments.len() > RETAINED_TOOL_INPUT_BYTES
                    {
                        // Valid JSON placeholder: providers send `arguments`
                        // verbatim; prose would break OpenAI tool-call parsing.
                        function.arguments = CLEARED_TOOL_INPUT_PLACEHOLDER.to_string();
                    }
                    // Freeform custom tool payloads ride in `text`, not
                    // `function.arguments` (see ToolCall::custom).
                    if call.text.as_ref().is_some_and(|text| text.len() > RETAINED_TOOL_INPUT_BYTES) {
                        call.text = Some(CLEARED_TOOL_INPUT_PLACEHOLDER.to_string());
                    }
                    call.thought_signature = None;
                }
            }
            // Tiny arguments can be cheaper than the JSON placeholder. Do
            // not spend more tokens on inputs than the results reclaimed.
            let input_growth =
                (message.estimate_tokens() as u64).saturating_sub(original_message.estimate_tokens() as u64);
            if input_growth <= cleared_tokens {
                *original_message = message;
                cleared_tokens = cleared_tokens.saturating_sub(input_growth);
            }
        }
    }

    tracing::debug!(
        original_tokens = estimated_tokens,
        projected_tokens = out
            .iter()
            .map(|message| message.estimate_tokens() as u64)
            .fold(0u64, u64::saturating_add),
        cleared_result_count,
        protected_batch_count = retained_batches.len(),
        "local tool-result clearing projection"
    );
    out
}

/// JSON placeholder for cleared tool-call inputs (must stay valid JSON).
const CLEARED_TOOL_INPUT_PLACEHOLDER: &str = "{\"cleared\":\"tool_input\"}";
// Keep ordinary command/path arguments intact instead of teaching providers
// to emit synthetic arguments that cannot pass the tool's declared schema.
const RETAINED_TOOL_INPUT_BYTES: usize = 1024;

/// Whether request assembly should apply local [`clear_old_tool_results`].
///
/// Local clearing and Anthropic `clear_tool_uses` are mutually exclusive:
/// native edits already reclaim tool bodies on the wire, so applying both
/// would double-shape the same history.
pub fn should_apply_local_tool_result_clearing(
    provider_name: &str,
    context_edits: bool,
    clearing_enabled: bool,
) -> bool {
    clearing_enabled && !(provider_name.eq_ignore_ascii_case("anthropic") && context_edits)
}

fn build_cleared_tool_result_stub(original: &Message) -> Message {
    let mut stub = original.clone();
    // serde_json (not `{:?}`) so non-ASCII tool names / call ids stay valid JSON.
    let body = serde_json::json!({
        "cleared": "tool_result",
        "reason": "tool_result_clearing",
        "note": CLEARED_TOOL_RESULT_NOTE,
        "tool": original.origin_tool.as_deref().unwrap_or("tool"),
        "tool_call_id": original.tool_call_id.as_deref().unwrap_or(""),
    })
    .to_string();
    stub.content = crate::llm::provider::MessageContent::Text(body);
    stub
}

/// Find a split point that keeps tool-call outputs paired with their calls.
pub fn safe_history_split_point(messages: &[Message], conversation_len: usize, preferred_split_at: usize) -> usize {
    if preferred_split_at == 0 || preferred_split_at >= conversation_len {
        return preferred_split_at;
    }

    let mut call_indices: HashMap<&str, usize> = HashMap::new();
    for (i, msg) in messages.iter().enumerate() {
        if let Some(tool_calls) = &msg.tool_calls {
            for call in tool_calls {
                call_indices.insert(&call.id, i);
            }
        }
    }

    let mut safe_split_at = preferred_split_at;
    loop {
        if safe_split_at == 0 {
            break;
        }

        let has_orphan = ((safe_split_at + 1)..messages.len()).any(|i| {
            messages
                .get(i)
                .and_then(|msg| msg.tool_call_id.as_ref())
                .and_then(|id| call_indices.get(id.as_str()))
                .is_some_and(|&call_idx| call_idx <= safe_split_at)
        });

        if !has_orphan {
            break;
        }

        safe_split_at -= 1;
    }

    safe_split_at
}

/// Ensure all tool calls have corresponding outputs in the message list.
pub fn ensure_call_outputs_present(messages: &mut Vec<Message>) {
    let report = validate_history_invariants(messages);

    // Create synthetic outputs for missing calls in reverse order to avoid index shifting
    for missing in report.missing_outputs.iter().rev() {
        let synthetic_message = Message::tool_response(
            missing.call_id.as_str().to_string(),
            "canceled: Tool execution was interrupted. This synthetic output was created \
             during history normalization to maintain conversation invariants."
                .to_string(),
        );

        tracing::warn!("Creating synthetic output for call {} due to missing execution result", missing.call_id);

        // Find the position to insert: right after the corresponding call
        let insert_pos = messages
            .iter()
            .position(|msg| {
                msg.tool_calls
                    .as_ref()
                    .is_some_and(|calls| calls.iter().any(|call| call.id == missing.call_id.as_str()))
            })
            .map(|pos| pos + 1);

        if let Some(pos) = insert_pos {
            messages.insert(pos, synthetic_message);
        } else {
            // If we can't find the call, just append the synthetic output
            messages.push(synthetic_message);
        }
    }
}

/// Remove outputs without corresponding calls (orphaned outputs) from the message list.
pub fn remove_orphan_outputs(messages: &mut Vec<Message>) {
    let report = validate_history_invariants(messages);

    if report.orphan_outputs.is_empty() {
        return;
    }

    let orphan_ids: HashSet<String> = report.orphan_outputs.iter().map(|id| id.as_str().to_string()).collect();

    let initial_len = messages.len();

    // Retain only messages that either:
    // - Don't have a tool_call_id (not a tool response)
    // - Have a tool_call_id that matches an existing call
    messages.retain(|msg| {
        if let Some(tool_call_id) = msg.tool_call_id.as_ref()
            && orphan_ids.contains(tool_call_id)
        {
            tracing::warn!("Removing orphan output for call {}", tool_call_id);
            return false;
        }
        true
    });

    if messages.len() != initial_len {
        tracing::info!("Removed {} orphan outputs", initial_len - messages.len());
    }
}

/// Normalize history to enforce call/output pairing invariants.
pub fn normalize_history(messages: &mut Vec<Message>) {
    ensure_call_outputs_present(messages);
    remove_orphan_outputs(messages);

    // Log if issues were found
    let report = validate_history_invariants(messages);
    if !report.is_valid() {
        tracing::warn!("History validation: {}", report.summary());
    } else {
        tracing::debug!("History normalized successfully");
    }
}

/// Recover from crashed or interrupted session by fixing history invariants.
pub fn recover_history_from_crash(messages: &mut Vec<Message>) {
    let report = validate_history_invariants(messages);

    if !report.missing_outputs.is_empty() {
        tracing::warn!("Found {} missing outputs during recovery", report.missing_outputs.len());
        ensure_call_outputs_present(messages);
    }

    if !report.orphan_outputs.is_empty() {
        tracing::warn!("Found {} orphan outputs during recovery", report.orphan_outputs.len());
        remove_orphan_outputs(messages);
    }

    if report.is_valid() {
        tracing::debug!("History invariants are valid");
    }
}

// ============================================================================
// Tests: Context Manager - Call/Output Pairing Invariants
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::provider::{Message, MessageRole};
    /// Helper: Create test messages
    fn make_tool_call(call_id: &str, tool_name: &str) -> Message {
        Message::assistant_with_tools(
            "".to_string(),
            vec![crate::llm::provider::ToolCall::function(
                call_id.to_string(),
                tool_name.to_string(),
                "{}".to_string(),
            )],
        )
    }

    fn make_tool_response(call_id: &str, content: &str) -> Message {
        Message::tool_response(call_id.to_string(), content.to_string())
    }

    /// Test: Valid history with matched calls and outputs
    #[test]
    fn test_validate_history_valid_matched_pairs() {
        let mut messages = vec![
            make_tool_call("call_1", "list_files"),
            make_tool_response("call_1", "file1.rs\nfile2.rs"),
        ];

        let report = validate_history_invariants(&messages);
        assert!(report.is_valid(), "Valid paired call/output should pass");
        assert!(report.missing_outputs.is_empty());
        assert!(report.orphan_outputs.is_empty());

        // Normalize should be a no-op
        normalize_history(&mut messages);
        assert_eq!(messages.len(), 2);
    }

    /// Test: Missing output (tool call without response)
    #[test]
    fn test_validate_history_missing_output() {
        let messages = vec![make_tool_call("call_1", "list_files")];

        let report = validate_history_invariants(&messages);
        assert!(!report.is_valid());
        assert_eq!(report.missing_outputs.len(), 1);
        assert_eq!(report.missing_outputs[0].call_id.as_str(), "call_1");
        assert!(report.orphan_outputs.is_empty());
    }

    /// Test: Orphan output (response without corresponding call)
    #[test]
    fn test_validate_history_orphan_output() {
        let messages = vec![make_tool_response("orphan_call", "Some result")];

        let report = validate_history_invariants(&messages);
        assert!(!report.is_valid());
        assert!(report.missing_outputs.is_empty());
        assert_eq!(report.orphan_outputs.len(), 1);
        assert_eq!(report.orphan_outputs[0].as_str(), "orphan_call");
    }

    /// Test: ensure_call_outputs_present creates synthetic outputs
    #[test]
    fn test_ensure_call_outputs_present() {
        let mut messages = vec![make_tool_call("call_1", "list_files")];
        let initial_len = messages.len();

        ensure_call_outputs_present(&mut messages);

        assert_eq!(messages.len(), initial_len + 1);
        let last_msg = &messages[initial_len];
        assert_eq!(last_msg.tool_call_id, Some("call_1".to_string()));
        assert!(last_msg.content.as_text().contains("canceled"));

        let report = validate_history_invariants(&messages);
        assert!(report.is_valid());
    }

    /// Test: remove_orphan_outputs filters out orphaned responses
    #[test]
    fn test_remove_orphan_outputs() {
        let mut messages = vec![
            make_tool_call("call_1", "list_files"),
            make_tool_response("call_1", "valid result"),
            make_tool_response("orphan_call", "orphan result"),
        ];

        let initial_len = messages.len();
        remove_orphan_outputs(&mut messages);

        assert_eq!(messages.len(), initial_len - 1);
        assert!(
            messages
                .iter()
                .any(|msg| msg.tool_call_id.as_ref().is_some_and(|id| id == "call_1"))
        );
        assert!(
            !messages
                .iter()
                .any(|msg| { msg.tool_call_id.as_ref().is_some_and(|id| id == "orphan_call") })
        );

        let report = validate_history_invariants(&messages);
        assert!(report.is_valid());
    }

    /// Test: normalize() applies both fixes (synthetic output + orphan removal)
    #[test]
    fn test_normalize_combined_fixes() {
        let mut messages = vec![
            make_tool_call("call_1", "read_file"),
            make_tool_call("call_2", "write_file"),
            make_tool_response("call_2", "written"),
            make_tool_response("orphan", "orphan result"),
        ];

        normalize_history(&mut messages);

        let report = validate_history_invariants(&messages);
        assert!(report.is_valid());
        assert!(
            messages
                .iter()
                .any(|msg| msg.tool_call_id.as_ref().is_some_and(|id| id == "call_1"))
        );
        assert!(
            !messages
                .iter()
                .any(|msg| msg.tool_call_id.as_ref().is_some_and(|id| id == "orphan"))
        );
    }

    #[test]
    fn request_normalization_shared_reuses_clean_history_arc() {
        let messages = Arc::new(vec![Message::user("clean history".to_string())]);

        let normalized = normalize_history_for_request_shared(Arc::clone(&messages));

        assert!(Arc::ptr_eq(&messages, &normalized));
    }

    #[test]
    fn request_normalization_reused_ids_match_causally_preceding_batches() {
        let messages = Arc::new(vec![
            make_tool_response("reused", "early result"),
            make_tool_call("reused", "read_file"),
            make_tool_response("reused", "first result"),
            make_tool_call("reused", "write_file"),
            Message::user("interleaved".to_string()),
            make_tool_response("reused", "second result"),
            make_tool_response("reused", "duplicate result"),
        ]);
        let normalized = normalize_history_for_request_shared(Arc::clone(&messages));
        assert_eq!(
            normalized.as_ref(),
            &vec![
                make_tool_call("reused", "read_file"),
                make_tool_response("reused", "first result"),
                make_tool_call("reused", "write_file"),
                make_tool_response("reused", "second result"),
                Message::user("interleaved".to_string()),
            ]
        );
        assert_eq!(messages.len(), 7);
        assert_eq!(messages[0].content.as_text(), "early result");
    }

    #[test]
    fn request_normalization_shared_repairs_asymmetric_batch_without_mutating_source() {
        let messages = Arc::new(vec![
            Message::assistant_with_tools(
                "three calls".to_string(),
                ["call_a", "call_b", "call_c"]
                    .into_iter()
                    .map(|id| {
                        crate::llm::provider::ToolCall::function(
                            id.to_string(),
                            "read_file".to_string(),
                            "{}".to_string(),
                        )
                    })
                    .collect(),
            ),
            Message::user("interleaved request".to_string()),
            make_tool_response("call_b", "result B"),
            make_tool_response("call_a", "result A"),
            make_tool_response("call_b", "duplicate B"),
        ]);
        let source_before = messages.as_ref().clone();

        let normalized = normalize_history_for_request_shared(Arc::clone(&messages));

        assert!(!Arc::ptr_eq(&messages, &normalized));
        assert_eq!(normalized.len(), 5);
        assert_eq!(normalized[0], source_before[0]);
        assert_eq!(normalized[1], make_tool_response("call_b", "result B"));
        assert_eq!(normalized[2], make_tool_response("call_a", "result A"));
        assert_eq!(normalized[3].role, MessageRole::Tool);
        assert_eq!(normalized[3].tool_call_id.as_deref(), Some("call_c"));
        assert_eq!(
            normalized[3].content.as_text(),
            "canceled: no tool result was recorded; this bounded placeholder preserves the tool-call protocol."
        );
        assert_eq!(normalized[4], Message::user("interleaved request".to_string()));
        assert_eq!(messages.as_ref(), &source_before);
        let repeated = normalize_history_for_request_shared(Arc::clone(&normalized));
        assert!(Arc::ptr_eq(&normalized, &repeated));
    }

    #[test]
    fn request_normalization_groups_split_results_after_intervening_messages() {
        let messages = vec![
            Message::assistant_with_tools(
                "".to_string(),
                vec![
                    crate::llm::provider::ToolCall::function(
                        "call_1".to_string(),
                        "read_file".to_string(),
                        "{}".to_string(),
                    ),
                    crate::llm::provider::ToolCall::function(
                        "call_2".to_string(),
                        "read_file".to_string(),
                        "{}".to_string(),
                    ),
                ],
            ),
            Message::system("intervening system note".to_string()),
            make_tool_response("call_2", "result two"),
            Message::user("intervening user note".to_string()),
            make_tool_response("call_1", "result one"),
            Message::assistant("done".to_string()),
        ];

        let normalized = normalize_history_for_request(&messages);

        assert_eq!(normalized[0].role, MessageRole::Assistant);
        assert_eq!(normalized[1].tool_call_id.as_deref(), Some("call_2"));
        assert_eq!(normalized[1].content.as_text(), "result two");
        assert_eq!(normalized[2].tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(normalized[2].content.as_text(), "result one");
        assert_eq!(normalized[3].role, MessageRole::System);
        assert_eq!(normalized[4].role, MessageRole::User);
        assert_eq!(normalized[5].role, MessageRole::Assistant);
    }

    #[test]
    fn request_normalization_drops_early_orphan_duplicate_and_synthesizes_missing_results() {
        let messages = vec![
            make_tool_response("call_1", "causally early"),
            make_tool_response("orphan", "orphaned"),
            make_tool_call("call_1", "read_file"),
            make_tool_call("call_2", "read_file"),
            make_tool_response("call_1", "valid"),
            make_tool_response("call_1", "duplicate"),
        ];

        let normalized = normalize_history_for_request(&messages);

        assert_eq!(normalized.len(), 4);
        assert_eq!(normalized[0].role, MessageRole::Assistant);
        assert_eq!(normalized[0].tool_calls.as_ref().map(Vec::len), Some(1));
        assert_eq!(normalized[1].tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(normalized[1].content.as_text(), "valid");
        assert_eq!(normalized[2].role, MessageRole::Assistant);
        assert_eq!(normalized[3].tool_call_id.as_deref(), Some("call_2"));
        assert!(normalized[3].content.as_text().contains("bounded placeholder"));
    }

    #[test]
    fn request_normalization_is_idempotent_and_does_not_mutate_durable_history() {
        let messages = vec![
            make_tool_call("call_1", "read_file"),
            Message::system("intervening".to_string()),
            make_tool_response("call_1", "result"),
        ];
        let durable_before = messages.clone();

        assert!(request_history_needs_normalization(&messages));
        let normalized = normalize_history_for_request(&messages);
        assert!(!request_history_needs_normalization(&normalized));
        assert_eq!(normalize_history_for_request(&normalized), normalized);
        assert_eq!(messages, durable_before);
    }

    /// Test: recover_history_from_crash handles both missing and orphan outputs
    #[test]
    fn test_recover_from_crash() {
        let mut messages = vec![
            make_tool_call("crashed_call", "dangerous_op"),
            make_tool_response("old_call", "stale result"),
        ];

        recover_history_from_crash(&mut messages);

        let report = validate_history_invariants(&messages);
        assert!(report.is_valid());
        assert!(
            messages
                .iter()
                .any(|msg| { msg.tool_call_id.as_ref().is_some_and(|id| id == "crashed_call") })
        );
        assert!(
            !messages
                .iter()
                .any(|msg| msg.tool_call_id.as_ref().is_some_and(|id| id == "old_call"))
        );
    }

    /// Test: HistoryValidationReport summary messages
    #[test]
    fn test_validation_report_summary() {
        let valid = HistoryValidationReport::default();
        assert_eq!(valid.summary(), "History invariants are valid");
        assert!(valid.is_valid());

        let invalid = HistoryValidationReport {
            missing_outputs: vec![
                MissingOutput {
                    call_id: ToolCallId::new("call_1"),
                    tool_name: "tool_a".into(),
                },
                MissingOutput {
                    call_id: ToolCallId::new("call_2"),
                    tool_name: "tool_b".into(),
                },
            ],
            orphan_outputs: vec![ToolCallId::new("orphan_1")],
        };
        assert_eq!(invalid.summary(), "2 missing outputs, 1 orphan outputs");
        assert!(!invalid.is_valid());
    }

    /// Test: Multiple tool calls with selective missing outputs
    #[test]
    fn test_multiple_calls_partial_outputs() {
        let _messages: Vec<Message> = (1..=3)
            .flat_map(|i| {
                vec![
                    make_tool_call(&format!("call_{i}"), &format!("tool_{i}")),
                    if i != 2 {
                        make_tool_response(&format!("call_{i}"), &format!("result_{i}"))
                    } else {
                        // Simulate a gap: we don't add a response for call_2 here directly,
                        // but we need to build messages differently.
                        // Instead, build manually below.
                        Message::tool_response("placeholder".into(), "".into())
                    },
                ]
            })
            .collect();
        // Redo: explicit construction
        let mut messages = vec![
            make_tool_call("call_1", "tool_1"),
            make_tool_response("call_1", "result_1"),
            make_tool_call("call_2", "tool_2"),
            make_tool_call("call_3", "tool_3"),
            make_tool_response("call_3", "result_3"),
        ];

        let report = validate_history_invariants(&messages);
        assert!(!report.is_valid());
        assert_eq!(report.missing_outputs.len(), 1);
        assert_eq!(report.missing_outputs[0].call_id.as_str(), "call_2");

        normalize_history(&mut messages);
        assert!(validate_history_invariants(&messages).is_valid());
    }

    /// Test: OutputStatus enum conversion
    #[test]
    fn test_output_status_as_str() {
        assert_eq!(OutputStatus::Success.as_str(), "success");
        assert_eq!(OutputStatus::Failed.as_str(), "failed");
        assert_eq!(OutputStatus::Canceled.as_str(), "canceled");
        assert_eq!(OutputStatus::Timeout.as_str(), "timeout");
    }

    /// Test: find_safe_split_point maintains call/output pairs
    #[test]
    fn test_find_safe_split_point() {
        let messages = vec![
            Message::user("User 1".into()),           // 0
            make_tool_call("call_a", "tool_a"),       // 1
            make_tool_response("call_a", "Result A"), // 2
            make_tool_call("call_b", "tool_b"),       // 3
            make_tool_response("call_b", "Result B"), // 4
        ];
        let conversation_len = 5;

        // Split at 3 means keeping 3,4. But response at 2 needs call at 1 -> must split at 2.
        let safe = safe_history_split_point(&messages, conversation_len, 3);
        assert_eq!(safe, 2, "Should move split to include Call A");

        // Split at 4 is safe: call_b (3) and response_b (4) are both kept.
        let safe2 = safe_history_split_point(&messages, conversation_len, 4);
        assert_eq!(safe2, 4, "Should stay at 4 as it is safe");
    }

    #[test]
    fn test_summarize_list_formatting() {
        assert_eq!(summarize_list(&[]), "none");
        assert_eq!(summarize_list(&["a".into()]), "a");
        assert_eq!(summarize_list(&["a".into(), "b".into()]), "a, b");
        let many: Vec<String> = (1..=7).map(|i| format!("item{i}")).collect();
        let result = summarize_list(&many);
        assert!(result.contains("item1, item2, item3, item4, item5"));
        assert!(result.contains("[+2 more]"));
    }

    fn bulky_tool_history(count: usize, body_chars: usize) -> Vec<Message> {
        let mut messages = vec![Message::user("start".to_string())];
        for i in 0..count {
            let call_id = format!("call_{i}");
            messages.push(make_tool_call(&call_id, "read_file"));
            messages.push(make_tool_response(&call_id, &"x".repeat(body_chars)));
        }
        messages.push(Message::assistant("done".to_string()));
        messages
    }

    fn is_cleared_stub(message: &Message) -> bool {
        message.content.as_text().contains("\"cleared\":\"tool_result\"")
    }

    #[test]
    fn clear_old_tool_results_noop_below_trigger() {
        let messages = bulky_tool_history(3, 200);
        let cleared = clear_old_tool_results(&messages, u64::MAX, 1, 1, false);
        assert_eq!(cleared.len(), messages.len());
        assert!(
            cleared
                .iter()
                .filter(|m| m.role == MessageRole::Tool)
                .all(|m| !is_cleared_stub(m))
        );
    }

    #[test]
    fn clear_old_tool_results_keeps_newest_and_stubs_all_older() {
        // Four large tool results; keep 1. Every non-kept result is stubbed so
        // re-shaping full durable history cannot leave a growing tail.
        let messages = bulky_tool_history(4, 4_000);
        let cleared = clear_old_tool_results(&messages, 1, 1, 1, false);
        let tool_msgs: Vec<&Message> = cleared.iter().filter(|m| m.role == MessageRole::Tool).collect();
        assert_eq!(tool_msgs.len(), 4);
        assert!(is_cleared_stub(tool_msgs[0]), "oldest result is cleared");
        assert!(is_cleared_stub(tool_msgs[1]));
        assert!(is_cleared_stub(tool_msgs[2]));
        assert!(!is_cleared_stub(tool_msgs[3]), "newest result stays intact");
        // Protocol pairing survives.
        for (original, rewritten) in messages.iter().zip(cleared.iter()) {
            assert_eq!(original.role, rewritten.role);
            assert_eq!(original.tool_call_id, rewritten.tool_call_id);
        }
    }

    #[test]
    fn clear_old_tool_results_respects_keep_tool_uses() {
        let messages = bulky_tool_history(5, 3_000);
        let cleared = clear_old_tool_results(&messages, 1, 3, 1, false);
        let tool_msgs: Vec<&Message> = cleared.iter().filter(|m| m.role == MessageRole::Tool).collect();
        assert_eq!(tool_msgs.len(), 5);
        assert!(!is_cleared_stub(tool_msgs[2]));
        assert!(!is_cleared_stub(tool_msgs[3]));
        assert!(!is_cleared_stub(tool_msgs[4]));
    }

    #[test]
    fn clear_old_tool_results_optional_clear_tool_inputs() {
        let mut messages = bulky_tool_history(3, 3_000);
        // Attach a recognizable argument payload on the first call.
        if let Some(call) = messages
            .get_mut(1)
            .and_then(|m| m.tool_calls.as_mut())
            .and_then(|calls| calls.first_mut())
            .and_then(|call| call.function.as_mut())
        {
            call.arguments = serde_json::json!({"path": "secret-path", "padding": "x".repeat(1024)}).to_string();
        }

        let cleared = clear_old_tool_results(&messages, 1, 1, 1, true);
        let first_call_args = cleared
            .get(1)
            .and_then(|m| m.tool_calls.as_ref())
            .and_then(|calls| calls.first())
            .and_then(|call| call.function.as_ref())
            .map(|function| function.arguments.as_str())
            .unwrap_or_default();
        assert!(!first_call_args.contains("secret-path"), "cleared call arguments must drop the original payload");
        assert!(
            first_call_args.contains("\"cleared\":\"tool_input\""),
            "cleared arguments must be a JSON placeholder, got {first_call_args:?}"
        );
        assert!(
            serde_json::from_str::<serde_json::Value>(first_call_args).is_ok(),
            "cleared arguments must stay valid JSON for the wire"
        );
        // Newest retained call keeps its arguments.
        let last_call_args = cleared
            .get(5)
            .and_then(|m| m.tool_calls.as_ref())
            .and_then(|calls| calls.first())
            .and_then(|call| call.function.as_ref())
            .map(|function| function.arguments.as_str())
            .unwrap_or_default();
        assert_eq!(last_call_args, "{}");
    }

    #[test]
    fn clear_old_tool_results_default_config_clears_paired_inputs() {
        // Regression: `clear_tool_inputs` now defaults to true, so the common
        // request-shaping path must drop stale apply_patch/write_file bodies
        // together with the stubbed results.
        let default_clear_tool_inputs =
            vtcode_config::core::agent::ToolResultClearingConfig::default().clear_tool_inputs;
        assert!(default_clear_tool_inputs, "config default must clear tool inputs");

        let mut messages = bulky_tool_history(3, 3_000);
        if let Some(call) = messages
            .get_mut(1)
            .and_then(|m| m.tool_calls.as_mut())
            .and_then(|calls| calls.first_mut())
            .and_then(|call| call.function.as_mut())
        {
            call.arguments =
                serde_json::json!({"input": format!("*** Begin Patch\n{}\n*** End Patch", "x".repeat(1024))})
                    .to_string();
        }

        let cleared = clear_old_tool_results(&messages, 1, 1, 1, default_clear_tool_inputs);
        let cleared_args = cleared
            .get(1)
            .and_then(|m| m.tool_calls.as_ref())
            .and_then(|calls| calls.first())
            .and_then(|call| call.function.as_ref())
            .map(|function| function.arguments.as_str())
            .unwrap_or_default();
        assert!(
            !cleared_args.contains("Begin Patch"),
            "default-on clearing must drop patch bodies, got {cleared_args:?}"
        );
        assert_eq!(cleared_args, CLEARED_TOOL_INPUT_PLACEHOLDER);

        let kept_args = cleared
            .get(5)
            .and_then(|m| m.tool_calls.as_ref())
            .and_then(|calls| calls.first())
            .and_then(|call| call.function.as_ref())
            .map(|function| function.arguments.as_str())
            .unwrap_or_default();
        assert_eq!(kept_args, "{}", "kept call arguments must stay intact");
    }

    #[test]
    fn clear_old_tool_results_is_idempotent_on_stubs() {
        let messages = bulky_tool_history(4, 3_000);
        let once = clear_old_tool_results(&messages, 1, 1, 1_000, false);
        let twice = clear_old_tool_results(&once, 1, 1, 1_000, false);
        let stubs_once = once.iter().filter(|m| is_cleared_stub(m)).count();
        let stubs_twice = twice.iter().filter(|m| is_cleared_stub(m)).count();
        assert_eq!(stubs_once, stubs_twice);
    }

    #[test]
    fn clear_old_tool_results_leaves_durable_history_unchanged() {
        let messages = bulky_tool_history(3, 2_500);
        let snapshot: Vec<String> = messages.iter().map(|m| m.content.as_text().into_owned()).collect();
        let request_messages = clear_old_tool_results(&messages, 1, 1, 1, false);
        let after: Vec<String> = messages.iter().map(|m| m.content.as_text().into_owned()).collect();
        assert_eq!(snapshot, after, "durable history must not be mutated");
        assert!(
            request_messages
                .iter()
                .filter(|m| m.role == MessageRole::Tool)
                .any(is_cleared_stub),
            "request messages should carry stubs"
        );
    }

    #[test]
    fn local_tool_result_clearing_gate_mirrors_native_edits() {
        use super::should_apply_local_tool_result_clearing as gate;
        assert!(gate("zai", false, true), "non-Anthropic uses local clearing");
        assert!(gate("openai", false, true), "OpenAI uses local clearing");
        assert!(gate("anthropic", false, true), "Anthropic without edits falls back to local");
        assert!(!gate("anthropic", true, true), "Anthropic with context_edits uses native clear_tool_uses only");
        assert!(!gate("zai", false, false), "disabled config never clears");
    }

    #[test]
    fn clear_old_tool_results_stub_is_valid_json_for_non_ascii_tool_names() {
        let mut messages = bulky_tool_history(3, 2_500);
        // Index 2 is the first tool response (call_0).
        messages[2].origin_tool = Some("读取文件".to_string());
        messages[2].tool_call_id = Some("call_ünïcode".to_string());
        let cleared = clear_old_tool_results(&messages, 1, 1, 1, false);
        let stub = cleared
            .iter()
            .filter(|m| is_cleared_stub(m))
            .find(|m| m.content.as_text().contains("call_ünïcode"))
            .expect("stub for the renamed call");
        let parsed: serde_json::Value =
            serde_json::from_str(stub.content.as_text().as_ref()).expect("stub must be valid JSON");
        assert_eq!(parsed["tool"], "读取文件");
        assert_eq!(parsed["tool_call_id"], "call_ünïcode");
    }

    #[test]
    fn clear_old_tool_results_clear_tool_inputs_covers_freeform_text() {
        let mut messages = bulky_tool_history(3, 2_500);
        // Freeform custom tool payload lives in `text`, not function.arguments.
        if let Some(call) = messages
            .get_mut(1)
            .and_then(|m| m.tool_calls.as_mut())
            .and_then(|calls| calls.first_mut())
        {
            call.text = Some("raw freeform payload".repeat(100));
            call.thought_signature = Some("sig".to_string());
        }
        let cleared = clear_old_tool_results(&messages, 1, 1, 1, true);
        let call = cleared
            .get(1)
            .and_then(|m| m.tool_calls.as_ref())
            .and_then(|calls| calls.first())
            .expect("call preserved");
        assert_eq!(call.text.as_deref(), Some(CLEARED_TOOL_INPUT_PLACEHOLDER));
        assert!(call.thought_signature.is_none());
    }
}

#[cfg(test)]
mod tool_result_clearing_tests;
