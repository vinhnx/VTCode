use anyhow::{Context, Result};
use std::path::Path;
use vtcode_commons::preview::{condense_text_bytes, tail_preview_text};
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::llm::provider::{LLMRequest, Message as LlmMessage};
use vtcode_core::llm::{
    LightweightFeature, collect_single_response, create_provider_for_model_route,
    resolve_lightweight_route,
};
use vtcode_core::tools::continuation::{PtyContinuationArgs, ReadChunkContinuationArgs};
use vtcode_core::tools::validation::unified_path::validate_and_resolve_path;

use super::error_handling::{serialize_json_for_model, truncate_text_for_model};
use super::helpers::serialize_output;
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

    let next_continue_args = obj
        .get("next_continue_args")
        .and_then(PtyContinuationArgs::from_value);
    let next_read_args = obj
        .get("next_read_args")
        .and_then(ReadChunkContinuationArgs::from_value);
    let session_id = obj.get("session_id").and_then(serde_json::Value::as_str);
    let process_session_id = session_id.or_else(|| {
        next_continue_args
            .as_ref()
            .map(|next_continue| next_continue.session_id.as_str())
    });
    let loop_detected = obj
        .get("loop_detected")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
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
        || should_keep_search_recovery_success_next_action(obj);
    let has_stderr = obj
        .get("stderr")
        .and_then(serde_json::Value::as_str)
        .is_some();
    let output_trimmed = obj
        .get("output")
        .and_then(serde_json::Value::as_str)
        .map(str::trim_end);

    let mut sanitized = serde_json::Map::with_capacity(obj.len());
    for (key, value) in obj {
        let skip = match key.as_str() {
            "message" | "metadata" | "no_spool" | "follow_up_prompt" | "next_poll_args"
            | "rows" | "cols" | "wall_time" => true,
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
            "loop_detected_note"
            | "spool_ref_only"
            | "result_ref_only"
            | "reused_spooled_output"
            | "reused_recent_result"
            | "repeat_count"
            | "tool" => loop_detected,
            "limit" => loop_detected && obj.get("tool").is_some(),
            "truncated" | "auto_recovered" | "loop_detected" | "query_truncated" => {
                is_false_bool(value)
            }
            "stdout" => {
                (is_exec_like && output_trimmed.is_some())
                    || output_trimmed == value.as_str().map(str::trim_end)
            }
            "process_id" => is_exec_like || process_session_id.is_some_and(|sid| value == sid),
            "is_exited" => {
                is_exec_like
                    && (value.as_bool().is_some()
                        || next_continue_args.is_some()
                        || obj.get("exit_code").is_some())
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

    if !obj.contains_key("stderr_preview")
        && let Some(stderr) = original.get("stderr").and_then(serde_json::Value::as_str)
        && !stderr.trim().is_empty()
    {
        obj.insert(
            "stderr_preview".to_string(),
            serde_json::Value::String(truncate_stderr_preview(stderr)),
        );
    }

    if !obj.contains_key("spool_path")
        && let Some(spool_path) = original
            .get("spool_path")
            .and_then(serde_json::Value::as_str)
    {
        obj.insert(
            "spool_path".to_string(),
            serde_json::Value::String(spool_path.to_string()),
        );
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

fn build_spooled_tool_summary_excerpt(
    tool_name: &str,
    output: &serde_json::Value,
    spool_content: &str,
) -> String {
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
            tail_preview_text(
                spool_content,
                TOOL_OUTPUT_SUMMARY_EXEC_TAIL_BYTES,
                TOOL_OUTPUT_SUMMARY_EXEC_MAX_LINES,
            )
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

fn build_spooled_tool_summary_input(
    tool_name: &str,
    output: &serde_json::Value,
    spool_content: &str,
) -> String {
    let metadata = maybe_inline_spooled(tool_name, output);
    let excerpt = build_spooled_tool_summary_excerpt(tool_name, output, spool_content);
    if excerpt.trim().is_empty() {
        metadata
    } else {
        format!("Tool payload:\n{metadata}\n\nSpooled content excerpt:\n{excerpt}")
    }
}

pub(super) async fn tool_output_summary_input_or_serialized(
    workspace_root: &Path,
    tool_name: &str,
    output: &serde_json::Value,
    serialized_output: &str,
) -> String {
    let Some(spool_path) = output
        .get("spool_path")
        .and_then(serde_json::Value::as_str)
        .filter(|path| !path.trim().is_empty())
    else {
        return serialized_output.to_string();
    };

    let resolved = match validate_and_resolve_path(workspace_root, spool_path).await {
        Ok(path) => path,
        Err(err) => {
            tracing::warn!(
                tool = %tool_name,
                spool_path,
                error = %err,
                "Failed to validate spool path for tool output summary; using compact fallback"
            );
            return serialized_output.to_string();
        }
    };

    match tokio::fs::read(&resolved).await {
        Ok(spool_bytes) => {
            let spool_content = String::from_utf8_lossy(&spool_bytes);
            build_spooled_tool_summary_input(tool_name, output, &spool_content)
        }
        Err(err) => {
            tracing::warn!(
                tool = %tool_name,
                spool_path = %resolved.display(),
                error = %err,
                "Failed to read spool file for tool output summary; using compact fallback"
            );
            serialized_output.to_string()
        }
    }
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
        || (tool_name == tool_names::UNIFIED_FILE
            && matches!(action, Some("read" | "read_chunk" | "cat")));
    if is_large_read && (serialized_len > 6_000 || output.get("spool_path").is_some()) {
        return Some(LightweightFeature::LargeReadSummary);
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
        messages: vec![LlmMessage::user(prompt)],
        system_prompt: Some(std::sync::Arc::new(
            TOOL_OUTPUT_SUMMARY_SYSTEM_PROMPT.to_string(),
        )),
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
        && route
            .provider_name
            .eq_ignore_ascii_case(ctx.config.provider.as_str())
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
    summarize_tool_output_with_provider(
        provider.as_ref(),
        &route.model,
        tool_name,
        serialized_output,
    )
    .await
}

fn summarized_tool_response_payload(
    tool_name: &str,
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
        obj.insert(
            "summary".to_string(),
            serde_json::Value::String(summary.to_string()),
        );
        obj.insert(
            "summarized_for_model".to_string(),
            serde_json::Value::Bool(true),
        );
    }
    serialize_output(&compacted)
}

pub(super) async fn prepare_tool_response_content(
    ctx: &mut TurnProcessingContext<'_>,
    tool_name: &str,
    args_val: &serde_json::Value,
    output: &serde_json::Value,
) -> String {
    let fallback = maybe_inline_spooled(tool_name, output);
    let serialized_output = serialize_json_for_model(output);
    let summary_input = tool_output_summary_input_or_serialized(
        ctx.tool_registry.workspace_root().as_path(),
        tool_name,
        output,
        &serialized_output,
    )
    .await;
    let Some(feature) =
        tool_output_summary_feature(tool_name, args_val, output, serialized_output.len())
    else {
        return fallback;
    };

    let resolution = resolve_lightweight_route(ctx.config, ctx.vt_cfg, feature, None);
    if let Some(warning) = &resolution.warning {
        tracing::warn!(warning = %warning, tool = %tool_name, "tool output route adjusted");
    }

    match summarize_tool_output_with_route(ctx, &resolution.primary, tool_name, &summary_input)
        .await
    {
        Ok(summary) if !summary.trim().is_empty() => {
            return summarized_tool_response_payload(tool_name, output, summary.trim());
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
                match summarize_tool_output_with_route(
                    ctx,
                    fallback_route,
                    tool_name,
                    &summary_input,
                )
                .await
                {
                    Ok(summary) if !summary.trim().is_empty() => {
                        return summarized_tool_response_payload(tool_name, output, summary.trim());
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

fn compact_next_continue_args(value: &serde_json::Value) -> serde_json::Value {
    let Some(obj) = value.as_object() else {
        return value.clone();
    };
    let Some(parsed) = PtyContinuationArgs::from_value(value) else {
        return value.clone();
    };

    let mut compacted = match parsed.to_compact_value() {
        serde_json::Value::Object(map) => map,
        _ => return value.clone(),
    };
    for (key, nested_value) in obj {
        if key != "action" && key != "session_id" && key != "s" {
            compacted.insert(key.clone(), nested_value.clone());
        }
    }
    serde_json::Value::Object(compacted)
}

fn compact_next_read_args(value: &serde_json::Value) -> serde_json::Value {
    let Some(obj) = value.as_object() else {
        return value.clone();
    };
    let Some(parsed) = ReadChunkContinuationArgs::from_value(value) else {
        return value.clone();
    };

    let mut compacted = match parsed.to_compact_value() {
        serde_json::Value::Object(map) => map,
        _ => return value.clone(),
    };
    for (key, nested_value) in obj {
        if !matches!(key.as_str(), "path" | "offset" | "limit" | "p" | "o" | "l") {
            compacted.insert(key.clone(), nested_value.clone());
        }
    }
    serde_json::Value::Object(compacted)
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
    has_error_payload(obj)
        && obj
            .get("is_recoverable")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
}

fn should_keep_exec_success_critical_note(
    obj: &serde_json::Map<String, serde_json::Value>,
    is_exec_like: bool,
) -> bool {
    is_exec_like && !has_error_payload(obj) && has_non_empty_string_field(obj, "critical_note")
}

fn should_keep_exec_success_next_action(
    obj: &serde_json::Map<String, serde_json::Value>,
    is_exec_like: bool,
) -> bool {
    is_exec_like && !has_error_payload(obj) && has_non_empty_string_field(obj, "next_action")
}

fn should_keep_recoverable_failure_next_action(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> bool {
    is_recoverable_failure_payload(obj) && has_non_empty_string_field(obj, "next_action")
}

fn should_keep_search_recovery_success_next_action(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> bool {
    !has_error_payload(obj)
        && obj.get("backend").and_then(serde_json::Value::as_str) == Some("ast-grep")
        && obj
            .get("is_recoverable")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        && obj
            .get("matches")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|matches| matches.is_empty())
        && has_non_empty_string_field(obj, "hint")
        && has_non_empty_string_field(obj, "next_action")
}
