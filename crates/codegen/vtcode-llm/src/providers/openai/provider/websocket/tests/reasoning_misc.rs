use super::*;

#[tokio::test]
async fn websocket_legacy_gpt_reasoning_matches_http_after_final_assembly() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    for empty_output in [false, true] {
        let events = vec![
            private_reasoning_delta(),
            text_delta("visible"),
            completion(
                "response",
                if empty_output {
                    json!([])
                } else {
                    text_output("visible")
                },
            ),
        ];
        let Some((base, _, handle)) = spawn_scripted_websocket_server(vec![vec![
            ScriptedReply::Events(vec![completion("warm", json!([]))]),
            ScriptedReply::Events(events.clone()),
        ]])
        .await
        else {
            panic!("server unavailable");
        };
        let http = MockServer::start().await;
        let body = events.iter().map(|event| format!("data: {event}\n\n")).collect::<String>();
        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(body),
            )
            .expect(1)
            .mount(&http)
            .await;
        let mut provider = websocket_test_provider(base);
        let mut request = websocket_test_request();
        request.model = "gpt-6-astra".to_string();
        let websocket_response = final_legacy(provider.stream_request(request.clone()).await.unwrap()).await;
        provider.websocket_mode = false;
        provider.responses_url = Arc::from(format!("{}/responses", http.uri()));
        let http_response = final_legacy(provider.stream_request(request).await.unwrap()).await;
        assert_eq!(websocket_response.content, http_response.content);
        assert_eq!(websocket_response.content.as_deref(), Some("visible"));
        assert!(http_response.reasoning.is_none());
        assert!(websocket_response.reasoning.is_none(), "legacy completion must reapply model policy after merge");
        assert!(websocket_response.reasoning_details.is_none());
        assert_eq!(websocket_response.usage.as_ref().unwrap().total_tokens, 26);
        handle.await.unwrap();
    }
}
#[tokio::test]
async fn websocket_gpt_reasoning_replay_chains_using_the_returned_history() {
    let output = json!([
        {"type":"reasoning", "id":"rs_private", "encrypted_content":"opaque", "summary":[]},
        {"type":"message", "id":"msg_first", "role":"assistant", "content":[{"type":"output_text", "text":"visible", "annotations":[]}]}
    ]);
    for entrypoint in ["generate", "legacy", "normalized"] {
        let Some((base, recorded, handle)) = spawn_scripted_websocket_server(vec![vec![
            ScriptedReply::Events(vec![completion("warm", json!([]))]),
            ScriptedReply::Events(vec![completion("first", output.clone())]),
            ScriptedReply::Events(vec![completion("second", text_output("next reply"))]),
        ]])
        .await
        else {
            panic!("server unavailable");
        };
        let provider = websocket_test_provider(base);
        let mut request = websocket_test_request();
        request.model = "gpt-6-astra".to_string();
        let first = match entrypoint {
            "generate" => provider.generate_request(request.clone()).await.unwrap(),
            "legacy" => final_legacy(provider.stream_request(request.clone()).await.unwrap()).await,
            _ => final_normalized(provider.stream_normalized_request(request.clone()).await.unwrap()).await,
        };
        assert!(first.reasoning_details.is_none());
        Arc::make_mut(&mut request.messages).push(ProviderMessage::assistant_with_tools_and_reasoning(
            first.content.unwrap(),
            first.tool_calls.unwrap_or_default(),
            None,
        ));
        Arc::make_mut(&mut request.messages).push(ProviderMessage::user("new turn".to_string()));
        let second = match entrypoint {
            "generate" => provider.generate_request(request).await.unwrap(),
            "legacy" => final_legacy(provider.stream_request(request).await.unwrap()).await,
            _ => final_normalized(provider.stream_normalized_request(request).await.unwrap()).await,
        };
        assert_eq!(second.content.as_deref(), Some("next reply"));
        {
            let requests = recorded.lock().unwrap();
            assert_eq!(requests.len(), 3);
            assert_eq!(requests[2]["previous_response_id"], "first", "{entrypoint}");
            assert_eq!(
                requests[2]["input"],
                json!([{"role":"user", "content":[{"type":"input_text", "text":"new turn"}]}]),
                "{entrypoint}"
            );
        }
        handle.await.unwrap();
    }
}
#[test]
fn websocket_replay_retains_reasoning_on_models_that_support_it() {
    let response = json!({"id":"reasoned", "output":[
        {"type":"reasoning", "id":"rs_1", "encrypted_content":"opaque", "summary":[]},
        {"type":"message", "role":"assistant", "content":[{"type":"output_text", "text":"answer"}]}
    ]});
    assert_eq!(
        super::super::completed_replay_output(&response, "o3").unwrap(),
        vec![
            json!({"type":"reasoning", "id":"rs_1", "encrypted_content":"opaque", "summary":[]}),
            json!({"role":"assistant", "content":[{"type":"output_text", "text":"answer"}]}),
        ]
    );
}
#[test]
fn fast_service_tier_rebuilt_client_payloads_follow_configuration() {
    for model in ["gpt-6-astra", "gpt-6.1-sol", "gpt-5.6-sol"] {
        for tier in [Some(vtcode_config::OpenAIServiceTier::Ultrafast), None] {
            let provider = OpenAIProvider::from_config(
                Some("fixture-key".to_string()),
                None,
                Some(model.to_string()),
                None,
                None,
                None,
                None,
                Some(OpenAIConfig { service_tier: tier, ..Default::default() }),
                None,
            );
            let request = LLMRequest {
                model: model.to_string(),
                messages: vec![crate::provider::Message::user("hello".to_string())].into(),
                ..Default::default()
            };
            let http = provider.convert_to_openai_responses_format(&request).unwrap();
            let chat = provider.convert_to_openai_format(&request).unwrap();
            for warmup in [false, true] {
                let websocket = prepare_websocket_event(&http, None, warmup).unwrap();
                let expected = (tier.is_some() && model != "gpt-5.6-sol").then_some("ultrafast");
                for payload in [&http, &chat, &websocket.event] {
                    assert_eq!(
                        payload.get("service_tier").and_then(Value::as_str),
                        expected,
                        "{model} {tier:?} warmup={warmup}"
                    );
                }
            }
        }
    }
}
#[test]
fn websocket_prompt_cache_ttl_matches_http_streaming_and_non_streaming() {
    let mut provider = OpenAIProvider::new("fixture-key".into());
    provider.prompt_cache_settings.prompt_cache_ttl = Some(vtcode_config::core::prompt_cache::PromptCacheTtl::H12);
    for stream in [false, true] {
        let request = LLMRequest {
            model: vtcode_config::constants::models::openai::GPT_6_LUNA.into(),
            messages: vec![ProviderMessage::user("hello".into())].into(),
            stream,
            ..Default::default()
        };
        let payload = provider.convert_to_openai_responses_format(&request).unwrap();
        let prepared = prepare_websocket_event(&payload, None, false).unwrap();
        assert_eq!(prepared.event.get("prompt_cache_options"), Some(&json!({"ttl": "12h"})));
        assert_eq!(prepared.event.get("prompt_cache_options"), payload.get("prompt_cache_options"));
    }
}
