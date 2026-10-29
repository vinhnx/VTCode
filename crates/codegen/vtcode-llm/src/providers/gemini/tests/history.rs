use super::*;

#[test]
fn convert_to_gemini_request_replays_preserved_raw_parts() {
    let provider = GeminiProvider::new("test-key".to_string());
    let preserved_parts = vec![
        Part::ToolCall {
            tool_call: ServerToolCall {
                tool_type: "GOOGLE_SEARCH_WEB".to_string(),
                args: Some(json!({ "queries": ["northernmost city in the United States"] })),
                id: Some("search_1".to_string()),
            },
            thought_signature: Some("tool_sig".to_string()),
        },
        Part::ToolResponse {
            tool_response: ServerToolResponse {
                tool_type: "GOOGLE_SEARCH_WEB".to_string(),
                response: json!({ "search_suggestions": ["Utqiaġvik, Alaska"] }),
                id: Some("search_1".to_string()),
            },
            thought_signature: Some("tool_sig".to_string()),
        },
        Part::FunctionCall {
            function_call: GeminiFunctionCall {
                name: "get_weather".to_string(),
                args: json!({ "city": "Utqiaġvik, Alaska" }),
                id: Some("call_weather".to_string()),
            },
            thought_signature: Some("function_sig".to_string()),
        },
    ];

    let assistant_message = Message::assistant_with_tools(
        String::new(),
        vec![ToolCall::function(
            "call_weather".to_string(),
            "get_weather".to_string(),
            json!({ "city": "Utqiaġvik, Alaska" }).to_string(),
        )],
    )
    .with_reasoning_details(Some(vec![json!(format!(
        "__vtcode_gemini_parts__:{}",
        serde_json::to_string(&preserved_parts).expect("serialize preserved parts")
    ))]));

    let request = LLMRequest {
        messages: vec![
            Message::user("Find the northernmost city and weather".to_string()),
            assistant_message,
            Message::tool_response(
                "call_weather".to_string(),
                json!({ "response": "Very cold. 22 degrees Fahrenheit." }).to_string(),
            ),
        ]
        .into(),
        tools: Some(Arc::new(vec![
            ToolDefinition::web_search(json!({})),
            ToolDefinition::function(
                "get_weather".to_string(),
                "Get the weather for a city".to_string(),
                json!({
                    "type": "object",
                    "properties": {
                        "city": { "type": "string" }
                    },
                    "required": ["city"]
                }),
            ),
        ])),
        model: models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
        ..Default::default()
    };

    let gemini_request = provider.convert_to_gemini_request(&request).expect("conversion should succeed");

    assert_eq!(gemini_request.contents.len(), 3);
    assert_eq!(gemini_request.contents[1].parts.len(), preserved_parts.len());
    assert!(matches!(
        &gemini_request.contents[1].parts[0],
        Part::ToolCall {
            tool_call,
            thought_signature
        } if tool_call.tool_type == "GOOGLE_SEARCH_WEB"
            && tool_call.id.as_deref() == Some("search_1")
            && thought_signature.as_deref() == Some("tool_sig")
    ));
    assert!(matches!(
        &gemini_request.contents[1].parts[2],
        Part::FunctionCall {
            function_call,
            thought_signature
        } if function_call.name == "get_weather"
            && function_call.id.as_deref() == Some("call_weather")
            && thought_signature.as_deref() == Some("function_sig")
    ));
}
#[cfg(test)]
mod caching_tests {
    use super::*;
    use vtcode_config::core::{GeminiPromptCacheMode, PromptCachingConfig};

    #[test]
    fn test_gemini_prompt_cache_settings() {
        // Test 1: Defaults (Implicit mode)
        let _provider = GeminiProvider::new("test-key".to_string());
        // Default is explicit caching disabled, implicit is enabled by default in provider logic if config is default?
        // Let's check from_config
        let config = PromptCachingConfig::default();
        let provider = GeminiProvider::from_config(Some("key".into()), None, None, Some(config), None, None, None);

        // Verification: we can't easily inspect private fields without a helper or reflection.
        // We can check if `convert_to_gemini_request` works.
        let request = LLMRequest {
            messages: vec![Message::user("Hello".to_string())].into(),
            model: "gemini-1.5-pro".to_string(),
            ..Default::default()
        };
        let res = provider.convert_to_gemini_request(&request);
        res.unwrap();
    }

    #[test]
    fn test_gemini_explicit_mode_config() {
        let mut config = PromptCachingConfig { enabled: true, ..Default::default() };
        config.providers.gemini.enabled = true;
        config.providers.gemini.mode = GeminiPromptCacheMode::Explicit;
        config.providers.gemini.explicit_ttl_seconds = Some(1200);

        let provider =
            GeminiProvider::from_config(Some("key".into()), None, None, Some(config.clone()), None, None, None);

        // Trigger request creation. It shouldn't panic or fail, even if explicit logic is placeholder.
        let request = LLMRequest {
            messages: vec![Message::user("Hello".to_string())].into(),
            model: "gemini-1.5-pro".to_string(),
            ..Default::default()
        };
        let res = provider.convert_to_gemini_request(&request);
        assert!(res.is_ok(), "Request conversion should succeed");

        // Verify the request conversion produces correct structure with explicit TTL
        let gemini_req = res.expect("request conversion");

        assert!(!gemini_req.contents.is_empty(), "Contents should not be empty");
        // Fail-safe: explicit mode emits the implicit text-only wire shape.
        // An inline `ttlSeconds` part is not valid generateContent schema
        // (`systemInstruction` accepts text only); true explicit caching needs
        // the separate cachedContents lifecycle, which is unimplemented.
        let system_str = serde_json::to_string(&gemini_req.system_instruction).unwrap_or_default();
        assert!(!system_str.contains("ttlSeconds"), "no inline TTL part on the wire");
        assert!(gemini_req.system_instruction.is_some(), "System instruction should be set");
    }

    /// Explicit-cache provider pointed at a mock server.
    fn explicit_cache_provider(server_uri: &str, model: &str) -> GeminiProvider {
        let mut config = PromptCachingConfig { enabled: true, ..Default::default() };
        config.providers.gemini.enabled = true;
        config.providers.gemini.mode = GeminiPromptCacheMode::Explicit;
        config.providers.gemini.explicit_ttl_seconds = Some(1200);
        GeminiProvider::from_config(
            Some("test-key".to_string()),
            Some(model.to_string()),
            Some(server_uri.to_string()),
            Some(config),
            None,
            None,
            None,
        )
    }

    use crate::providers::test_support::start_mock_server_or_skip;

    /// First attempt carries `cachedContent` and gets a stale-cache 404; the
    /// uncached retry gets a minimal SSE success.
    fn stale_cache_then_stream(request: &wiremock::Request) -> wiremock::ResponseTemplate {
        let body: Value = serde_json::from_slice(&request.body).unwrap_or(Value::Null);
        if body.get("cachedContent").is_some() {
            return wiremock::ResponseTemplate::new(404).set_body_json(json!({
                "error": {
                    "code": 404,
                    "message": "CachedContent not found: cachedContents/vtcode-test",
                    "status": "NOT_FOUND"
                }
            }));
        }
        wiremock::ResponseTemplate::new(200)
            .insert_header("content-type", "text/event-stream")
            .set_body_raw("data: {\"candidates\":[]}\n\n", "text/event-stream")
    }

    /// Explicit-cache creation succeeds against `{base}/cachedContents`.
    fn cached_contents_mock() -> wiremock::Mock {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};

        Mock::given(method("POST"))
            .and(path("/cachedContents"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "name": "cachedContents/vtcode-test" })))
    }

    fn stream_attempts(requests: &[wiremock::Request]) -> Vec<&wiremock::Request> {
        requests
            .iter()
            .filter(|request| request.url.path().ends_with(":streamGenerateContent"))
            .collect()
    }

    #[tokio::test]
    async fn stream_recovers_from_stale_explicit_cache_with_uncached_retry() {
        use wiremock::Mock;
        use wiremock::matchers::{method, path};

        let Some(server) = start_mock_server_or_skip().await else {
            return;
        };
        let model = models::google::GEMINI_3_FLASH_PREVIEW;

        cached_contents_mock().mount(&server).await;
        Mock::given(method("POST"))
            .and(path(format!("/models/{model}:streamGenerateContent")))
            .respond_with(stale_cache_then_stream)
            .mount(&server)
            .await;

        let provider = explicit_cache_provider(&server.uri(), model);
        let request = LLMRequest {
            model: model.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            ..Default::default()
        };
        if let Err(err) = provider.stream(request).await {
            panic!("a stale cachedContent must not fail the streaming turn: {err}");
        }

        let requests = server.received_requests().await.expect("received requests");
        let attempts = stream_attempts(&requests);
        assert_eq!(attempts.len(), 2, "the stale cache name must be retried exactly once");
        let cached: Value = serde_json::from_slice(&attempts[0].body).expect("cached attempt body");
        let retried: Value = serde_json::from_slice(&attempts[1].body).expect("retry body");
        assert_eq!(cached["cachedContent"], "cachedContents/vtcode-test");
        assert!(cached.get("systemInstruction").is_none(), "the cached attempt supplies the prefix from the cache");
        assert!(cached.get("toolConfig").is_none(), "the API rejects toolConfig alongside cachedContent");
        assert!(retried.get("cachedContent").is_none(), "the retry must not reuse the expired cache name");
        assert!(retried.get("systemInstruction").is_some(), "the retry must resend the full system instruction");
        assert!(
            provider.explicit_cache.current().is_none(),
            "the dead cache slot must be cleared so the next turn rebuilds it"
        );
    }

    #[tokio::test]
    async fn non_stale_stream_failure_is_not_retried_without_cache() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};

        let Some(server) = start_mock_server_or_skip().await else {
            return;
        };
        let model = models::google::GEMINI_3_FLASH_PREVIEW;

        cached_contents_mock().mount(&server).await;
        Mock::given(method("POST"))
            .and(path(format!("/models/{model}:streamGenerateContent")))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "error": { "code": 400, "message": "Invalid request: unsupported generationConfig field" }
            })))
            .mount(&server)
            .await;

        let provider = explicit_cache_provider(&server.uri(), model);
        let request = LLMRequest {
            model: model.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            ..Default::default()
        };
        assert!(provider.stream(request).await.is_err(), "a non-stale 400 must surface as an error");

        let requests = server.received_requests().await.expect("received requests");
        assert_eq!(stream_attempts(&requests).len(), 1, "only stale-cache failures may trigger the uncached retry");
        assert!(
            provider.explicit_cache.current().is_some(),
            "an unrelated failure must keep the cache slot for the next request"
        );
    }

    /// The API forbids `toolConfig` next to `cachedContent`, so a request that
    /// constrains tool use must keep the implicit shape and send the config
    /// itself; otherwise the constraint is silently lost. The segment entry
    /// must survive that skip so later default-config turns reuse it.
    #[tokio::test]
    async fn constrained_tool_choice_skips_cache_without_losing_the_segment_entry() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};

        let Some(server) = start_mock_server_or_skip().await else {
            return;
        };
        let model = models::google::GEMINI_3_FLASH_PREVIEW;

        cached_contents_mock().mount(&server).await;
        Mock::given(method("POST"))
            .and(path(format!("/models/{model}:streamGenerateContent")))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_raw("data: {\"candidates\":[]}\n\n", "text/event-stream"),
            )
            .mount(&server)
            .await;

        let provider = explicit_cache_provider(&server.uri(), model);
        let default_request = |tool_choice: Option<ToolChoice>| LLMRequest {
            model: model.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            tools: Some(Arc::new(vec![ToolDefinition::function(
                "search_workspace".to_string(),
                "Search project files".to_string(),
                json!({
                    "type": "object",
                    "properties": { "query": { "type": "string" } },
                    "required": ["query"]
                }),
            )])),
            tool_choice,
            ..Default::default()
        };

        // 1. A default-config turn installs the segment entry.
        if let Err(err) = provider.stream(default_request(None)).await {
            panic!("the default-config turn must succeed: {err}");
        }
        let installed = provider
            .explicit_cache
            .current()
            .expect("the default-config turn installs a segment entry")
            .0;
        assert_eq!(installed, "cachedContents/vtcode-test");

        // 2. The constrained turn keeps its config on the body and must not
        //    clear or replace the installed entry.
        if let Err(err) = provider.stream(default_request(Some(ToolChoice::None))).await {
            panic!("a constrained tool choice must stay enforceable on the wire: {err}");
        }
        assert_eq!(
            provider.explicit_cache.current().map(|(name, _)| name).as_deref(),
            Some(installed.as_str()),
            "skipping the cache must not lose the segment entry"
        );

        // 3. The next default-config turn reuses that same entry.
        if let Err(err) = provider.stream(default_request(None)).await {
            panic!("the following default-config turn must succeed: {err}");
        }

        let requests = server.received_requests().await.expect("received requests");
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.url.path().ends_with("/cachedContents"))
                .count(),
            1,
            "only the default-config turn may create the cache"
        );
        let attempts = stream_attempts(&requests);
        assert_eq!(attempts.len(), 3, "each turn issues exactly one stream request");
        let body = |index: usize| -> Value { serde_json::from_slice(&attempts[index].body).expect("stream body") };
        assert_eq!(body(0)["cachedContent"], "cachedContents/vtcode-test");
        assert!(body(1).get("cachedContent").is_none(), "the constraint cannot travel alongside cachedContent");
        assert_eq!(
            body(1)["toolConfig"]["functionCallingConfig"]["mode"],
            "NONE",
            "the disabled-tool constraint must reach the provider"
        );
        assert_eq!(
            body(2)["cachedContent"],
            "cachedContents/vtcode-test",
            "the following default-config turn must reuse the surviving entry"
        );
    }
}

#[test]
fn grown_history_keeps_contents_prefix_stable() {
    // Implicit caching keys on the request prefix: a grown second turn must
    // extend `contents` without rewriting earlier turns, and the system
    // instruction must repeat verbatim.
    let provider = GeminiProvider::new("test-key".to_string());
    let first_turn = vec![
        Message::user("list files".to_string()),
        Message::assistant_with_tools(
            String::new(),
            vec![ToolCall::function(
                "call_1".to_string(),
                "list_files".to_string(),
                json!({ "path": "." }).to_string(),
            )],
        ),
        Message::tool_response("call_1".to_string(), "a.txt".to_string()),
    ];
    let mut second_turn = first_turn.clone();
    second_turn.push(Message::user("read a.txt".to_string()));

    let build = |messages: Vec<Message>| {
        provider
            .convert_to_gemini_request(&LLMRequest {
                messages: messages.into(),
                system_prompt: Some(Arc::from("stable instructions")),
                model: "gemini-2.5-flash".to_string(),
                ..Default::default()
            })
            .expect("conversion should succeed")
    };

    let first = build(first_turn);
    let second = build(second_turn);

    assert_eq!(
        serde_json::to_value(&first.system_instruction).expect("serialize"),
        serde_json::to_value(&second.system_instruction).expect("serialize"),
        "system instruction must repeat verbatim"
    );
    let first_contents = serde_json::to_value(&first.contents).expect("serialize");
    let second_contents = serde_json::to_value(&second.contents).expect("serialize");
    let (Some(first_items), Some(second_items)) = (first_contents.as_array(), second_contents.as_array()) else {
        panic!("contents should serialize as arrays");
    };
    assert!(second_items.len() > first_items.len(), "grown history must extend contents");
    assert_eq!(
        &second_items[..first_items.len()],
        first_items,
        "grown history must not rewrite the contents prefix"
    );
}

#[test]
fn part_json_deserialization_function_call_with_thought_signature() {
    // Test 1: FunctionCall with thoughtSignature (camelCase - native Gemini API)
    let json_camel = json!({
        "functionCall": {"name": "test_func", "args": {"key": "value"}},
        "thoughtSignature": "sig_camel_123"
    });
    let part: Part =
        serde_json::from_value(json_camel).expect("should deserialize function call with camelCase thoughtSignature");
    match &part {
        Part::FunctionCall { function_call, thought_signature } => {
            assert_eq!(function_call.name, "test_func");
            assert_eq!(
                thought_signature.as_deref(),
                Some("sig_camel_123"),
                "thoughtSignature (camelCase) should be captured"
            );
        }
        other => panic!("Expected FunctionCall, got {other:?}"),
    }

    // Test 2: FunctionCall WITHOUT thought signature
    let json_no_sig = json!({
        "functionCall": {"name": "test_func", "args": {"key": "value"}}
    });
    let part2: Part = serde_json::from_value(json_no_sig).expect("should deserialize function call without signature");
    match &part2 {
        Part::FunctionCall { function_call, thought_signature } => {
            assert_eq!(function_call.name, "test_func");
            assert_eq!(thought_signature, &None, "missing signature should be None");
        }
        other => panic!("Expected FunctionCall, got {other:?}"),
    }

    // Test 3: Text part
    let json_text = json!({"text": "hello world"});
    let part3: Part = serde_json::from_value(json_text).expect("should deserialize text part");
    match &part3 {
        Part::Text { text, .. } => {
            assert_eq!(text, "hello world");
        }
        other => panic!("Expected Text, got {other:?}"),
    }

    // Test 4: Full candidate with function call + thought signature (simulates API response)
    let candidate_json = json!({
        "content": {
            "role": "model",
            "parts": [{
                "functionCall": {"name": "exec_command", "args": {"command": "cargo check"}},
                "thoughtSignature": "api_signature_abc"
            }]
        },
        "finishReason": "FUNCTION_CALL"
    });
    let candidate: StreamingCandidate =
        serde_json::from_value(candidate_json).expect("should deserialize streaming candidate");
    assert_eq!(candidate.content.parts.len(), 1);
    match &candidate.content.parts[0] {
        Part::FunctionCall { function_call, thought_signature } => {
            assert_eq!(function_call.name, "exec_command");
            assert_eq!(
                thought_signature.as_deref(),
                Some("api_signature_abc"),
                "thought signature should be preserved from API response"
            );
        }
        other => panic!("Expected FunctionCall in candidate, got {other:?}"),
    }
}
