use std::sync::Arc;

use anyhow::{Context, Result};
use vtcode_commons::preview::{condense_text_bytes, tail_preview_text};
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::core::agent::result_reducers::{project_model_tool_result, reduce_tool_result};
use vtcode_core::llm::provider::{LLMRequest, Message as LlmMessage};
use vtcode_core::llm::{
    LightweightFeature, collect_single_response, create_provider_for_model_route, resolve_lightweight_route,
};
use vtcode_core::tools::SpooledOutputReference;
use vtcode_core::tools::continuation::{PtyContinuationArgs, ReadChunkContinuationArgs};

use super::error_handling::{serialize_json_for_model, truncate_text_for_model};
use super::helpers::serialize_output;
use super::read_extent;
use crate::agent::runloop::unified::turn::context::TurnProcessingContext;

const TOOL_OUTPUT_SUMMARY_MAX_INPUT_CHARS: usize = 12_000;
const TOOL_OUTPUT_SUMMARY_MAX_OUTPUT_TOKENS: u32 = 400;
const TOOL_OUTPUT_SUMMARY_SYSTEM_PROMPT: &str = "You summarize tool outputs for VT Code before they are added to the model context. Preserve actionable facts, concrete errors, file paths, commands, and next steps. Keep it concise, faithful, and specific. Do not invent details.";
const TOOL_OUTPUT_SUMMARY_READ_HEAD_BYTES: usize = 6_000;
const TOOL_OUTPUT_SUMMARY_READ_TAIL_BYTES: usize = 4_000;
const TOOL_OUTPUT_SUMMARY_EXEC_TAIL_BYTES: usize = 10_000;
const TOOL_OUTPUT_SUMMARY_EXEC_MAX_LINES: usize = 120;
const TOOL_OUTPUT_SUMMARY_GENERIC_HEAD_BYTES: usize = 4_000;
const TOOL_OUTPUT_SUMMARY_GENERIC_TAIL_BYTES: usize = 4_000;

pub(crate) fn compact_model_tool_payload(output: serde_json::Value) -> serde_json::Value {
    let Some(obj) = output.as_object() else {
        return output;
    };

    let next_continue_args = obj.get("next_continue_args").and_then(PtyContinuationArgs::from_value);
    let next_read_args = obj.get("next_read_args").and_then(ReadChunkContinuationArgs::from_value);
    let session_id = obj.get("session_id").and_then(serde_json::Value::as_str);
    let process_session_id = session_id.or_else(|| {
        next_continue_args
            .as_ref()
            .map(|next_continue| next_continue.session_id.as_str())
    });
    let loop_detected = obj.get("loop_detected").and_then(serde_json::Value::as_bool).unwrap_or(false);
    let is_exec_like = obj.contains_key("command")
        || obj.contains_key("working_directory")
        || session_id.is_some()
        || obj.contains_key("process_id")
        || obj.contains_key("is_exited")
        || obj.contains_key("exit_code")
        || obj.contains_key("rows")
        || obj.contains_key("cols")
        || obj
            .get("content_type")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|content_type| matches!(content_type, "exec_inspect" | "git_diff"));
    let keep_exec_success_critical_note = should_keep_exec_success_critical_note(obj, is_exec_like);
    let keep_next_action = should_keep_exec_success_next_action(obj, is_exec_like)
        || should_keep_recoverable_failure_next_action(obj)
        || should_keep_search_recovery_success_next_action(obj)
        || loop_detected;
    let has_stderr = obj.get("stderr").and_then(serde_json::Value::as_str).is_some();
    let output_trimmed = obj.get("output").and_then(serde_json::Value::as_str).map(str::trim_end);

    let mut sanitized = serde_json::Map::with_capacity(obj.len());
    for (key, value) in obj {
        let skip = match key.as_str() {
            "message" | "metadata" | "no_spool" | "follow_up_prompt" | "next_poll_args" | "rows" | "cols"
            | "wall_time" | "modified_files" => true,
            "success" => value.as_bool().unwrap_or(false),
            "status" => value.as_str().is_some_and(|status| status == "success"),
            "spool_hint" | "spooled_bytes" | "spooled_to_file" => true,
            "id" => session_id.is_some_and(|sid| value == sid),
            "working_directory" => is_exec_like || value.is_null(),
            "critical_note" => !keep_exec_success_critical_note,
            "next_action" => !keep_next_action,
            "has_more" | "preferred_next_action" => {
                next_continue_args.is_some() || next_read_args.is_some() || is_false_bool(value)
            }
            "session_id" | "command" => is_exec_like,
            "spool_path" => {
                !loop_detected
                    && next_read_args
                        .as_ref()
                        .is_some_and(|next_read| value == next_read.path.as_str())
            }
            "next_offset" => next_read_args
                .as_ref()
                .is_some_and(|next_read| value_matches_usize(value, next_read.offset)),
            "chunk_limit" => next_read_args
                .as_ref()
                .is_some_and(|next_read| value_matches_usize(value, next_read.limit)),
            "stderr_preview" => has_stderr,
            "loop_detected_note" => false,
            "spool_ref_only" => false,
            "result_ref_only" => false,
            "reused_spooled_output" => false,
            "reused_recent_result" => false,
            "repeat_count" => false,
            "tool" => false,
            "limit" => loop_detected && obj.get("tool").is_some(),
            // `loop_detected` is internal control logic — always strip it from
            // model output.  The related metadata keys (`reused_recent_result`,
            // `loop_detected_note`, etc.) are gated on `loop_detected` being true
            // elsewhere, so stripping the flag itself doesn't affect them.
            "loop_detected" => true,
            "truncated" | "auto_recovered" | "query_truncated" => is_false_bool(value),
            "stdout" => {
                (is_exec_like && output_trimmed.is_some()) || output_trimmed == value.as_str().map(str::trim_end)
            }
            "process_id" => is_exec_like || process_session_id.is_some_and(|sid| value == sid),
            "is_exited" => {
                is_exec_like
                    && (value.as_bool().is_some() || next_continue_args.is_some() || obj.get("exit_code").is_some())
            }
            _ => false,
        };

        if skip {
            continue;
        }

        let cloned_value = match key.as_str() {
            "next_continue_args" => compact_next_continue_args(value),
            "next_read_args" => compact_next_read_args(value),
            _ => value.clone(),
        };
        sanitized.insert(key.clone(), cloned_value);
    }

    serde_json::Value::Object(sanitized)
}

fn should_prefer_spool_reference_only(tool_name: &str, output: &serde_json::Value) -> bool {
    vtcode_core::tools::tool_intent::should_use_spool_reference_only(Some(tool_name), output)
}

pub(super) fn truncate_stderr_preview(stderr: &str) -> String {
    const PREVIEW_CHARS: usize = 500;
    if stderr.chars().nth(PREVIEW_CHARS).is_some() {
        let mut truncated: String = stderr.chars().take(PREVIEW_CHARS).collect();
        truncated.push_str("... (truncated)");
        truncated
    } else {
        stderr.to_string()
    }
}

fn apply_spool_reference_only(compacted: &mut serde_json::Value, original: &serde_json::Value) {
    let Some(obj) = compacted.as_object_mut() else {
        return;
    };

    obj.remove("output");
    obj.remove("content");
    obj.remove("stdout");
    obj.remove("stderr");
    obj.remove("matches");
    obj.remove("results");
    obj.remove("files");
    obj.remove("entries");

    if let Some(stderr) = original.get("stderr").and_then(serde_json::Value::as_str)
        && !stderr.trim().is_empty()
    {
        obj.entry("stderr_preview".to_string())
            .or_insert_with(|| serde_json::Value::String(truncate_stderr_preview(stderr)));
    }

    if let Some(spool_path) = original.get("spool_path").and_then(serde_json::Value::as_str) {
        obj.entry("spool_path".to_string())
            .or_insert_with(|| serde_json::Value::String(spool_path.to_string()));
    }

    if let Some(spooled_bytes) = original.get("spooled_bytes") {
        obj.entry("spooled_bytes".to_string()).or_insert_with(|| spooled_bytes.clone());
    }

    obj.insert("result_ref_only".to_string(), serde_json::Value::Bool(true));
}

pub(super) fn maybe_inline_spooled(tool_name: &str, output: &serde_json::Value) -> String {
    let mut compacted = compact_model_tool_payload(output.clone());
    if should_prefer_spool_reference_only(tool_name, output) {
        apply_spool_reference_only(&mut compacted, output);
    }
    serialize_output(&compacted)
}

/// Shape a spooled result for the model using the bounded preview produced by
/// the core spooler. Consumers never reopen `spool_path` on this response path.
pub(super) fn maybe_inline_spooled_with_preview(tool_name: &str, output: &serde_json::Value) -> String {
    let mut compacted = compact_model_tool_payload(output.clone());
    let prefer_ref_only = should_prefer_spool_reference_only(tool_name, output);
    if !prefer_ref_only {
        return serialize_output(&compacted);
    }

    let spooled_output = SpooledOutputReference::from_value(output);
    apply_spool_reference_only(&mut compacted, output);
    if let Some(obj) = compacted.as_object_mut() {
        obj.remove("preview");
        if let Some(preview) = spooled_output.and_then(|spooled| spooled.preview) {
            obj.insert("preview".to_string(), serde_json::Value::String(preview.to_string()));
        }
    }

    serialize_output(&compacted)
}

fn read_summary_source_path(output: &serde_json::Value) -> Option<&str> {
    output
        .get("source_path")
        .and_then(serde_json::Value::as_str)
        .or_else(|| output.get("path").and_then(serde_json::Value::as_str))
        .filter(|path| !path.trim().is_empty())
}

fn is_large_read_summary_output(tool_name: &str, output: &serde_json::Value) -> bool {
    matches!(tool_name, tool_names::READ_FILE)
        || (tool_name == tool_names::UNIFIED_FILE && read_summary_source_path(output).is_some())
}

fn build_spooled_tool_summary_excerpt(tool_name: &str, output: &serde_json::Value, spool_content: &str) -> String {
    let mut sections = Vec::new();

    if should_prefer_spool_reference_only(tool_name, output) {
        if let Some(stderr_preview) = output
            .get("stderr_preview")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            sections.push(format!("stderr_preview:\n{stderr_preview}"));
        }
        sections.push(format!(
            "tail_excerpt:\n{}",
            tail_preview_text(spool_content, TOOL_OUTPUT_SUMMARY_EXEC_TAIL_BYTES, TOOL_OUTPUT_SUMMARY_EXEC_MAX_LINES,)
        ));
        return sections.join("\n\n");
    }

    if is_large_read_summary_output(tool_name, output) {
        if let Some(path) = read_summary_source_path(output) {
            sections.push(format!("source_path: {path}"));
        }
        sections.push(format!(
            "content_excerpt:\n{}",
            condense_text_bytes(
                spool_content,
                TOOL_OUTPUT_SUMMARY_READ_HEAD_BYTES,
                TOOL_OUTPUT_SUMMARY_READ_TAIL_BYTES,
            )
        ));
        return sections.join("\n\n");
    }

    format!(
        "content_excerpt:\n{}",
        condense_text_bytes(
            spool_content,
            TOOL_OUTPUT_SUMMARY_GENERIC_HEAD_BYTES,
            TOOL_OUTPUT_SUMMARY_GENERIC_TAIL_BYTES,
        )
    )
}

fn build_spooled_tool_summary_input(tool_name: &str, output: &serde_json::Value, spool_content: &str) -> String {
    let metadata = maybe_inline_spooled(tool_name, output);
    let excerpt = build_spooled_tool_summary_excerpt(tool_name, output, spool_content);
    if excerpt.trim().is_empty() {
        metadata
    } else {
        format!("Tool payload:\n{metadata}\n\nSpooled content excerpt:\n{excerpt}")
    }
}

pub(super) fn tool_output_summary_input_or_serialized(
    tool_name: &str,
    output: &serde_json::Value,
    serialized_output: &str,
) -> String {
    let Some(preview) = SpooledOutputReference::from_value(output).and_then(|spooled| spooled.preview) else {
        return serialized_output.to_string();
    };
    build_spooled_tool_summary_input(tool_name, output, preview)
}

fn tool_output_summary_feature(
    tool_name: &str,
    args_val: &serde_json::Value,
    output: &serde_json::Value,
    serialized_len: usize,
) -> Option<LightweightFeature> {
    let action = args_val.get("action").and_then(serde_json::Value::as_str);
    let command = args_val
        .get("command")
        .map(serialize_json_for_model)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let content_type = output
        .get("content_type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();

    let is_large_read = matches!(tool_name, tool_names::READ_FILE)
        || (tool_name == tool_names::UNIFIED_FILE && matches!(action, Some("read" | "read_chunk" | "cat")));
    if is_large_read {
        // Spooled output is genuinely huge (already offloaded to disk); always
        // summarize regardless of how the read was scoped.
        if output.get("spool_path").is_some() {
            return Some(LightweightFeature::LargeReadSummary);
        }
        // A bounded read (explicit offset/limit/line-range) is a deliberate
        // request for exact content. Summarizing it forces a wasteful raw
        // re-read of the same slice, so honor the model's narrowing and return
        // it verbatim. Only whole-file reads over the byte threshold are
        // summarized.
        if !read_extent::args_have_bounded_extent(args_val) && serialized_len > 6_000 {
            return Some(LightweightFeature::LargeReadSummary);
        }
    }

    if tool_name == tool_names::WEB_FETCH || content_type == "web_page" {
        return (serialized_len > 2_500).then_some(LightweightFeature::WebSummary);
    }

    let is_git_history = tool_name == tool_names::UNIFIED_EXEC
        && (content_type == "git_diff"
            || command.contains("git log")
            || command.contains("git show")
            || command.contains("git diff"));
    if is_git_history && serialized_len > 2_500 {
        return Some(LightweightFeature::GitHistorySummary);
    }

    None
}

async fn summarize_tool_output_with_provider(
    provider: &dyn vtcode_core::llm::provider::LLMProvider,
    model: &str,
    tool_name: &str,
    serialized_output: &str,
) -> Result<String> {
    let prompt = format!(
        "Tool: {tool_name}\n\nOutput:\n{}\n\nReturn a concise summary that preserves the actionable details VT Code should remember for the next model turn.",
        truncate_text_for_model(serialized_output, TOOL_OUTPUT_SUMMARY_MAX_INPUT_CHARS).0
    );
    let request = LLMRequest {
        messages: Arc::new(vec![LlmMessage::user(prompt)]),
        system_prompt: Some(Arc::from(TOOL_OUTPUT_SUMMARY_SYSTEM_PROMPT)),
        model: model.to_string(),
        max_tokens: Some(TOOL_OUTPUT_SUMMARY_MAX_OUTPUT_TOKENS),
        temperature: Some(0.0),
        stream: false,
        ..Default::default()
    };
    let response = collect_single_response(provider, request)
        .await
        .context("tool output summarization failed")?;
    Ok(response.content_text().trim().to_string())
}

async fn summarize_tool_output_with_route(
    ctx: &mut TurnProcessingContext<'_>,
    route: &vtcode_core::llm::ModelRoute,
    tool_name: &str,
    serialized_output: &str,
) -> Result<String> {
    let same_runtime_provider = !ctx.config.provider.trim().is_empty()
        && route.provider_name.eq_ignore_ascii_case(ctx.config.provider.as_str())
        && route.model == ctx.config.model;
    if same_runtime_provider {
        return summarize_tool_output_with_provider(
            ctx.provider_client.as_ref(),
            &route.model,
            tool_name,
            serialized_output,
        )
        .await;
    }

    let provider = create_provider_for_model_route(route, ctx.config, ctx.vt_cfg)?;
    summarize_tool_output_with_provider(provider.as_ref(), &route.model, tool_name, serialized_output).await
}

fn summarized_tool_response_payload(
    tool_name: &str,
    args_val: &serde_json::Value,
    output: &serde_json::Value,
    summary: &str,
) -> String {
    let mut compacted = compact_model_tool_payload(output.clone());
    if should_prefer_spool_reference_only(tool_name, output) {
        apply_spool_reference_only(&mut compacted, output);
    }
    if let Some(obj) = compacted.as_object_mut() {
        obj.remove("output");
        obj.remove("content");
        obj.remove("stdout");
        obj.remove("stderr");
        obj.insert("summary".to_string(), serde_json::Value::String(summary.to_string()));
        obj.insert("summarized_for_model".to_string(), serde_json::Value::Bool(true));

        // Add read-once guidance for file reads to prevent redundant re-reads
        let action = args_val.get("action").and_then(serde_json::Value::as_str).unwrap_or("");
        let is_file_read = (tool_name == tool_names::UNIFIED_FILE || tool_name == tool_names::READ_FILE)
            && (action == "read" || action.is_empty());
        if is_file_read {
            obj.insert(
                "hint".to_string(),
                serde_json::Value::String(
                    "This summary contains sufficient context for most tasks. \
                     Re-read this file only for specific line-level detail the \
                     summary lacks; use raw=true to bypass summarization when \
                     you need exact content."
                        .to_string(),
                ),
            );
        }
    }
    serialize_output(&compacted)
}

async fn build_tool_response_content(
    ctx: &mut TurnProcessingContext<'_>,
    tool_name: &str,
    args_val: &serde_json::Value,
    output: &serde_json::Value,
) -> String {
    // Strip TUI-only display fields (e.g. task_tracker `view`) before any
    // model-facing serialization. The TUI reads `view` from the original
    // `output` via the pipeline-output path, not from this string, so
    // removing it here is display-safe and avoids feeding redundant display
    // data to the model on every tracker call.
    //
    // Apply `reduce_tool_result` first so interactive sessions share the
    // headless AgentRunner's hard caps (32 KiB / 2000 lines on read/exec
    // bodies). Without this, `raw=true` and summarizer-failure fallbacks
    // push full tool bodies into history.
    let reduced = reduce_tool_result(tool_name, output.clone());
    let model_output = project_model_tool_result(tool_name, args_val, &reduced);
    let output = model_output.as_ref();

    // Skip LLM summarization when raw=true is requested
    if args_val.get("raw").and_then(serde_json::Value::as_bool).unwrap_or(false) {
        return maybe_inline_spooled_with_preview(tool_name, output);
    }

    let fallback = maybe_inline_spooled_with_preview(tool_name, output);
    let serialized_output = serialize_json_for_model(output);
    let summary_input = tool_output_summary_input_or_serialized(tool_name, output, &serialized_output);
    let Some(feature) = tool_output_summary_feature(tool_name, args_val, output, serialized_output.len()) else {
        return fallback;
    };

    let resolution = resolve_lightweight_route(ctx.config, ctx.vt_cfg, feature, None);
    if let Some(warning) = &resolution.warning {
        tracing::warn!(warning = %warning, tool = %tool_name, "tool output route adjusted");
    }

    match summarize_tool_output_with_route(ctx, &resolution.primary, tool_name, &summary_input).await {
        Ok(summary) if !summary.trim().is_empty() => {
            return summarized_tool_response_payload(tool_name, args_val, output, summary.trim());
        }
        Ok(_) => {}
        Err(primary_err) => {
            if let Some(fallback_route) = resolution.fallback.as_ref() {
                tracing::warn!(
                    tool = %tool_name,
                    model = %resolution.primary.model,
                    fallback_model = %fallback_route.model,
                    error = %primary_err,
                    "tool output summarization failed on lightweight route; retrying with main model"
                );
                match summarize_tool_output_with_route(ctx, fallback_route, tool_name, &summary_input).await {
                    Ok(summary) if !summary.trim().is_empty() => {
                        return summarized_tool_response_payload(tool_name, args_val, output, summary.trim());
                    }
                    Ok(_) => {}
                    Err(fallback_err) => {
                        tracing::warn!(
                            tool = %tool_name,
                            error = %fallback_err,
                            "tool output summarization failed on main model; using compact fallback"
                        );
                    }
                }
            } else {
                tracing::warn!(
                    tool = %tool_name,
                    error = %primary_err,
                    "tool output summarization failed; using compact fallback"
                );
            }
        }
    }

    fallback
}

pub(super) async fn prepare_tool_response_content(
    ctx: &mut TurnProcessingContext<'_>,
    tool_name: &str,
    args_val: &serde_json::Value,
    output: &serde_json::Value,
) -> String {
    let content = build_tool_response_content(ctx, tool_name, args_val, output).await;
    record_prepared_tool_response_metrics(ctx.harness_state, output, &content);
    content
}

fn record_prepared_tool_response_metrics(
    harness_state: &mut crate::agent::runloop::unified::run_loop_context::HarnessTurnState,
    output: &serde_json::Value,
    _content: &str,
) {
    let spooled_output = SpooledOutputReference::from_value(output);
    let raw_spooled_bytes = spooled_output.and_then(|spooled| spooled.original_bytes).unwrap_or(0);
    let reused = ["reused_recent_result", "reused_spooled_output"]
        .into_iter()
        .any(|key| output.get(key).and_then(serde_json::Value::as_bool) == Some(true));
    harness_state.record_tool_output_metrics(reused, spooled_output.is_some(), raw_spooled_bytes, 0);
}

fn compact_next_continue_args(value: &serde_json::Value) -> serde_json::Value {
    let Some(parsed) = PtyContinuationArgs::from_value(value) else {
        return value.clone();
    };
    let compacted = parsed.to_compact_value();
    merge_compaction_extras(value, compacted, &["action", "session_id", "s"])
}

fn compact_next_read_args(value: &serde_json::Value) -> serde_json::Value {
    let Some(parsed) = ReadChunkContinuationArgs::from_value(value) else {
        return value.clone();
    };
    let compacted = parsed.to_compact_value();
    merge_compaction_extras(value, compacted, &["path", "offset", "limit", "p", "o", "l"])
}

/// Merge non-canonical keys from the original object onto its compact form.
///
/// Single home for the clone-and-preserve-extras pattern (DRY): both
/// continuation shapes compact their identity keys and keep any extra
/// steering fields verbatim for the harness.
fn merge_compaction_extras(
    original: &serde_json::Value,
    compacted: serde_json::Value,
    exclude: &[&str],
) -> serde_json::Value {
    let Some(obj) = original.as_object() else {
        return original.clone();
    };
    let serde_json::Value::Object(mut map) = compacted else {
        return original.clone();
    };
    for (key, nested_value) in obj {
        if !exclude.contains(&key.as_str()) {
            map.insert(key.clone(), nested_value.clone());
        }
    }
    serde_json::Value::Object(map)
}

fn is_false_bool(value: &serde_json::Value) -> bool {
    value.as_bool().is_some_and(|flag| !flag)
}

fn value_matches_usize(value: &serde_json::Value, expected: usize) -> bool {
    value
        .as_u64()
        .and_then(|actual| usize::try_from(actual).ok())
        .is_some_and(|actual| actual == expected)
}

fn has_non_empty_string_field(obj: &serde_json::Map<String, serde_json::Value>, key: &str) -> bool {
    obj.get(key)
        .and_then(serde_json::Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
}

fn has_error_payload(obj: &serde_json::Map<String, serde_json::Value>) -> bool {
    match obj.get("error") {
        Some(serde_json::Value::String(message)) => !message.trim().is_empty(),
        Some(serde_json::Value::Object(error)) => error
            .get("message")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|message| !message.trim().is_empty()),
        _ => false,
    }
}

fn is_recoverable_failure_payload(obj: &serde_json::Map<String, serde_json::Value>) -> bool {
    has_error_payload(obj) && obj.get("is_recoverable").and_then(serde_json::Value::as_bool) == Some(true)
}

fn should_keep_exec_success_critical_note(
    obj: &serde_json::Map<String, serde_json::Value>,
    is_exec_like: bool,
) -> bool {
    is_exec_like && !has_error_payload(obj) && has_non_empty_string_field(obj, "critical_note")
}

fn should_keep_exec_success_next_action(obj: &serde_json::Map<String, serde_json::Value>, is_exec_like: bool) -> bool {
    is_exec_like && !has_error_payload(obj) && has_non_empty_string_field(obj, "next_action")
}

fn should_keep_recoverable_failure_next_action(obj: &serde_json::Map<String, serde_json::Value>) -> bool {
    let policy_denied = has_error_payload(obj)
        && obj
            .get("error_class")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|class| matches!(class, "policy_denied" | "policy_blocked"));
    (is_recoverable_failure_payload(obj) || policy_denied) && has_non_empty_string_field(obj, "next_action")
}

fn should_keep_search_recovery_success_next_action(obj: &serde_json::Map<String, serde_json::Value>) -> bool {
    !has_error_payload(obj)
        && obj.get("is_recoverable").and_then(serde_json::Value::as_bool) == Some(true)
        && (obj
            .get("matches")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|matches| matches.is_empty())
            || obj
                .get("results")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|results| results.is_empty()))
        && has_non_empty_string_field(obj, "hint")
        && has_non_empty_string_field(obj, "next_action")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::agent::runloop::unified::run_loop_context::{HarnessTurnState, TurnId, TurnRunId};
    use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;

    fn big_read_output() -> serde_json::Value {
        json!({ "content_kind": "text", "path": "src/cli/mod.rs" })
    }

    #[test]
    fn whole_file_read_over_threshold_is_summarized() {
        let args = json!({ "action": "read", "path": "src/cli/mod.rs" });
        let feature = tool_output_summary_feature(tool_names::UNIFIED_FILE, &args, &big_read_output(), 10_000);
        assert!(matches!(feature, Some(LightweightFeature::LargeReadSummary)));
    }

    #[test]
    fn bounded_read_is_not_summarized_even_when_large() {
        // Reproduces the turn_640 waste: a ranged read got summarized, forcing
        // a wasteful raw re-read of the identical `offset`/`limit` slice.
        let args = json!({ "action": "read", "path": "src/cli/mod.rs", "offset": 81, "limit": 189 });
        let feature = tool_output_summary_feature(tool_names::UNIFIED_FILE, &args, &big_read_output(), 10_000);
        assert!(feature.is_none(), "bounded reads must be returned verbatim, not summarized");
    }

    #[test]
    fn bounded_read_via_start_line_is_not_summarized() {
        let args = json!({ "action": "read", "path": "src/cli/mod.rs", "start_line": 10, "end_line": 40 });
        let feature = tool_output_summary_feature(tool_names::UNIFIED_FILE, &args, &big_read_output(), 10_000);
        assert!(feature.is_none());
    }

    #[test]
    fn spooled_bounded_read_is_still_summarized() {
        // Genuinely huge output offloaded to disk must always be summarized,
        // even when the read was scoped, so context is not blown out.
        let args = json!({ "action": "read", "path": "big.log", "offset": 0, "limit": 500 });
        let output = json!({
            "content_kind": "text",
            "path": "big.log",
            "spool_path": ".vtcode/context/tool_outputs/big.log"
        });
        let feature = tool_output_summary_feature(tool_names::UNIFIED_FILE, &args, &output, 200_000);
        assert!(matches!(feature, Some(LightweightFeature::LargeReadSummary)));
    }

    #[test]
    fn small_whole_file_read_is_not_summarized() {
        let args = json!({ "action": "read", "path": "small.rs" });
        let feature = tool_output_summary_feature(tool_names::UNIFIED_FILE, &args, &big_read_output(), 500);
        assert!(feature.is_none());
    }

    #[test]
    fn apply_patch_modified_files_remain_internal() {
        let compacted = compact_model_tool_payload(json!({
            "success": true,
            "applied": [{"path": "src/widget.rs", "operation": "update"}],
            "modified_files": ["/workspace/src/widget.rs"]
        }));

        assert_eq!(compacted, json!({"applied": [{"path": "src/widget.rs", "operation": "update"}]}));
    }

    #[test]
    fn prepared_spooled_response_records_spool_metadata_without_visible_bytes() {
        let output = json!({
            "spool_path": ".vtcode/context/tool_outputs/command.log",
            "spooled_bytes": 12_345,
            "preview": "bounded command preview",
            "exit_code": 1,
            "failure_diagnostics": "command completed with status 1"
        });
        let content = maybe_inline_spooled_with_preview(tool_names::UNIFIED_EXEC, &output);
        let mut harness_state = HarnessTurnState::new(
            TurnRunId("response-content-run".to_string()),
            TurnId("response-content-turn".to_string()),
            4,
            120,
            2,
        );

        record_prepared_tool_response_metrics(&mut harness_state, &output, &content);

        let diagnostics = harness_state.snapshot_turn_diagnostics(Default::default(), 0);
        assert_eq!(diagnostics.spooled_results, 1);
        assert_eq!(diagnostics.raw_spooled_bytes, 12_345);
        assert_eq!(diagnostics.model_visible_output_bytes, 0);
    }

    #[test]
    fn generic_reference_only_spooled_response_preserves_spool_metadata_and_preview() {
        let output = json!({
            "spool_path": ".vtcode/context/tool_outputs/command.log",
            "spooled_bytes": 38_912,
            "spool_line_count": 377,
            "preview": "bounded command preview",
            "output": "large output that was written to the spool"
        });

        let content = maybe_inline_spooled_with_preview(tool_names::UNIFIED_EXEC, &output);
        let shaped: serde_json::Value = serde_json::from_str(&content).expect("shaped response should be JSON");
        assert_eq!(shaped["spool_path"], output["spool_path"]);
        assert_eq!(shaped["spooled_bytes"], 38_912);
        assert_eq!(shaped["spool_line_count"], 377);
        assert_eq!(shaped["preview"], "bounded command preview");
        assert!(shaped.get("output").is_none());
    }

    #[tokio::test]
    async fn successful_prepared_tool_response_counts_visible_bytes_once_after_history_push() {
        let output = json!({
            "spool_path": ".vtcode/context/tool_outputs/command.log",
            "spooled_bytes": 12_345,
            "preview": "bounded command preview",
            "exit_code": 1,
            "failure_diagnostics": "command completed with status 1"
        });
        let mut backing = TestTurnProcessingBacking::new(4).await;

        let diagnostics = {
            let mut ctx = backing.turn_processing_context();
            let args = json!({ "cmd": "printf test", "raw": true });
            let content = prepare_tool_response_content(&mut ctx, tool_names::UNIFIED_EXEC, &args, &output).await;
            let expected_visible_bytes = content.len() as u64;

            ctx.push_tool_response("call_1", Some(tool_names::UNIFIED_EXEC), content);

            let diagnostics = ctx.harness_state.snapshot_turn_diagnostics(Default::default(), 0);
            assert_eq!(diagnostics.model_visible_output_bytes, expected_visible_bytes);
            diagnostics
        };

        assert_eq!(diagnostics.spooled_results, 1);
        assert_eq!(diagnostics.raw_spooled_bytes, 12_345);
    }

    #[tokio::test]
    async fn same_call_tool_response_overwrite_replaces_visible_byte_accounting() {
        let mut backing = TestTurnProcessingBacking::new(4).await;

        let diagnostics = {
            let mut ctx = backing.turn_processing_context();
            let first_content = r#"{"output":"first"}"#.to_string();
            let final_content = r#"{"output":"latest result"}"#.to_string();

            ctx.push_tool_response("call_1", Some(tool_names::READ_FILE), first_content);
            ctx.push_tool_response("call_1", Some(tool_names::READ_FILE), final_content.clone());

            let tool_messages: Vec<&vtcode_core::llm::provider::Message> = ctx
                .working_history
                .iter()
                .filter(|message| matches!(message.role, vtcode_core::llm::provider::MessageRole::Tool))
                .collect();
            assert_eq!(tool_messages.len(), 1, "same-call overwrite should replace the in-place history entry");
            assert_eq!(tool_messages[0].content.as_text_borrowed(), Some(final_content.as_str()));

            let diagnostics = ctx.harness_state.snapshot_turn_diagnostics(Default::default(), 0);
            assert_eq!(
                diagnostics.model_visible_output_bytes,
                final_content.len() as u64,
                "visible-byte diagnostics should match the final history content exactly once"
            );
            diagnostics
        };

        assert_eq!(diagnostics.spooled_results, 0);
        assert_eq!(diagnostics.raw_spooled_bytes, 0);
    }

    #[tokio::test]
    async fn raw_tool_response_applies_reduce_tool_result_hard_caps() {
        // Interactive path must share the headless AgentRunner's reduce caps
        // even when raw=true skips summarization (the previous leak).
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let stdout = (0..2200).map(|_| "a").collect::<Vec<_>>().join("\n");
        let output = json!({ "stdout": stdout, "exit_code": 0 });
        let args = json!({ "cmd": "gen", "raw": true });

        let mut ctx = backing.turn_processing_context();
        let content = prepare_tool_response_content(&mut ctx, tool_names::UNIFIED_EXEC, &args, &output).await;

        let shaped: serde_json::Value = serde_json::from_str(&content).expect("model content should be JSON");
        assert_eq!(shaped["is_truncated"], json!(true));
        let visible = shaped["stdout"].as_str().expect("stdout remains a string");
        assert!(
            visible.lines().count() < 2200,
            "raw=true must still apply reduce_tool_result line caps, got {} lines",
            visible.lines().count()
        );
    }

    #[test]
    fn compact_continuation_preserves_extras_asymmetrically() {
        // Asymmetric: continue carries a wait hint extra, read carries a path-extra.
        // Both must compact identity keys yet keep their own extras verbatim.
        let continued = compact_next_continue_args(&json!({
            "session_id": "run-continue-1",
            "next_hint": "wait-again"
        }));
        assert_eq!(continued["s"], json!("run-continue-1"));
        assert!(continued.get("session_id").is_none());
        assert_eq!(continued["next_hint"], json!("wait-again"));

        let read = compact_next_read_args(&json!({
            "path": "a.log",
            "offset": 81,
            "limit": 40,
            "spool_chunked": true
        }));
        assert_eq!(read["p"], json!("a.log"));
        assert_eq!(read["o"], json!(81));
        assert!(read.get("path").is_none());
        assert_eq!(read["spool_chunked"], json!(true));
    }

    #[test]
    fn compact_continuation_passes_through_unparsable() {
        // Boundary: non-object and missing-identity inputs must round-trip untouched,
        // never produce an empty compact object the harness would misread.
        let scalar = json!("not-an-object");
        assert_eq!(compact_next_continue_args(&scalar), scalar);
        let missing = json!({"unrelated": 1});
        assert_eq!(compact_next_continue_args(&missing), missing);
        assert_eq!(compact_next_read_args(&missing), missing);
    }
}
