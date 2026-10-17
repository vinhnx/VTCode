use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::fmt::Write;
use std::sync::Arc;
use vtcode_commons::is_context_capacity_message;
use vtcode_commons::llm::FinishReason;
use vtcode_config::constants::context::DEFAULT_COMPACTION_TRIGGER_RATIO;

use crate::config::types::{ReasoningEffortLevel, VerbosityLevel};
use crate::exec::events::CompactionMode;
use crate::llm::reasoning_effort::ReasoningEffortMapper;
use crate::llm::utils::truncate_to_token_limit;
use crate::llm::{
    collect_single_response,
    provider::{
        LLMProvider, LLMRequest, LLMResponse, Message, MessageContent, MessageRole, ResponsesCompactionOptions,
        ToolChoice, ToolDefinition,
    },
};

pub mod auto;
pub mod memory_envelope;
pub mod prefire;
pub mod two_pass;

pub use crate::compaction::memory_envelope::{effective_context_budget, effective_session_context_budget};
pub use crate::compaction::prefire::{AsyncCompactionCache, PrefireState};

pub const SUPPRESS_NONE: u8 = 0;
pub const SUPPRESS_TURN: u8 = 1;
pub const SUPPRESS_STICKY: u8 = 2;
pub const SUPPRESS_UNTIL_SUCCESS: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SuppressReason {
    CreditBlock,
    Size,
    Auth,
    Schema,
    Other,
}

impl SuppressReason {
    fn suppress_state(self) -> u8 {
        match self {
            SuppressReason::Size | SuppressReason::Schema => SUPPRESS_STICKY,
            SuppressReason::CreditBlock | SuppressReason::Auth => SUPPRESS_UNTIL_SUCCESS,
            SuppressReason::Other => SUPPRESS_TURN,
        }
    }
}

/// Classify a deterministic compaction failure's error text into a fixed
/// [`SuppressReason`] (drives telemetry + sticky-vs-per-turn scope).
pub(crate) fn classify_suppress_reason(error_msg: &str) -> SuppressReason {
    let m = error_msg.to_ascii_lowercase();
    if m.contains("spending-limit")
        || m.contains("spending limit")
        || m.contains("out of credits")
        || m.contains("usage balance exhausted")
        || m.contains("usage limit reached")
    {
        SuppressReason::CreditBlock
    } else if m.contains("context length") || m.contains("too many tokens") {
        SuppressReason::Size
    } else if m.contains("status 401") || m.contains("unauthorized") {
        SuppressReason::Auth
    } else if m.contains("invalid_request_error") {
        SuppressReason::Schema
    } else {
        SuppressReason::Other
    }
}

mod config;
mod continuity;
mod history_bounds;
mod history_build;
mod native_inline;
mod retention;
mod strategy;
mod summarize;

pub use config::*;
pub use continuity::*;
pub(crate) use history_bounds::*;
pub(crate) use history_build::*;
pub(crate) use retention::*;
pub use strategy::*;
pub use summarize::*;

#[cfg(test)]
mod local_summary_budget_tests;

#[cfg(test)]
mod summarization_fork_bounds_tests;

#[cfg(test)]
mod tests;
