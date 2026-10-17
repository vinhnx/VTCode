//! Responses history, stable instructions, and explicit cache breakpoints.

use super::ResponsesRequestContext;
use super::model_policy::{
    is_gpt5_codex_model, is_gpt6_model, is_gpt56_model, is_openai_gpt_responses_model, supports_assistant_phase_replay,
};
use crate::provider;
use crate::providers::openai::responses_api::build_standard_responses_payload;
use crate::providers::openai::types::{InstructionSegmentKind, OpenAIResponsesPayload};
use crate::providers::shared::split_dynamic_prompt_suffix;
use crate::system_prompt::{default_system_prompt, openai_gpt6_contract_addendum, openai_gpt56_contract_addendum};
use serde_json::{Value, json};

/// Trailing reminder label for relocated volatile prompt content.
///
/// Constant bytes keep the reminder tail cheap to reprocess; the label itself
/// never varies across turns.
const DYNAMIC_RUNTIME_REMINDER_LABEL: &str = "[System reminder — runtime context, not a user request]";

fn strip_non_native_assistant_phase(input: &mut [Value]) {
    for item in input {
        if let Some(map) = item.as_object_mut() {
            map.remove("phase");
        }
    }
}

fn default_replay_instructions(model: &str) -> Option<String> {
    if is_gpt5_codex_model(model) {
        Some(format!("You are Codex, based on GPT-5. {}", default_system_prompt()))
    } else if is_gpt56_model(model)
        || is_gpt6_model(model)
        || vtcode_config::models::model_catalog_entry("openai", model)
            .is_some_and(|entry| entry.prompt_contract.is_some())
    {
        Some(default_system_prompt())
    } else {
        None
    }
}

pub(super) fn augment_openai_instructions(model: &str, instructions: String) -> String {
    let addendum =
        match vtcode_config::models::model_catalog_entry("openai", model).and_then(|entry| entry.prompt_contract) {
            Some("gpt6") => Some(openai_gpt6_contract_addendum()),
            Some("gpt56") => Some(openai_gpt56_contract_addendum()),
            _ if is_gpt6_model(model) => Some(openai_gpt6_contract_addendum()),
            _ if is_gpt56_model(model) => Some(openai_gpt56_contract_addendum()),
            _ => None,
        };

    let Some(addendum) = addendum else {
        return instructions;
    };
    let trimmed_addendum = addendum.trim();
    if instructions.contains(trimmed_addendum) {
        instructions
    } else if instructions.trim().is_empty() {
        addendum
    } else {
        format!("{instructions}\n\n{addendum}")
    }
}

/// Retained custom Responses item/history boundary.
///
/// Rig 0.40 has typed Responses input items, but does not preserve VTCode's
/// assistant `phase` replay field, open-ended include strings such as
/// `output_text.annotations`, synthetic missing tool outputs, or the exact
/// instruction fallback used for ChatGPT replay parity. Protective tests:
/// `api_key_and_chatgpt_subscription_share_responses_item_history_builder`,
/// `openai_request_builder_preserves_custom_include_strings_around_typed_include`,
/// and the `responses_api` history tests. Remove this boundary once Rig exposes
/// open custom include values and VTCode-compatible structured history hooks.
fn is_explicit_cache_breakpoint_model(model: &str) -> bool {
    // GPT-5.6+ (and GPT-6 family) support explicit `prompt_cache_breakpoint`
    // markers. Older models (GPT-5.5 and earlier) reject `prompt_cache_options`
    // and `prompt_cache_breakpoint` outright, so never emit markers for them.
    super::supports_prompt_cache_options(model)
}

fn is_eligible_cache_breakpoint_block(block: &Value) -> bool {
    matches!(block.get("type").and_then(Value::as_str), Some("input_text" | "input_image" | "input_file"))
}

/// Whether an input item is the relocated volatile-reminance reminder.
///
/// The reminder carries per-turn content, so it must never receive an explicit
/// breakpoint (the marker would churn every turn).
fn is_runtime_reminder_item(item: &Value) -> bool {
    item.get("content").and_then(Value::as_array).is_some_and(|blocks| {
        blocks.iter().any(|block| {
            block
                .get("text")
                .and_then(Value::as_str)
                .is_some_and(|text| text.starts_with(DYNAMIC_RUNTIME_REMINDER_LABEL))
        })
    })
}

fn apply_explicit_cache_breakpoints(input: &mut [Value], model: &str, instruction_anchor: bool) {
    if !is_explicit_cache_breakpoint_model(model) || input.is_empty() {
        return;
    }

    // Stable-prefix discipline: the newest trailing item may be a varying runtime
    // reminder, which must not consume a cache write. Explicit markers go on user-message
    // boundaries inside the stable prefix so growing history keeps earlier
    // markers byte-stable across turns. Implicit mode uses one of four write slots,
    // leaving three explicit writes. Reserve one for developer instructions
    // so that the anchor survives compaction replacing the history.
    let stable_end = input.len().saturating_sub(1);
    let mut marked = u8::from(instruction_anchor);
    for item in input[..stable_end].iter_mut().rev() {
        if marked >= 3 {
            break;
        }
        if is_runtime_reminder_item(item) {
            continue;
        }
        let Some(content) = item.get_mut("content").and_then(Value::as_array_mut) else {
            continue;
        };
        let Some(block) = content.iter_mut().rev().find(|block| is_eligible_cache_breakpoint_block(block)) else {
            continue;
        };
        if block.get("prompt_cache_breakpoint").is_some() {
            continue;
        }
        block["prompt_cache_breakpoint"] = json!({ "mode": "explicit" });
        marked += 1;
    }

    // Single-turn (or no eligible block in the stable prefix): mark the last
    // eligible block so the first request still writes. The trailing item is
    // the varying suffix (latest message or relocated runtime reminder), so
    // skip it whenever an earlier item exists; otherwise the marker itself
    // would churn every turn.
    if marked == 0 {
        let fallback_end = if input.len() > 1 { input.len() - 1 } else { input.len() };
        for item in input[..fallback_end].iter_mut().rev() {
            // A reminder-only input (history of only System messages) has no
            // stable content to anchor; marking it would churn per turn.
            if is_runtime_reminder_item(item) {
                continue;
            }
            let Some(content) = item.get_mut("content").and_then(Value::as_array_mut) else {
                continue;
            };
            if let Some(block) = content.iter_mut().rev().find(|block| is_eligible_cache_breakpoint_block(block))
                && block.get("prompt_cache_breakpoint").is_none()
            {
                block["prompt_cache_breakpoint"] = json!({ "mode": "explicit" });
                break;
            }
        }
    }
}

pub(super) fn build_responses_item_history(
    request: &provider::LLMRequest,
    ctx: &ResponsesRequestContext<'_>,
) -> Result<OpenAIResponsesPayload, provider::LLMError> {
    let preserve_structured_history = ctx.include_structured_history_in_input
        || (ctx.preserve_structured_history_on_replay
            && (is_openai_gpt_responses_model(&request.model) || is_gpt5_codex_model(&request.model)));
    let mut responses_payload = build_standard_responses_payload(request, preserve_structured_history)?;
    if responses_payload.instructions.is_none()
        && preserve_structured_history
        && let Some(instructions) = default_replay_instructions(&request.model)
    {
        responses_payload.instructions = Some(instructions);
    }

    separate_dynamic_instructions(
        request,
        &mut responses_payload,
        ctx.is_responses_api_model && ctx.include_explicit_cache_breakpoints,
    );

    if !(ctx.include_assistant_phase
        || ctx.preserve_assistant_phase_on_replay && supports_assistant_phase_replay(&request.model))
    {
        strip_non_native_assistant_phase(&mut responses_payload.input);
    }

    // Explicit breakpoints add chosen stable-prefix writes alongside implicit
    // caching, which can also look up earlier eligible message boundaries.
    // Gated on the Responses model family (not the TTL flag) so the API-key
    // and ChatGPT backends keep sharing one item/history builder, and on the
    // backend flag so third-party endpoints never receive markers.
    // Older models reject the field outright, enforced inside the helper, and
    // non-native endpoints never receive markers (they 400 on the field).
    if ctx.is_responses_api_model && ctx.include_explicit_cache_breakpoints {
        // Top-level instructions cannot carry a breakpoint. Relocate the
        // supplied stable system prompt at the wire boundary only; replay-only
        // fallback instructions keep their existing representation.
        let instruction_anchor = is_explicit_cache_breakpoint_model(&request.model)
            && request.system_prompt.is_some()
            && responses_payload
                .instructions
                .as_ref()
                .is_some_and(|text| !text.trim().is_empty());
        if instruction_anchor && let Some(instructions) = responses_payload.instructions.take() {
            responses_payload.input.insert(
                0,
                json!({
                    "role": "developer",
                    "content": [{"type": "input_text", "text": instructions,
                        "prompt_cache_breakpoint": {"mode": "explicit"}}]
                }),
            );
        }
        apply_explicit_cache_breakpoints(&mut responses_payload.input, &request.model, instruction_anchor);
    }

    Ok(responses_payload)
}

/// Split volatile content out of Responses `instructions` into a trailing reminder.
///
/// The payload builder folds the whole system prompt (including per-turn
/// sections like `[Runtime Tool Catalog]`, `[Deferred Tools]`, and planning
/// notices) plus every history `System` message (compaction summaries, resume
/// notes) into one `instructions` string. Any per-turn change there rewrites
/// the cached prefix from byte zero, so sustained hit rates collapse.
/// Mirror the Anthropic wire split: the stable system prefix stays in
/// `instructions` (with the model contract addendum applied to the stable part
/// only) while volatile segments — labeled by the builder via
/// `InstructionSegmentKind`, so segment attribution never depends on guessing
/// at a joined-string layout — move to a trailing reminder item.
/// History order is otherwise untouched, so grown histories keep the stable
/// input prefix byte-stable across turns.
///
/// Fast path (no volatile segments): `instructions` keeps the exact legacy
/// augmented value. When the builder supplied no provenance (e.g. a static
/// replay fallback injected upstream), the whole string is treated as stable.
fn separate_dynamic_instructions(
    request: &provider::LLMRequest,
    payload: &mut OpenAIResponsesPayload,
    native_cache_boundaries: bool,
) {
    let Some(built) = payload.instructions.take() else {
        return;
    };
    let segments = match payload.instruction_segments.take() {
        Some(segments) => segments,
        None => {
            // No provenance: static replay fallback or a caller-built string.
            // Keep it whole — it is session-stable by construction.
            payload.instructions = Some(augment_openai_instructions(&request.model, built));
            return;
        }
    };

    // Partition by provenance into a stable prefix and a volatile tail, then
    // split the system-prompt segment on dynamic-section headers so its
    // per-turn sections (`[Harness Limits]`, `[Runtime Tool Catalog]`, …) also
    // leave the cached prefix. Volatile segments keep their wire order.
    let mut stable_parts: Vec<String> = Vec::with_capacity(segments.len());
    let mut volatile_parts: Vec<String> = Vec::new();
    for (kind, text) in &segments {
        match kind {
            InstructionSegmentKind::SystemPrompt => {
                let (stable, dynamic) = split_dynamic_prompt_suffix(text);
                if !stable.is_empty() {
                    stable_parts.push(stable);
                }
                if let Some(dynamic) = dynamic.filter(|text| !text.trim().is_empty()) {
                    volatile_parts.push(dynamic);
                }
            }
            InstructionSegmentKind::HistorySystem | InstructionSegmentKind::FoldedHistory => {
                volatile_parts.push(text.clone());
            }
        }
    }

    if volatile_parts.is_empty() {
        payload.instructions = Some(augment_openai_instructions(&request.model, built));
        return;
    }

    let stable_joined = stable_parts.join("\n\n");
    let stable_instructions = if stable_joined.trim().is_empty() {
        // A system prompt that is entirely dynamic leaves no stable text, but
        // the model contract addendum must still ride the cached prefix so it
        // is not silently dropped (legacy always appended it).
        let had_system_prompt = segments.iter().any(|(kind, _)| *kind == InstructionSegmentKind::SystemPrompt);
        let addendum_only = augment_openai_instructions(&request.model, String::new());
        (had_system_prompt && !addendum_only.trim().is_empty()).then_some(addendum_only)
    } else {
        Some(augment_openai_instructions(&request.model, stable_joined))
    };
    payload.instructions = stable_instructions;

    let tail = volatile_parts.join("\n\n");
    let mut reminder = String::with_capacity(DYNAMIC_RUNTIME_REMINDER_LABEL.len() + 1 + tail.len());
    reminder.push_str(DYNAMIC_RUNTIME_REMINDER_LABEL);
    reminder.push('\n');
    reminder.push_str(&tail);
    // Native GPT-5.6+ writes its implicit cache at the latest eligible user or
    // tool-result boundary. This request-only reminder is replaced each time:
    // making it a user message writes a prefix that the next request cannot
    // match, starving growing tool history of reusable writes. A developer
    // message after the initial developer group is not an implicit boundary.
    // Keep legacy/backend shapes where these cache semantics are unavailable.
    // https://developers.openai.com/api/docs/guides/prompt-caching#how-caching-works
    let role = if native_cache_boundaries && is_explicit_cache_breakpoint_model(&request.model) {
        "developer"
    } else {
        "user"
    };
    payload.input.push(json!({
        "role": role,
        "content": [{ "type": "input_text", "text": reminder }]
    }));
}
