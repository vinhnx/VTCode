use std::collections::{HashSet, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use crate::config::types::{ReasoningEffortLevel, VerbosityLevel};
use crate::core::agent::features::FeatureSet;
use crate::llm::provider::{LLMRequest, Message, ParallelToolConfig, ToolChoice, ToolDefinition};
use crate::tools::tool_intent;
use crate::tools::validation::commands;

#[derive(Debug, Clone)]
pub struct SessionToolCatalogSnapshot {
    pub version: u64,
    pub epoch: u64,
    pub plan_mode: bool,
    pub request_user_input_enabled: bool,
    pub snapshot: Option<Arc<Vec<ToolDefinition>>>,
    pub cache_hit: bool,
    pub tool_catalog_hash: Option<u64>,
}

impl SessionToolCatalogSnapshot {
    pub fn new(
        version: u64,
        epoch: u64,
        plan_mode: bool,
        request_user_input_enabled: bool,
        snapshot: Option<Arc<Vec<ToolDefinition>>>,
        cache_hit: bool,
    ) -> Self {
        let tool_catalog_hash = hash_tool_definitions(snapshot.as_deref().map(Vec::as_slice));
        Self {
            version,
            epoch,
            plan_mode,
            request_user_input_enabled,
            snapshot,
            cache_hit,
            tool_catalog_hash,
        }
    }

    pub fn available_tools(&self) -> usize {
        self.snapshot.as_ref().map_or(0, |defs| defs.len())
    }

    pub fn has_tools(&self) -> bool {
        self.snapshot.is_some()
    }

    pub fn with_cache_hit(mut self, cache_hit: bool) -> Self {
        self.cache_hit = cache_hit;
        self
    }
}

#[derive(Debug, Clone)]
pub struct FallbackRecommendation {
    pub tool_name: String,
    pub args: Value,
}

#[derive(Debug, Clone)]
pub struct PreparedToolCall {
    pub canonical_name: String,
    pub readonly_classification: bool,
    pub parallel_safe_after_preflight: bool,
    pub effective_args: Value,
    pub fallback_recommendation: Option<FallbackRecommendation>,
    pub already_preflighted: bool,
}

impl PreparedToolCall {
    pub fn new(
        canonical_name: String,
        readonly_classification: bool,
        parallel_safe_after_preflight: bool,
        effective_args: Value,
    ) -> Self {
        Self {
            canonical_name,
            readonly_classification,
            parallel_safe_after_preflight,
            effective_args,
            fallback_recommendation: None,
            already_preflighted: true,
        }
    }

    pub fn with_fallback_recommendation(
        mut self,
        fallback_recommendation: Option<FallbackRecommendation>,
    ) -> Self {
        self.fallback_recommendation = fallback_recommendation;
        self
    }

    pub fn can_parallelize(&self) -> bool {
        self.readonly_classification && self.parallel_safe_after_preflight
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreparedToolBatchKind {
    Sequential,
    ParallelReadonly,
}

#[derive(Debug, Clone)]
pub struct PreparedToolBatch {
    pub kind: PreparedToolBatchKind,
    pub calls: Vec<PreparedToolCall>,
}

impl PreparedToolBatch {
    pub fn plan_layout(
        parallelizable: impl IntoIterator<Item = bool>,
        allow_parallel: bool,
    ) -> Vec<(PreparedToolBatchKind, usize)> {
        let mut layout = Vec::new();
        let mut parallel_batch_len = 0usize;

        for can_parallelize in parallelizable {
            if allow_parallel && can_parallelize {
                parallel_batch_len += 1;
                continue;
            }

            if parallel_batch_len > 0 {
                layout.push((PreparedToolBatchKind::ParallelReadonly, parallel_batch_len));
                parallel_batch_len = 0;
            }
            layout.push((PreparedToolBatchKind::Sequential, 1));
        }

        if parallel_batch_len > 0 {
            layout.push((PreparedToolBatchKind::ParallelReadonly, parallel_batch_len));
        }

        layout
    }

    pub fn plan_layout_with_names<'a>(
        calls: impl IntoIterator<Item = (bool, &'a str)>,
        allow_parallel: bool,
    ) -> Vec<(PreparedToolBatchKind, usize)> {
        if !allow_parallel {
            return calls
                .into_iter()
                .map(|_| (PreparedToolBatchKind::Sequential, 1))
                .collect();
        }

        let mut layout = Vec::new();
        let mut parallel_batch_len = 0usize;
        let mut parallel_tool_names = HashSet::new();

        for (can_parallelize, tool_name) in calls {
            if !can_parallelize {
                push_parallel_batch_layout(&mut layout, &mut parallel_batch_len);
                parallel_tool_names.clear();
                layout.push((PreparedToolBatchKind::Sequential, 1));
                continue;
            }

            if !parallel_tool_names.insert(tool_name) {
                push_parallel_batch_layout(&mut layout, &mut parallel_batch_len);
                parallel_tool_names.clear();
                parallel_tool_names.insert(tool_name);
            }
            parallel_batch_len += 1;
        }

        push_parallel_batch_layout(&mut layout, &mut parallel_batch_len);
        layout
    }

    pub fn plan(
        calls: impl IntoIterator<Item = PreparedToolCall>,
        allow_parallel: bool,
    ) -> Vec<Self> {
        let calls: Vec<_> = calls.into_iter().collect();
        let layout = Self::plan_layout_with_names(
            calls
                .iter()
                .map(|call| (call.can_parallelize(), call.canonical_name.as_str())),
            allow_parallel,
        );
        let mut calls = calls.into_iter();

        layout
            .into_iter()
            .map(|(kind, len)| Self {
                kind,
                calls: calls.by_ref().take(len).collect(),
            })
            .collect()
    }
}

fn push_parallel_batch_layout(
    layout: &mut Vec<(PreparedToolBatchKind, usize)>,
    parallel_batch_len: &mut usize,
) {
    match *parallel_batch_len {
        0 => {}
        1 => layout.push((PreparedToolBatchKind::Sequential, 1)),
        len => layout.push((PreparedToolBatchKind::ParallelReadonly, len)),
    }
    *parallel_batch_len = 0;
}

#[derive(Debug, Clone)]
pub enum RecoveryDirective {
    Retry { delay: Option<Duration> },
    ToolFreeSynthesis { reason: String },
    SurfaceHint { message: String },
    Abort { reason: String },
}

#[derive(Debug, Clone)]
pub struct ExecutionFailure {
    pub category: vtcode_commons::ErrorCategory,
    pub retryable: bool,
    pub message: String,
    pub retry_after: Option<Duration>,
    pub directive: RecoveryDirective,
}

impl ExecutionFailure {
    pub fn from_tool_error(error: &crate::tools::registry::ToolExecutionError) -> Self {
        let retry_after = error.retry_after().or_else(|| error.retry_delay());
        let directive = if error.retryable {
            RecoveryDirective::Retry { delay: retry_after }
        } else {
            RecoveryDirective::SurfaceHint {
                message: error.user_message(),
            }
        };
        Self {
            category: error.category,
            retryable: error.retryable,
            message: error.user_message(),
            retry_after,
            directive,
        }
    }

    pub fn from_anyhow(error: &anyhow::Error) -> Self {
        let category = vtcode_commons::classify_anyhow_error(error);
        // Delegate to the canonical authority in vtcode-commons so that any new
        // retryable category added there is automatically honoured here.
        let retryable = category.is_retryable();
        let retry_after = None;
        let directive = if retryable {
            RecoveryDirective::Retry { delay: retry_after }
        } else {
            RecoveryDirective::SurfaceHint {
                message: error.to_string(),
            }
        };
        Self {
            category,
            retryable,
            message: error.to_string(),
            retry_after,
            directive,
        }
    }
}

#[derive(Debug, Clone)]
pub struct HarnessRequestPlan {
    pub request: LLMRequest,
    pub has_tools: bool,
    pub stable_prefix_hash: u64,
    pub tool_catalog_hash: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct HarnessRequestPlanInput {
    pub messages: Vec<Message>,
    pub system_prompt: String,
    pub tools: Option<Arc<Vec<ToolDefinition>>>,
    pub model: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub stream: bool,
    pub tool_choice: Option<ToolChoice>,
    pub parallel_tool_config: Option<Box<ParallelToolConfig>>,
    pub reasoning_effort: Option<ReasoningEffortLevel>,
    pub verbosity: Option<VerbosityLevel>,
    pub metadata: Option<Value>,
    pub context_management: Option<Value>,
    pub previous_response_id: Option<String>,
    pub prompt_cache_key: Option<String>,
    pub prompt_cache_profile: Option<crate::llm::provider::PromptCacheProfile>,
    pub tool_catalog_hash: Option<u64>,
}

pub fn build_harness_request_plan(input: HarnessRequestPlanInput) -> HarnessRequestPlan {
    let tools = input.tools.filter(|tools| !tools.is_empty());
    let stable_prefix_hash = stable_system_prefix_hash(&input.system_prompt);
    let tool_catalog_hash = input
        .tool_catalog_hash
        .or_else(|| hash_tool_definitions(tools.as_deref().map(Vec::as_slice)));
    let has_tools = tools.is_some();
    let request = LLMRequest {
        messages: input.messages,
        system_prompt: Some(Arc::new(input.system_prompt)),
        tools,
        model: input.model,
        max_tokens: input.max_tokens,
        temperature: input.temperature,
        stream: input.stream,
        tool_choice: input.tool_choice,
        parallel_tool_config: input.parallel_tool_config,
        reasoning_effort: input.reasoning_effort,
        verbosity: input.verbosity,
        metadata: input.metadata,
        context_management: input.context_management,
        previous_response_id: input.previous_response_id,
        prompt_cache_key: input.prompt_cache_key,
        prompt_cache_profile: input.prompt_cache_profile,
        ..Default::default()
    };

    HarnessRequestPlan {
        request,
        has_tools,
        stable_prefix_hash,
        tool_catalog_hash,
    }
}

pub fn stable_system_prefix_hash(system_prompt: &str) -> u64 {
    let stable_prefix = system_prompt
        .split("\n[Runtime Context]\n")
        .next()
        .unwrap_or(system_prompt)
        .split("\n[Context]\n")
        .next()
        .unwrap_or(system_prompt)
        .trim_end();
    hash_value(&stable_prefix)
}

pub fn hash_tool_definitions(tools: Option<&[ToolDefinition]>) -> Option<u64> {
    tools.and_then(hash_json_value)
}

pub fn should_expose_tool_in_mode(
    tool: &ToolDefinition,
    plan_mode: bool,
    request_user_input_enabled: bool,
) -> bool {
    let Some(name) = tool.function.as_ref().map(|func| func.name.as_str()) else {
        return true;
    };

    FeatureSet::tool_enabled_for_mode(name, plan_mode, request_user_input_enabled)
}

pub fn filter_tool_definitions_for_mode(
    tools: Option<Arc<Vec<ToolDefinition>>>,
    plan_mode: bool,
    request_user_input_enabled: bool,
) -> Option<Arc<Vec<ToolDefinition>>> {
    let tools = tools?;
    if tools
        .iter()
        .all(|tool| should_expose_tool_in_mode(tool, plan_mode, request_user_input_enabled))
    {
        return Some(tools);
    }

    let filtered: Vec<ToolDefinition> = tools
        .iter()
        .filter(|tool| should_expose_tool_in_mode(tool, plan_mode, request_user_input_enabled))
        .cloned()
        .collect();
    if filtered.is_empty() {
        None
    } else {
        Some(Arc::new(filtered))
    }
}

pub fn reduce_tool_result(tool_name: &str, result: Value) -> Value {
    let canonical_tool_name =
        tool_intent::canonical_unified_exec_tool_name(tool_name).unwrap_or(tool_name);
    match canonical_tool_name {
        crate::config::constants::tools::UNIFIED_SEARCH => reduce_search_result(result),
        crate::config::constants::tools::READ_FILE => reduce_read_file_result(result),
        crate::config::constants::tools::UNIFIED_EXEC => reduce_command_result(result),
        _ => result,
    }
}

fn hash_value<T: Hash>(value: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn hash_json_value<T: Serialize + ?Sized>(value: &T) -> Option<u64> {
    let mut hasher = DefaultHasher::new();
    serde_json::to_writer(HasherWriter::new(&mut hasher), value)
        .ok()
        .map(|_| {
            hasher.write_u8(0xff);
            hasher.finish()
        })
}

fn reduce_search_result(result: Value) -> Value {
    const MAX_GREP_RESULTS: usize = 5;
    const MAX_LIST_FILES: usize = 50;

    let Some(obj) = result.as_object() else {
        return result;
    };

    if let Some(matches) = obj.get("matches").and_then(Value::as_array) {
        let mut deduped = Vec::with_capacity(matches.len());
        let mut seen = HashSet::new();
        for entry in matches {
            let path = entry
                .get("path")
                .or_else(|| entry.get("file"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            let line = entry
                .get("line")
                .or_else(|| entry.get("line_number"))
                .and_then(Value::as_i64);
            if path.is_none() && line.is_none() {
                deduped.push(entry.clone());
                continue;
            }
            if seen.insert((path, line)) {
                deduped.push(entry.clone());
            }
        }
        let total = deduped.len();
        if total > MAX_GREP_RESULTS {
            return serde_json::json!({
                "matches": deduped.into_iter().take(MAX_GREP_RESULTS).collect::<Vec<_>>(),
                "overflow": format!("[+{} more matches]", total - MAX_GREP_RESULTS),
                "total": total,
                "note": "Showing top 5 unique matches (by path/line)"
            });
        }
        if total != matches.len() {
            return serde_json::json!({
                "matches": deduped,
                "total": total,
                "note": "unique grep matches (collapsed by path/line)"
            });
        }
        return serde_json::json!({
            "matches": deduped,
            "total": total,
            "note": "grep results normalized"
        });
    }

    let Some(files) = obj
        .get("files")
        .or_else(|| obj.get("items"))
        .and_then(Value::as_array)
    else {
        return result;
    };
    if files.len() <= MAX_LIST_FILES {
        return result;
    }

    serde_json::json!({
        "total_files": files.len(),
        "sample": files.iter().take(5).cloned().collect::<Vec<_>>(),
        "note": format!("Showing 5 of {} files. Use unified_search for specific patterns.", files.len())
    })
}

fn reduce_read_file_result(result: Value) -> Value {
    const MAX_FILE_LINES: usize = 2000;

    let Some(obj) = result.as_object() else {
        return result;
    };
    let Some(content) = obj.get("content").and_then(Value::as_str) else {
        return result;
    };

    let (content, is_truncated) = truncate_lines(content, MAX_FILE_LINES)
        .map(|(truncated, _)| (truncated, true))
        .unwrap_or_else(|| (content.to_string(), false));

    let mut reduced = serde_json::Map::new();
    reduced.insert("success".to_string(), Value::Bool(true));
    reduced.insert(
        "status".to_string(),
        obj.get("status")
            .cloned()
            .unwrap_or_else(|| Value::String("success".to_string())),
    );
    if let Some(message) = obj.get("message") {
        reduced.insert("message".to_string(), message.clone());
    }
    reduced.insert("content".to_string(), Value::String(content));
    if let Some(path) = obj.get("path").or_else(|| obj.get("file")) {
        reduced.insert("path".to_string(), path.clone());
    }
    if let Some(metadata) = obj.get("metadata") {
        reduced.insert("metadata".to_string(), metadata.clone());
    }
    if is_truncated {
        reduced.insert("is_truncated".to_string(), Value::Bool(true));
    }

    Value::Object(reduced)
}

fn reduce_command_result(result: Value) -> Value {
    const MAX_FILE_LINES: usize = 2000;

    let Some(obj) = result.as_object() else {
        return result;
    };
    let stream_key = if obj.get("stdout").and_then(Value::as_str).is_some() {
        "stdout"
    } else {
        "output"
    };
    let Some(stream) = obj.get(stream_key).and_then(Value::as_str) else {
        return result;
    };
    let Some((truncated, lines_count)) = truncate_lines(stream, MAX_FILE_LINES) else {
        return result;
    };

    let mut reduced = obj.clone();
    reduced.insert(stream_key.to_string(), Value::String(truncated));
    reduced.insert("is_truncated".to_string(), Value::Bool(true));
    reduced.insert(
        "original_lines".to_string(),
        Value::Number(serde_json::Number::from(lines_count as u64)),
    );
    reduced.insert(
        "note".to_string(),
        Value::String("Command output truncated for context economy.".to_string()),
    );
    Value::Object(reduced)
}

fn truncate_lines(text: &str, max_lines: usize) -> Option<(String, usize)> {
    if max_lines == 0 {
        return Some((String::new(), text.lines().count()));
    }

    let mut lines = text.lines();
    let mut total = 0usize;
    let mut out = String::new();
    while let Some(line) = lines.next() {
        total += 1;
        if total <= max_lines {
            if total > 1 {
                out.push('\n');
            }
            out.push_str(line);
            continue;
        }
        total += lines.count();
        return Some((out, total));
    }
    None
}

pub fn is_parallel_safe_tool_batch(calls: &[PreparedToolCall]) -> bool {
    calls.iter().all(PreparedToolCall::can_parallelize)
}

pub fn looks_like_grep_style_command(command: &str) -> bool {
    let lower = command.trim().to_ascii_lowercase();
    lower.starts_with("grep ")
        || lower.starts_with("rg ")
        || lower.contains("/grep ")
        || lower.contains("/rg ")
}

pub fn command_is_safe(command: &str) -> bool {
    commands::validate_command_safety(command).is_ok()
}

pub fn low_signal_attempt_key(name: &str, args: &Value) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    let mut input_len = 0usize;
    if serde_json::to_writer(HashingWriter::new(&mut hash, &mut input_len), args).is_err() {
        for byte in b"{}" {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
            input_len = input_len.saturating_add(1);
        }
    }

    format!("{name}:len{input_len}-fnv{hash:016x}")
}

struct HashingWriter<'a> {
    hash: &'a mut u64,
    input_len: &'a mut usize,
}

impl<'a> HashingWriter<'a> {
    fn new(hash: &'a mut u64, input_len: &'a mut usize) -> Self {
        Self { hash, input_len }
    }
}

impl std::io::Write for HashingWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        for byte in buf {
            *self.hash ^= u64::from(*byte);
            *self.hash = self.hash.wrapping_mul(0x100000001b3);
            *self.input_len = self.input_len.saturating_add(1);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct HasherWriter<'a, H> {
    hasher: &'a mut H,
}

impl<'a, H> HasherWriter<'a, H> {
    fn new(hasher: &'a mut H) -> Self {
        Self { hasher }
    }
}

impl<H: Hasher> std::io::Write for HasherWriter<'_, H> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.hasher.write(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::constants::tools;

    fn function_tool(name: &str) -> ToolDefinition {
        ToolDefinition::function(name.to_string(), name.to_string(), serde_json::json!({}))
    }

    #[test]
    fn request_plan_keeps_stable_prefix_hash() {
        let plan = build_harness_request_plan(HarnessRequestPlanInput {
            messages: vec![Message::user("hello".to_string())],
            system_prompt: "base\n[Runtime Context]\n- turns: 1".to_string(),
            tools: Some(Arc::new(vec![function_tool(tools::UNIFIED_SEARCH)])),
            model: "gpt-5".to_string(),
            max_tokens: Some(128),
            temperature: Some(0.7),
            stream: true,
            tool_choice: Some(ToolChoice::auto()),
            parallel_tool_config: None,
            reasoning_effort: None,
            verbosity: None,
            metadata: None,
            context_management: None,
            previous_response_id: None,
            prompt_cache_key: None,
            prompt_cache_profile: None,
            tool_catalog_hash: None,
        });

        assert!(plan.has_tools);
        assert!(plan.tool_catalog_hash.is_some());
        assert_eq!(
            plan.stable_prefix_hash,
            stable_system_prefix_hash("base\n[Runtime Context]\n- turns: 1")
        );
    }

    #[test]
    fn request_plan_drops_empty_tool_catalog() {
        let plan = build_harness_request_plan(HarnessRequestPlanInput {
            messages: vec![Message::user("hello".to_string())],
            system_prompt: "base".to_string(),
            tools: Some(Arc::new(Vec::new())),
            model: "gpt-5".to_string(),
            max_tokens: Some(128),
            temperature: Some(0.7),
            stream: true,
            tool_choice: Some(ToolChoice::auto()),
            parallel_tool_config: None,
            reasoning_effort: None,
            verbosity: None,
            metadata: None,
            context_management: None,
            previous_response_id: None,
            prompt_cache_key: None,
            prompt_cache_profile: None,
            tool_catalog_hash: None,
        });

        assert!(!plan.has_tools);
        assert!(plan.request.tools.is_none());
        assert!(plan.tool_catalog_hash.is_none());
    }

    #[test]
    fn prepared_tool_batches_group_contiguous_parallel_reads() {
        let batches = PreparedToolBatch::plan(
            vec![
                PreparedToolCall::new("read_a".to_string(), true, true, serde_json::json!({})),
                PreparedToolCall::new("read_b".to_string(), true, true, serde_json::json!({})),
                PreparedToolCall::new("edit".to_string(), false, false, serde_json::json!({})),
            ],
            true,
        );

        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].kind, PreparedToolBatchKind::ParallelReadonly);
        assert_eq!(batches[0].calls.len(), 2);
        assert_eq!(batches[1].kind, PreparedToolBatchKind::Sequential);
    }

    #[test]
    fn prepared_tool_batches_preserve_order_around_mutating_calls() {
        let batches = PreparedToolBatch::plan(
            vec![
                PreparedToolCall::new("read_a".to_string(), true, true, serde_json::json!({})),
                PreparedToolCall::new("edit".to_string(), false, false, serde_json::json!({})),
                PreparedToolCall::new("read_b".to_string(), true, true, serde_json::json!({})),
            ],
            true,
        );

        assert_eq!(batches.len(), 3);
        assert!(
            batches
                .iter()
                .all(|batch| batch.kind == PreparedToolBatchKind::Sequential)
        );
        assert_eq!(batches[0].calls[0].canonical_name, "read_a");
        assert_eq!(batches[1].calls[0].canonical_name, "edit");
        assert_eq!(batches[2].calls[0].canonical_name, "read_b");
    }

    #[test]
    fn prepared_tool_batches_split_duplicate_parallel_tool_names() {
        let batches = PreparedToolBatch::plan(
            vec![
                PreparedToolCall::new("read_file".to_string(), true, true, serde_json::json!({})),
                PreparedToolCall::new("read_file".to_string(), true, true, serde_json::json!({})),
            ],
            true,
        );

        assert_eq!(batches.len(), 2);
        assert!(
            batches
                .iter()
                .all(|batch| batch.kind == PreparedToolBatchKind::Sequential)
        );
    }

    #[test]
    fn prepared_tool_batches_serializes_all_calls_when_parallel_disabled() {
        let batches = PreparedToolBatch::plan(
            vec![
                PreparedToolCall::new("read_a".to_string(), true, true, serde_json::json!({})),
                PreparedToolCall::new("read_b".to_string(), true, true, serde_json::json!({})),
            ],
            false,
        );

        assert_eq!(batches.len(), 2);
        assert!(
            batches
                .iter()
                .all(|batch| batch.kind == PreparedToolBatchKind::Sequential)
        );
    }

    #[test]
    fn filter_tool_definitions_respects_request_user_input_toggle() {
        let tools = Arc::new(vec![
            function_tool(tools::UNIFIED_SEARCH),
            function_tool(tools::REQUEST_USER_INPUT),
        ]);

        let filtered =
            filter_tool_definitions_for_mode(Some(tools), true, false).expect("filtered tools");
        let names: Vec<&str> = filtered.iter().map(|tool| tool.function_name()).collect();

        assert!(names.contains(&tools::UNIFIED_SEARCH));
        assert!(!names.contains(&tools::REQUEST_USER_INPUT));
    }

    #[test]
    fn tool_catalog_hash_matches_legacy_json_string_hash() {
        let tools = vec![
            function_tool(tools::UNIFIED_SEARCH),
            ToolDefinition::function(
                "custom_tool".to_string(),
                "Custom".to_string(),
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "line": { "type": "integer" }
                    }
                }),
            )
            .with_strict(true)
            .with_defer_loading(true),
        ];

        let expected = serde_json::to_string(&tools)
            .ok()
            .map(|text| hash_value(&text));

        assert_eq!(hash_tool_definitions(Some(&tools)), expected);
    }

    #[test]
    fn reduce_command_result_truncates_large_output() {
        let stdout = (0..2200).map(|_| "a").collect::<Vec<_>>().join("\n");
        let reduced = reduce_tool_result(
            tools::UNIFIED_EXEC,
            serde_json::json!({
                "stdout": stdout
            }),
        );

        assert_eq!(reduced.get("is_truncated"), Some(&Value::Bool(true)));
    }
}
