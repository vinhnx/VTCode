//! Harness request plan construction.
//!
//! Builds the LLM request plan with tool definitions, system prompt hashing,
//! and cache key management for efficient prompt caching.

use std::sync::Arc;

use serde_json::Value;

use crate::config::types::{ReasoningEffortLevel, VerbosityLevel};
use crate::llm::provider::{LLMRequest, Message, ParallelToolConfig, ToolChoice, ToolDefinition};

use super::harness_kernel::{hash_tool_definitions, stable_system_prefix_hash};

/// A prepared LLM request with metadata for the harness loop.
#[derive(Debug, Clone)]
pub struct HarnessRequestPlan {
    pub request: LLMRequest,
    pub has_tools: bool,
    pub stable_prefix_hash: u64,
    pub tool_catalog_hash: Option<u64>,
}

/// Input parameters for building a harness request plan.
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
    /// Precomputed stable prefix hash of `system_prompt`. When supplied (the
    /// normal path, from the memoized prompt bundle) the per-turn
    /// `build_harness_request_plan` skips re-hashing the full system prompt.
    pub system_prompt_prefix_hash: Option<u64>,
}

/// Build a harness request plan from the given input parameters.
pub fn build_harness_request_plan(input: HarnessRequestPlanInput) -> HarnessRequestPlan {
    let tools = input.tools.filter(|tools| !tools.is_empty());
    let stable_prefix_hash = input
        .system_prompt_prefix_hash
        .unwrap_or_else(|| stable_system_prefix_hash(&input.system_prompt));
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
