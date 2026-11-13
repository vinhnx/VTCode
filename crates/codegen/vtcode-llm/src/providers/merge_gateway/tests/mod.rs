#![allow(
    clippy::disallowed_methods,
    reason = "test fixtures talk to a local mock server; the shared timeout budget does not apply"
)]
pub(super) use super::*;
pub(super) use crate::provider::{LLMProvider, LLMRequest, Message, NormalizedStreamEvent, ToolCall, ToolChoice};
pub(super) use futures::StreamExt;
pub(super) use serde_json::json;
pub(super) use std::sync::Arc;
pub(super) use vtcode_config::TimeoutsConfig;
pub(super) use vtcode_config::constants::models;
pub(super) use vtcode_utility_tool_specs::{apply_patch_parameters, write_stdin_parameters};
pub(super) use wiremock::matchers::{method, path};
pub(super) use wiremock::{Mock, MockServer, ResponseTemplate};

fn test_provider(base_url: &str) -> MergeGatewayProvider {
    MergeGatewayProvider::new_with_client(
        "test-key".to_string(),
        models::merge_gateway::DEFAULT_ROUTING.to_string(),
        HttpClient::new(),
        base_url.to_string(),
        TimeoutsConfig::default(),
    )
}

fn sse(event: &str, data: Value) -> String {
    format!("event: {event}\ndata: {}\n\n", serde_json::to_string(&data).expect("event payload"))
}

fn sse_data(data: Value) -> String {
    format!("data: {}\n\n", serde_json::to_string(&data).expect("event payload"))
}

mod capability;
mod payload;
mod usage_stream;
