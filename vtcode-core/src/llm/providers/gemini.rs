#![allow(clippy::collapsible_if, clippy::result_large_err)]

use crate::config::TimeoutsConfig;
use crate::config::constants::{env_vars, models, urls};
use crate::config::core::{
    AnthropicConfig, GeminiPromptCacheMode, GeminiPromptCacheSettings, PromptCachingConfig,
};
use crate::gemini::function_calling::{
    FunctionCall as GeminiFunctionCall, FunctionCallingConfig, FunctionResponse,
};
use crate::gemini::models::SystemInstruction;
use crate::gemini::streaming::{
    StreamingCandidate, StreamingConfig, StreamingError, StreamingProcessor, StreamingResponse,
};
use crate::gemini::{
    Candidate, Content, FunctionDeclaration, GenerateContentRequest, GenerateContentResponse, Part,
    Tool, ToolConfig,
};
use crate::llm::client::LLMClient;
use crate::llm::error_display;
use crate::llm::provider::{
    FinishReason, FunctionCall, LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream,
    LLMStreamEvent, Message, MessageContent, MessageRole, ToolCall, ToolChoice,
};
use crate::llm::types as llm_types;
use async_stream::try_stream;
use async_trait::async_trait;
use reqwest::Client as HttpClient;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing;
use vtcode_config::types::ReasoningEffortLevel;

use super::common::{extract_prompt_cache_settings, override_base_url, resolve_model};
use super::error_handling::{format_network_error, format_parse_error, is_rate_limit_error};

mod provider;
mod llm_provider;
mod helpers;
mod sanitize;
mod client;
#[cfg(test)]
mod tests;

pub use provider::GeminiProvider;
pub use sanitize::sanitize_function_parameters;
