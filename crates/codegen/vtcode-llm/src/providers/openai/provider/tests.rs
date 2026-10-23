use super::super::CustomProviderAuthHandle;
use super::super::backend_setup::{
    CHATGPT_CODEX_BASE, ChatGptSubscriptionAuthSource, OpenAIBackendKind, OpenAIBackendRefreshBehaviour,
    OpenAIBackendSetup,
};
use super::super::tool_serialization;
use super::*;
use crate::provider::{LLMProvider, NormalizedStreamEvent, ParallelToolConfig};
use futures::StreamExt;
use reqwest::StatusCode;
use rig::providers::chatgpt::ChatGPTAuth as RigChatGptAuth;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;
use vtcode_config::TimeoutsConfig;
use vtcode_config::auth::{AuthCredentialsStoreMode, OpenAIChatGptAuthHandle, OpenAIChatGptSession};
use vtcode_config::constants::{env_vars, urls};
use vtcode_config::core::CustomProviderCommandAuthConfig;
use vtcode_config::core::{
    CustomProviderApiFormat, OpenAIHostedShellConfig, OpenAIHostedShellDomainSecret, OpenAIHostedShellEnvironment,
    OpenAIHostedShellNetworkPolicy, OpenAIHostedShellNetworkPolicyType, OpenAIHostedSkill, OpenAIHostedSkillVersion,
    OpenAIServiceTier, PromptCacheRetention,
};
use vtcode_config::{OpenAIAuthConfig, auth::OpenAIChatGptSessionRefresher};

mod auth;
mod cache_ttl;
mod cache_usage;
mod caching;
mod compaction;
mod files;
mod history;
mod hosted_shell;
mod request_policy;
mod tools;
mod transport;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ─── Test Fixtures ───────────────────────────────────────────────────────────
struct ExternalSessionRefresher {
    calls: Arc<Mutex<usize>>,
}

#[async_trait::async_trait]
impl OpenAIChatGptSessionRefresher for ExternalSessionRefresher {
    async fn refresh_session(&self, current: &OpenAIChatGptSession) -> anyhow::Result<OpenAIChatGptSession> {
        let mut calls = self.calls.lock().expect("mutex should lock");
        *calls += 1;
        let mut refreshed = current.clone();
        refreshed.access_token = "oauth-access-refreshed".to_string();
        refreshed.refreshed_at = current.refreshed_at.saturating_add(1);
        refreshed.expires_at = None;
        Ok(refreshed)
    }
}

fn write_token_lines(dir: &std::path::Path, tokens: &[&str]) {
    std::fs::write(dir.join("tokens.txt"), tokens.join("\n")).expect("write tokens");
}
fn custom_provider_auth_fixture(dir: &TempDir, tokens: &[&str]) -> CustomProviderCommandAuthConfig {
    write_token_lines(dir.path(), tokens);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let script_path = dir.path().join("print-token.sh");
        std::fs::write(
            &script_path,
            r#"#!/bin/sh
first_line=$(sed -n '1p' tokens.txt)
printf '%s\n' "$first_line"
tail -n +2 tokens.txt > tokens.next
mv tokens.next tokens.txt
"#,
        )
        .expect("write script");
        let mut perms = std::fs::metadata(&script_path).expect("metadata").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script_path, perms).expect("set permissions");
        CustomProviderCommandAuthConfig {
            command: "./print-token.sh".to_string(),
            args: Vec::new(),
            cwd: Some(dir.path().to_path_buf()),
            timeout_ms: 5_000,
            refresh_interval_ms: 60_000,
        }
    }
    #[cfg(windows)]
    {
        let script_path = dir.path().join("print-token.ps1");
        std::fs::write(
            &script_path,
            r#"$lines = Get-Content -Path tokens.txt
if ($lines.Count -eq 0) { exit 1 }
Write-Output $lines[0]
$lines | Select-Object -Skip 1 | Set-Content -Path tokens.txt
"#,
        )
        .expect("write script");
        CustomProviderCommandAuthConfig {
            command: "powershell".to_string(),
            args: vec![
                "-NoProfile".to_string(),
                "-ExecutionPolicy".to_string(),
                "Bypass".to_string(),
                "-File".to_string(),
                script_path.to_string_lossy().into_owned(),
            ],
            cwd: Some(dir.path().to_path_buf()),
            timeout_ms: 5_000,
            refresh_interval_ms: 60_000,
        }
    }
}

use crate::providers::test_support::start_mock_server_or_skip;

// ─── Helper constructors ─────────────────────────────────────────────────────
fn tool_def(name: &str, desc: &str, schema: Value) -> provider::ToolDefinition {
    provider::ToolDefinition::function(name.to_owned(), desc.to_owned(), schema)
}

fn sample_tool() -> provider::ToolDefinition {
    tool_def(
        "search_workspace",
        "Search project files",
        json!({"type":"object","properties":{"query":{"type":"string"}},"required":["query"],"additionalProperties":false}),
    )
}

fn shell_tool() -> provider::ToolDefinition {
    tool_def(
        "shell",
        "Execute a shell command and return its output.",
        json!({"type":"object","properties":{"command":{"type":"string"}},"required":["command"],"additionalProperties":false}),
    )
}

fn sample_request(model: &str) -> provider::LLMRequest {
    provider::LLMRequest {
        messages: vec![provider::Message::user("Hello".to_owned())].into(),
        tools: Some(Arc::new(vec![sample_tool()])),
        model: model.to_string(),
        ..Default::default()
    }
}

fn shell_request(model: &str) -> provider::LLMRequest {
    provider::LLMRequest {
        messages: vec![provider::Message::user("Run pwd".to_owned())].into(),
        tools: Some(Arc::new(vec![shell_tool()])),
        model: model.to_string(),
        ..Default::default()
    }
}

fn test_provider(base_url: &str, model: &str) -> OpenAIProvider {
    let http_client = reqwest::Client::builder().no_proxy().build().expect("test client should build");
    OpenAIProvider::new_with_client(
        "test-key".to_string(),
        None,
        model.to_string(),
        http_client,
        base_url.to_string(),
        TimeoutsConfig::default(),
    )
}

fn native_openai_mock_base_url(server: &MockServer) -> String {
    server.uri().replacen("http://", "http://api.openai.com@", 1)
}

fn chatgpt_mock_base_url(server: &MockServer) -> String {
    server.uri().replacen("http://", "http://chatgpt.com@", 1)
}

fn sample_chatgpt_auth_handle() -> OpenAIChatGptAuthHandle {
    OpenAIChatGptAuthHandle::new(
        OpenAIChatGptSession {
            openai_api_key: String::new(),
            id_token: "id-token".to_string(),
            access_token: "oauth-access".to_string(),
            refresh_token: "refresh-token".to_string(),
            account_id: Some("acc_123".to_string()),
            email: Some("test@example.com".to_string()),
            plan: Some("plus".to_string()),
            obtained_at: 1,
            refreshed_at: u64::MAX / 2,
            expires_at: None,
        },
        OpenAIAuthConfig::default(),
        AuthCredentialsStoreMode::File,
    )
}

// ─── Config builders ─────────────────────────────────────────────────────────
fn priority_openai_config() -> OpenAIConfig {
    OpenAIConfig {
        service_tier: Some(OpenAIServiceTier::Priority),
        ..Default::default()
    }
}

fn flex_openai_config() -> OpenAIConfig {
    OpenAIConfig {
        service_tier: Some(OpenAIServiceTier::Flex),
        ..Default::default()
    }
}

fn hosted_shell_openai_config() -> OpenAIConfig {
    OpenAIConfig {
        hosted_shell: OpenAIHostedShellConfig {
            enabled: true,
            environment: OpenAIHostedShellEnvironment::ContainerAuto,
            container_id: None,
            file_ids: vec!["file_123".to_string()],
            skills: vec![OpenAIHostedSkill::SkillReference {
                skill_id: "skill_123".to_string(),
                version: OpenAIHostedSkillVersion::default(),
            }],
            network_policy: OpenAIHostedShellNetworkPolicy::default(),
        },
        ..Default::default()
    }
}

fn chatgpt_backend_provider(model: &str) -> OpenAIProvider {
    OpenAIProvider::from_config(
        Some(String::new()),
        Some(sample_chatgpt_auth_handle()),
        Some(model.to_string()),
        None,
        None,
        None,
        None,
        None,
        None,
    )
}

fn native_openai_provider(model: &str) -> OpenAIProvider {
    OpenAIProvider::with_model(String::new(), model.to_string())
}

fn compatible_endpoint_provider(model: &str, base_url: &str) -> OpenAIProvider {
    OpenAIProvider::from_config(
        Some(String::new()),
        None,
        Some(model.to_string()),
        Some(base_url.to_string()),
        None,
        None,
        None,
        None,
        None,
    )
}
// ─── Assertion helpers ───────────────────────────────────────────────────────

fn assert_str_field(value: &Value, key: &str, expected: &str) {
    assert_eq!(value.get(key).and_then(Value::as_str), Some(expected), "field '{key}' mismatch");
}

fn assert_str_field_obj(obj: &serde_json::Map<String, Value>, key: &str, expected: &str) {
    assert_eq!(obj.get(key).and_then(Value::as_str), Some(expected), "field '{key}' mismatch");
}

fn assert_absent(value: &Value, key: &str) {
    assert!(value.get(key).is_none(), "field '{key}' should be absent");
}

fn without_responses_backend_fields(mut payload: Value) -> Value {
    let map = payload.as_object_mut().expect("responses payload should be an object");
    for field in [
        "include",
        "output_types",
        "previous_response_id",
        "prompt_cache_options",
        "prompt_cache_retention",
        "sampling_parameters",
        "store",
        "text",
    ] {
        map.remove(field);
    }
    payload
}

fn without_responses_backend_capability_fields(mut payload: Value) -> Value {
    let map = payload.as_object_mut().expect("responses payload should be an object");
    for field in [
        "include",
        "output_types",
        "prompt_cache_options",
        "prompt_cache_retention",
        "sampling_parameters",
        "text",
    ] {
        map.remove(field);
    }
    payload
}

fn without_native_cache_projection(mut payload: Value) -> Value {
    if let Some(items) = payload.get_mut("input").and_then(Value::as_array_mut) {
        // Compare shared history after checking the intentional native/legacy
        // reminder-role difference separately at the call site.
        if let Some(reminder) = items.last_mut()
            && reminder
                .pointer("/content/0/text")
                .and_then(Value::as_str)
                .is_some_and(|text| text.starts_with("[System reminder — runtime context, not a user request]"))
        {
            reminder["role"] = json!("user");
        }
        for item in items {
            if let Some(blocks) = item.get_mut("content").and_then(Value::as_array_mut) {
                for block in blocks {
                    if let Some(map) = block.as_object_mut() {
                        map.remove("prompt_cache_breakpoint");
                    }
                }
            }
        }
    }
    payload
}

fn count_input_breakpoints(payload: &Value) -> usize {
    payload
        .get("input")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("content").and_then(Value::as_array))
                .flat_map(|blocks| blocks.iter())
                .filter(|block| block.get("prompt_cache_breakpoint").is_some())
                .count()
        })
        .unwrap_or(0)
}

fn get_input_array(payload: &Value) -> &[Value] {
    payload
        .get("input")
        .and_then(Value::as_array)
        .expect("input array should exist")
}

fn input_role_at(payload: &Value, index: usize) -> Option<&str> {
    get_input_array(payload)
        .get(index)
        .and_then(|v| v.get("role"))
        .and_then(Value::as_str)
}

fn input_type_at(payload: &Value, index: usize) -> Option<&str> {
    get_input_array(payload)
        .get(index)
        .and_then(|v| v.get("type"))
        .and_then(Value::as_str)
}

fn input_call_id_at(payload: &Value, index: usize) -> Option<&str> {
    get_input_array(payload)
        .get(index)
        .and_then(|v| v.get("call_id"))
        .and_then(Value::as_str)
}

fn responses_payload_for(model: &str, provider: &OpenAIProvider) -> Value {
    provider
        .convert_to_openai_responses_format(&sample_request(model))
        .expect("conversion should succeed")
}

fn responses_allowed_tools_request(model: &str) -> provider::LLMRequest {
    provider::LLMRequest {
        messages: vec![provider::Message::user("Hello".to_owned())].into(),
        tools: Some(Arc::new(vec![
            sample_tool(),
            provider::ToolDefinition::web_search(json!({})),
            provider::ToolDefinition::file_search(json!({"vector_store_ids":["vs_123"]})),
        ])),
        tool_choice: Some(provider::ToolChoice::allowed_tools_auto(vec![
            "web_search".to_string(),
            "search_workspace".to_string(),
        ])),
        model: model.to_string(),
        ..Default::default()
    }
}

fn chat_payload_for(model: &str, provider: &OpenAIProvider) -> Value {
    provider
        .convert_to_openai_format(&sample_request(model))
        .expect("conversion should succeed")
}

fn mock_retry_by_call_count(seen_tokens: Arc<Mutex<Vec<String>>>) -> impl Fn(&wiremock::Request) -> ResponseTemplate {
    move |request: &wiremock::Request| {
        let bearer = request
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .expect("authorization header required")
            .to_string();
        seen_tokens.lock().expect("mutex not poisoned").push(bearer);
        let count = seen_tokens.lock().expect("mutex not poisoned").len();
        match count {
            1 => ResponseTemplate::new(401),
            2 => ResponseTemplate::new(200),
            n => ResponseTemplate::new(500).set_body_string(format!("unexpected retry count: {n}")),
        }
    }
}

fn mock_service_tier_fallback(
    seen: Arc<Mutex<Vec<Option<String>>>>,
    success_body: Value,
) -> impl Fn(&wiremock::Request) -> ResponseTemplate {
    move |request: &wiremock::Request| {
        let payload: Value = serde_json::from_slice(&request.body).expect("valid json body");
        let tier = payload.get("service_tier").and_then(Value::as_str).map(ToOwned::to_owned);
        seen.lock().expect("mutex not poisoned").push(tier.clone());
        match tier.as_deref() {
            Some("flex") => ResponseTemplate::new(400).set_body_json(json!({
                "error": {"message": "Flex is not available for this model.", "type": "invalid_request_error"}
            })),
            None => ResponseTemplate::new(200).set_body_json(success_body.clone()),
            other => ResponseTemplate::new(500).set_body_string(format!("unexpected service tier: {other:?}")),
        }
    }
}

fn schema_keyword_path(value: &Value, keywords: &[&str], path: &str) -> Option<String> {
    match value {
        Value::Object(map) => {
            let properties_object = path.ends_with(".properties");
            if !properties_object {
                for keyword in keywords {
                    if map.contains_key(*keyword) {
                        return Some(format!("{path}.{keyword}"));
                    }
                }
            }
            for (key, nested) in map {
                if let Some(found) = schema_keyword_path(nested, keywords, &format!("{path}.{key}")) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => items
            .iter()
            .enumerate()
            .find_map(|(i, nested)| schema_keyword_path(nested, keywords, &format!("{path}[{i}]"))),
        _ => None,
    }
}

// ─── Auth & Retry Tests ──────────────────────────────────────────────────────

fn chat_completion_response_body(text: &str) -> Value {
    json!({
        "id": "chatcmpl-test",
        "object": "chat.completion",
        "created": 0,
        "model": "test-model",
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": text },
            "finish_reason": "stop"
        }],
        "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
    })
}

fn responses_api_response_body(text: &str) -> Value {
    json!({
        "id": "resp_test",
        "object": "response",
        "model": "test-model",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": text
            }]
        }],
        "usage": { "input_tokens": 1, "output_tokens": 1 }
    })
}

// ─── Tool Serialization Tests ────────────────────────────────────────────────

// ─── Chat Completions Payload Tests ──────────────────────────────────────────

// ─── Responses API Payload Tests ─────────────────────────────────────────────

// ─── Hosted Shell Tests ──────────────────────────────────────────────────────

// Hosted shell fallback: keeps local shell tool when conditions aren't met

// ─── Validation & Schema Tests ───────────────────────────────────────────────

// NOTE: responses_function_tools_sanitize_openai_incompatible_parameter_keywords and
// responses_function_tools_strip_openai_schema_combinators_from_builtin_tools were removed
// because they required vtcode-core::tools types which are not available in vtcode-llm.
// If this test coverage is needed, move them to vtcode-core.

// ─── Specialized Tool Types ──────────────────────────────────────────────────

// ─── ChatGPT Backend History Tests ───────────────────────────────────────────

// Helper to build ChatGPT backend history payload
fn chatgpt_codex_payload(messages: Vec<provider::Message>, model: &str) -> Value {
    let provider = chatgpt_backend_provider(model);
    let request = provider::LLMRequest {
        messages: messages.into(),
        model: model.to_string(),
        ..Default::default()
    };
    provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed")
}

// Parametrized phase omission tests for ChatGPT backend models

// Phase behavior: native OpenAI includes assistant phase, non-native omits it

// ─── ChatGPT Backend Omissions ───────────────────────────────────────────────

// ─── Compaction & Responses API State ────────────────────────────────────────

// ─── Supported Models & Harmony ──────────────────────────────────────────────

// ─── Prompt Cache & Websocket ────────────────────────────────────────────────

use vtcode_config::core::PromptCachingConfig;

// ─── Max Tokens & Reasoning ──────────────────────────────────────────────────

// ─── Streaming Tests ─────────────────────────────────────────────────────────

// ─── Harmony Parsing ─────────────────────────────────────────────────────────

// ─── Retry & Fallback Tests ──────────────────────────────────────────────────

// ─── Request Metadata & Content Type ─────────────────────────────────────────

// ─── Manual Compaction ───────────────────────────────────────────────────────

// ─── Debug redaction for OpenAIRequestAuth ───────────────────────────────────
