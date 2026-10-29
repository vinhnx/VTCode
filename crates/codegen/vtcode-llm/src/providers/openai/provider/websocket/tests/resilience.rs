use super::*;

#[tokio::test]
async fn websocket_pre_output_failures_use_http_once_and_post_output_never_replay() {
    let failures = vec![
        vec![Message::Close(None)],
        vec![], // Socket EOF when the script ends.
        vec![Message::Text("{invalid".into())],
        vec![Message::Text(json!({"type":"response.failed","response":{"error":{"message":"failed"}}}).to_string().into())],
        vec![Message::Text(json!({"type":"response.incomplete","response":{"status":"incomplete"}}).to_string().into())],
        vec![Message::Text(json!({"type":"error","error":{"code":"invalid_request_error","message":"Conversation already has an active response in progress: resp_active"}}).to_string().into())],
        vec![Message::Text(json!({"type":"error","error":{"code":"websocket_connection_limit_reached","message":"limit"}}).to_string().into())],
        vec![Message::Text(json!({"type":"error","error":{"code":"previous_response_not_found","message":"missing"}}).to_string().into())],
        vec![Message::Text(json!({"type":"response.completed","response":{"id":"empty","output":[]}}).to_string().into())],
    ];
    for legacy in [false, true] {
        for post_output in [false, true] {
            for failure in &failures {
                let mut frames = Vec::new();
                if post_output {
                    frames.push(Message::Text(text_delta("partial").to_string().into()));
                }
                // An empty successful completion after a delta is valid recovery,
                // covered above; use a missing response to test malformed completion.
                if post_output
                    && failure
                        .first()
                        .is_some_and(|frame| matches!(frame,Message::Text(text) if text.contains("response.completed")))
                {
                    frames.push(Message::Text(json!({"type":"response.completed"}).to_string().into()));
                } else {
                    frames.extend(failure.clone());
                }
                let Some((base, recorded, handle)) = spawn_scripted_websocket_server(vec![vec![
                    ScriptedReply::Events(vec![completion("warm", json!([]))]),
                    ScriptedReply::Frames(frames),
                ]])
                .await
                else {
                    panic!("server unavailable");
                };
                let http = http_stream_server(if post_output { 0 } else { 1 }).await;
                let mut provider = websocket_test_provider(base);
                provider.responses_url = Arc::from(format!("{}/responses", http.uri()));
                if legacy {
                    let mut stream = provider.stream_request(websocket_test_request()).await.unwrap();
                    let mut errors = 0;
                    let mut text = String::new();
                    let mut done = false;
                    while let Some(event) = stream.next().await {
                        match event {
                            Ok(crate::provider::LLMStreamEvent::Token { delta }) => text.push_str(&delta),
                            Ok(crate::provider::LLMStreamEvent::Completed { .. }) => done = true,
                            Err(_) => errors += 1,
                            _ => {}
                        }
                    }
                    assert_eq!(text, if post_output { "partial" } else { "http" });
                    assert_eq!(errors, usize::from(post_output));
                    assert_eq!(done, !post_output);
                } else {
                    let mut stream = provider.stream_normalized_request(websocket_test_request()).await.unwrap();
                    let mut errors = 0;
                    let mut text = String::new();
                    let mut done = false;
                    while let Some(event) = stream.next().await {
                        match event {
                            Ok(crate::provider::NormalizedStreamEvent::TextDelta { delta }) => text.push_str(&delta),
                            Ok(crate::provider::NormalizedStreamEvent::Done { .. }) => done = true,
                            Err(_) => errors += 1,
                            _ => {}
                        }
                    }
                    assert_eq!(text, if post_output { "partial" } else { "http" });
                    assert_eq!(errors, usize::from(post_output));
                    assert_eq!(done, !post_output);
                }
                assert_eq!(recorded.lock().unwrap().len(), 2, "one warmup and one generation, no WS retry");
                assert!(provider.websocket_session.lock().await.is_none());
                assert!(provider.websocket_continuation_snapshot().is_none());
                assert_eq!(http.received_requests().await.unwrap().len(), usize::from(!post_output));
                handle.await.unwrap();
            }
        }
    }
}
#[tokio::test]
async fn websocket_tier_rejection_uses_tierless_http_then_cached_reconnect() {
    for legacy in [false, true] {
        for warmup_rejected in [false, true] {
            for tier in ["flex", "ultrafast"] {
                let mut first = Vec::new();
                if !warmup_rejected {
                    first.push(ScriptedReply::Events(vec![completion("warm_old", json!([]))]));
                }
                first.push(ScriptedReply::Events(vec![json!({"type":"error", "status":400,"error":{"code":"invalid_request_error","param":"service_tier","message":"The requested service tier is not allowed for this project"}})]));
                let Some((base, recorded, handle)) = spawn_scripted_websocket_server(vec![
                    first,
                    vec![
                        ScriptedReply::Events(vec![completion("warm_new", json!([]))]),
                        ScriptedReply::Events(vec![completion("ws_final", text_output("cached"))]),
                    ],
                ])
                .await
                else {
                    panic!("server unavailable");
                };
                let http = http_stream_server(1).await;
                let mut provider = websocket_test_provider(base);
                provider.responses_url = Arc::from(format!("{}/responses", http.uri()));
                let mut request = websocket_test_request();
                request.model = "gpt-6-astra".to_string();
                request.service_tier = Some(tier.to_string());
                if legacy {
                    let mut stream = provider.stream_request(request.clone()).await.unwrap();
                    let mut content = String::new();
                    while let Some(event) = stream.next().await {
                        if let crate::provider::LLMStreamEvent::Completed { response } = event.unwrap() {
                            content = response.content.unwrap();
                        }
                    }
                    assert_eq!(content, "http");
                } else {
                    assert_eq!(
                        final_normalized(provider.stream_normalized_request(request.clone()).await.unwrap())
                            .await
                            .content
                            .as_deref(),
                        Some("http")
                    );
                }
                let response = final_normalized(provider.stream_normalized_request(request).await.unwrap()).await;
                assert_eq!(response.content.as_deref(), Some("cached"));
                let http_requests = http.received_requests().await.unwrap();
                let body: Value = serde_json::from_slice(&http_requests[0].body).unwrap();
                assert!(body.get("service_tier").is_none());
                {
                    let requests = recorded.lock().unwrap();
                    let failed_count = if warmup_rejected { 1 } else { 2 };
                    assert_eq!(requests.len(), failed_count + 2);
                    assert!(requests[..failed_count].iter().all(|event| event["service_tier"] == tier));
                    assert!(requests[failed_count..].iter().all(|event| event.get("service_tier").is_none()));
                    assert!(requests[failed_count].get("previous_response_id").is_none());
                    assert_eq!(requests[failed_count + 1]["previous_response_id"], "warm_new");
                }

                handle.await.unwrap();
            }
        }
    }
}
#[tokio::test]
async fn websocket_blank_and_missing_tier_are_omitted_and_opt_out_uses_http() {
    for tier in [None, Some("   ".to_string())] {
        let Some((base, recorded, handle)) = spawn_scripted_websocket_server(vec![vec![
            ScriptedReply::Events(vec![completion("warm", json!([]))]),
            ScriptedReply::Events(vec![completion("reply", text_output("ok"))]),
        ]])
        .await
        else {
            panic!("server unavailable");
        };
        let provider = websocket_test_provider(base);
        let mut request = websocket_test_request();
        request.service_tier = tier;
        final_normalized(provider.stream_normalized_request(request).await.unwrap()).await;
        assert!(recorded.lock().unwrap().iter().all(|event| event.get("service_tier").is_none()));
        handle.await.unwrap();
    }
    for unsupported in [false, true] {
        let http = http_stream_server(2).await;
        let mut provider = websocket_test_provider("http://127.0.0.1:1/v1".to_string());
        provider.responses_url = Arc::from(format!("{}/responses", http.uri()));
        if unsupported {
            provider.backend_setup =
                super::super::super::super::backend_setup::OpenAIBackendSetup::chatgpt_subscription_rig(
                    provider.base_url.to_string(),
                );
        } else {
            provider.websocket_mode = false;
        }
        assert!(!provider.websocket_mode_enabled("gpt-5.6"));
        final_normalized(provider.stream_normalized_request(websocket_test_request()).await.unwrap()).await;
        let mut stream = provider.stream_request(websocket_test_request()).await.unwrap();
        while let Some(event) = stream.next().await {
            event.unwrap();
        }
        assert_eq!(http.received_requests().await.unwrap().len(), 2);
    }
}
#[tokio::test]
async fn websocket_stream_and_generation_share_a_serial_lane() {
    let gate = Arc::new(tokio::sync::Notify::new());
    let Some((base, recorded, handle)) = spawn_scripted_websocket_server(vec![vec![
        ScriptedReply::Events(vec![completion("warm", json!([]))]),
        ScriptedReply::Paused {
            events: vec![text_delta("first")],
            gate: Arc::clone(&gate),
            completion: completion("first_response", text_output("first")),
        },
        ScriptedReply::Events(vec![completion("second_response", text_output("second"))]),
    ]])
    .await
    else {
        panic!("server unavailable");
    };
    let provider = websocket_test_provider(base);
    let mut stream = provider.stream_normalized_request(websocket_test_request()).await.unwrap();
    assert!(stream.next().await.unwrap().is_ok());
    let mut next = websocket_test_request();
    Arc::make_mut(&mut next.messages).push(ProviderMessage::assistant("first".to_string()));
    Arc::make_mut(&mut next.messages).push(ProviderMessage::user("second".to_string()));
    let generation = provider.generate_via_responses_websocket(&next);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(30), generation)
            .await
            .is_err()
    );
    assert_eq!(recorded.lock().unwrap().len(), 2, "queued request must not send on the active lane");
    gate.notify_one();
    final_normalized(stream).await;
    let response = provider.generate_via_responses_websocket(&next).await.unwrap();
    assert_eq!(response.content.as_deref(), Some("second"));
    {
        let requests = recorded.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[2]["previous_response_id"], "first_response");
        assert_eq!(requests[2]["input"], json!([{"role":"user","content":[{"type":"input_text","text":"second"}]}]));
    }

    handle.await.unwrap();
}
#[tokio::test]
async fn websocket_stream_owns_provider_state_and_control_frames_do_not_extend_deadline() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(socket).await.unwrap();
        socket.next().await.unwrap().unwrap();
        socket
            .send(Message::Text(completion("warm", json!([])).to_string().into()))
            .await
            .unwrap();
        socket.next().await.unwrap().unwrap();
        socket
            .send(Message::Text(text_delta("start").to_string().into()))
            .await
            .unwrap();
        let mut pongs = 0;
        for _ in 0..20 {
            if socket.send(Message::Ping(vec![1, 2].into())).await.is_err() {
                break;
            }
            match tokio::time::timeout(std::time::Duration::from_millis(20), socket.next()).await {
                Ok(Some(Ok(Message::Pong(_)))) => pongs += 1,
                Ok(Some(Err(_))) | Ok(None) => break,
                _ => {}
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        pongs
    });
    let mut provider = websocket_test_provider(format!("http://api.openai.com@{addr}/v1"));
    provider.websocket_streaming_ceiling = std::time::Duration::from_millis(100);
    let mut stream = provider.stream_normalized_request(websocket_test_request()).await.unwrap();
    drop(provider);
    assert!(stream.next().await.unwrap().is_ok());
    let error = tokio::time::timeout(std::time::Duration::from_millis(400), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert!(error.to_string().contains("deadline exceeded"));
    assert!(stream.next().await.is_none());
    assert!(task.await.unwrap() > 0, "ping must receive a pong while the request is live");
    let provider = OpenAIProvider::from_config(
        Some("test".to_string()),
        None,
        Some("gpt-5.6".to_string()),
        None,
        None,
        Some(vtcode_config::TimeoutsConfig { streaming_ceiling_seconds: 0, ..Default::default() }),
        None,
        Some(OpenAIConfig { websocket_mode: true, ..Default::default() }),
        None,
    );
    assert_eq!(provider.websocket_streaming_ceiling, std::time::Duration::from_secs(600));
}
#[tokio::test]
async fn websocket_tool_start_failure_respects_caller_output_boundary() {
    let start = json!({"type":"response.output_item.added", "output_index":0, "sequence_number":1,
        "item":{"type":"function_call", "id":"fc_1", "call_id":"call_1", "name":"lookup", "arguments":"", "status":"in_progress"}});
    for legacy in [false, true] {
        let Some((base, recorded, handle)) = spawn_scripted_websocket_server(vec![vec![
            ScriptedReply::Events(vec![completion("warm", json!([]))]),
            ScriptedReply::Events(vec![
                start.clone(),
                json!({"type":"response.failed", "response":{"error":{"message":"after tool"}}}),
            ]),
        ]])
        .await
        else {
            panic!("server unavailable");
        };
        let http = http_stream_server(u64::from(legacy)).await;
        let mut provider = websocket_test_provider(base);
        provider.responses_url = Arc::from(format!("{}/responses", http.uri()));
        if legacy {
            let response = final_legacy(provider.stream_request(websocket_test_request()).await.unwrap()).await;
            assert_eq!(response.content.as_deref(), Some("http"));
        } else {
            let mut stream = provider.stream_normalized_request(websocket_test_request()).await.unwrap();
            assert!(
                matches!(stream.next().await.unwrap().unwrap(),crate::provider::NormalizedStreamEvent::ToolCallStart{call_id,..} if call_id=="call_1")
            );
            assert!(stream.next().await.unwrap().unwrap_err().to_string().contains("after tool"));
            assert!(stream.next().await.is_none());
        }
        assert_eq!(recorded.lock().unwrap().len(), 2);
        assert_eq!(http.received_requests().await.unwrap().len(), usize::from(legacy));
        assert!(provider.websocket_session.lock().await.is_none());
        handle.await.unwrap();
    }
}
#[tokio::test]
async fn websocket_usage_before_done_keeps_the_stream_lease_until_delivery() {
    use crate::provider::NormalizedStreamEvent;
    for text_delta_sent in [false, true] {
        for drop_before_done in [false, true] {
            let mut events = Vec::new();
            if text_delta_sent {
                events.push(text_delta("old reply"));
            }
            events.push(completion("old", text_output("old reply")));
            let mut first_connection = vec![
                ScriptedReply::Events(vec![completion("warm_old", json!([]))]),
                ScriptedReply::Events(events),
            ];
            let script = if drop_before_done {
                vec![
                    first_connection,
                    vec![
                        ScriptedReply::Events(vec![completion("warm_new", json!([]))]),
                        ScriptedReply::Events(vec![completion("new", text_output("new reply"))]),
                    ],
                ]
            } else {
                first_connection.push(ScriptedReply::Events(vec![completion("new", text_output("new reply"))]));
                vec![first_connection]
            };
            let Some((base, recorded, handle)) = spawn_scripted_websocket_server(script).await else {
                panic!("server unavailable");
            };
            let provider = websocket_test_provider(base);
            let mut request = websocket_test_request();
            let mut stream = provider.stream_normalized_request(request.clone()).await.unwrap();
            if text_delta_sent {
                assert!(
                    matches!(stream.next().await.unwrap().unwrap(), NormalizedStreamEvent::TextDelta{delta} if delta == "old reply")
                );
            }
            assert!(
                matches!(stream.next().await.unwrap().unwrap(), NormalizedStreamEvent::Usage{usage} if usage.total_tokens == 26)
            );
            if !drop_before_done {
                assert!(
                    matches!(stream.next().await.unwrap().unwrap(), NormalizedStreamEvent::Done{response} if response.content.as_deref() == Some("old reply"))
                );
            }
            drop(stream);
            assert_eq!(provider.websocket_session.lock().await.is_none(), drop_before_done);
            assert_eq!(provider.websocket_continuation_snapshot().is_none(), drop_before_done);
            Arc::make_mut(&mut request.messages).push(ProviderMessage::assistant("old reply".to_string()));
            Arc::make_mut(&mut request.messages).push(ProviderMessage::user("next".to_string()));
            assert_eq!(
                final_normalized(provider.stream_normalized_request(request).await.unwrap())
                    .await
                    .content
                    .as_deref(),
                Some("new reply")
            );
            {
                let requests = recorded.lock().unwrap();
                assert_eq!(requests.len(), if drop_before_done { 4 } else { 3 });
                if drop_before_done {
                    assert!(requests[2].get("previous_response_id").is_none());
                    assert_eq!(requests[2]["generate"], false);
                    assert_eq!(requests[3]["previous_response_id"], "warm_new");
                    assert_eq!(requests[3]["input"], json!([]));
                } else {
                    assert_eq!(requests[2]["previous_response_id"], "old");
                    assert_eq!(
                        requests[2]["input"],
                        json!([{"role":"user","content":[{"type":"input_text","text":"next"}]}])
                    );
                }
            }
            handle.await.unwrap();
        }
    }
}
#[tokio::test]
async fn websocket_unpolled_completion_stream_drop_invalidates_state() {
    for legacy in [false, true] {
        for include_usage in [false, true] {
            let mut completed = completion("old", text_output("old reply"));
            if !include_usage {
                completed["response"].as_object_mut().unwrap().remove("usage");
            }
            let Some((base, recorded, handle)) = spawn_scripted_websocket_server(vec![
                vec![
                    ScriptedReply::Events(vec![completion("warm_old", json!([]))]),
                    ScriptedReply::Events(vec![completed]),
                ],
                vec![
                    ScriptedReply::Events(vec![completion("warm_new", json!([]))]),
                    ScriptedReply::Events(vec![completion("new", text_output("new reply"))]),
                ],
            ])
            .await
            else {
                panic!("server unavailable");
            };
            let provider = websocket_test_provider(base);
            if legacy {
                drop(provider.stream_request(websocket_test_request()).await.unwrap());
            } else {
                drop(provider.stream_normalized_request(websocket_test_request()).await.unwrap());
            }
            assert!(provider.websocket_session.lock().await.is_none(), "legacy={legacy} usage={include_usage}");
            assert!(provider.websocket_continuation_snapshot().is_none());
            assert_eq!(
                final_normalized(provider.stream_normalized_request(websocket_test_request()).await.unwrap())
                    .await
                    .content
                    .as_deref(),
                Some("new reply")
            );
            {
                let requests = recorded.lock().unwrap();
                assert_eq!(requests.len(), 4);
                assert!(requests[2].get("previous_response_id").is_none());
                assert_eq!(requests[2]["generate"], false);
                assert_eq!(requests[3]["previous_response_id"], "warm_new");
            }
            handle.await.unwrap();
        }
    }
}
