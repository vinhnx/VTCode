use super::*;
use std::error::Error;
use std::io;

#[test]
fn legacy_session_load_defaults_review_history() {
    let loaded: SessionLoadResult = serde_json::from_value(serde_json::json!({
        "session": {"sessionId": "legacy", "title": "Legacy", "workspace": "/workspace", "completed": true, "resumable": true},
        "messages": [{"role": "user", "text": "Continue"}]
    })).unwrap();
    assert!(loaded.review_items.is_empty());
    assert!(!loaded.review_history_incomplete);
    assert!(loaded.activity_items.is_empty());
    assert!(loaded.activity_output_cursors.is_empty());
    assert!(!loaded.activity_history_incomplete);
    assert!(loaded.timeline_events.is_empty());
    assert!(!loaded.timeline_incomplete);
    assert_eq!(loaded.history_start_index, 0);
    assert!(!loaded.history_has_earlier);
    assert_eq!(loaded.messages[0].archive_index, None);
    assert_eq!(loaded.messages[0].text, "Continue");
    assert_eq!(loaded.session.size_bytes, 0);
    assert_eq!(loaded.session.updated_at, None);
    let round_trip: SessionLoadResult = serde_json::from_value(serde_json::to_value(&loaded).unwrap()).unwrap();
    assert_eq!(round_trip, loaded);
}

#[test]
fn session_history_page_round_trips_archive_cursor_and_request_identity() {
    let params = SessionHistoryParams {
        session_id: "saved-session".into(),
        request_id: "page-2".into(),
        before_index: 160,
        limit: 80,
    };
    let page = SessionHistoryPage {
        session_id: params.session_id.clone(),
        request_id: params.request_id.clone(),
        start_index: 80,
        messages: vec![SessionHistoryMessage {
            event_item_id: None,
            phase: None,
            hidden: false,
            archive_index: Some(80),
            role: "assistant".into(),
            text: "Earlier reply".into(),
        }],
        has_earlier: true,
    };
    let decoded_params: SessionHistoryParams = serde_json::from_value(serde_json::to_value(&params).unwrap()).unwrap();
    let decoded_page: SessionHistoryPage = serde_json::from_value(serde_json::to_value(&page).unwrap()).unwrap();
    assert_eq!(decoded_params, params);
    assert_eq!(decoded_page, page);
    let legacy: SessionHistoryMessage = serde_json::from_value(serde_json::json!({
        "archiveIndex": 1, "role": "user", "text": "hello"
    }))
    .unwrap();
    assert!(!legacy.hidden);
}

#[test]
fn session_output_page_round_trips_bounded_cursor_and_request_identity() {
    let params = SessionOutputReadParams {
        session_id: "saved-session".into(),
        request_id: "output-page-2".into(),
        item_id: "tool-output-1".into(),
        offset: 32_768,
    };
    let page = SessionOutputPage {
        session_id: params.session_id.clone(),
        request_id: params.request_id.clone(),
        item_id: params.item_id.clone(),
        offset: params.offset,
        next_offset: params.offset + 3,
        total_bytes: params.offset + 8,
        text: "界".into(),
        has_more: true,
    };
    let loaded = SessionLoadResult {
        timeline_events: Vec::new(),
        timeline_incomplete: false,
        session: SessionSummary {
            session_id: params.session_id.clone(),
            title: None,
            workspace: "/workspace".into(),
            completed: true,
            resumable: true,
            size_bytes: 0,
            updated_at: None,
        },
        messages: Vec::new(),
        history_start_index: 0,
        history_has_earlier: false,
        review_items: Vec::new(),
        review_history_incomplete: false,
        activity_items: Vec::new(),
        activity_output_cursors: vec![SessionOutputCursor {
            item_id: params.item_id.clone(),
            next_offset: params.offset,
            total_bytes: page.total_bytes,
        }],
        activity_history_incomplete: true,
    };

    assert_eq!(
        serde_json::from_value::<SessionOutputReadParams>(serde_json::to_value(&params).unwrap()).unwrap(),
        params
    );
    assert_eq!(serde_json::from_value::<SessionOutputPage>(serde_json::to_value(&page).unwrap()).unwrap(), page);
    assert_eq!(serde_json::from_value::<SessionLoadResult>(serde_json::to_value(&loaded).unwrap()).unwrap(), loaded);
}

#[test]
fn mcp_app_read_round_trips_params_and_bounded_result() {
    let params = McpAppReadParams {
        uri: "ui://demo/app.html".into(),
        request_id: "app-1".into(),
    };
    let round_trip: McpAppReadParams = serde_json::from_value(serde_json::to_value(&params).unwrap()).unwrap();
    assert_eq!(round_trip, params);

    let result = McpAppReadResult {
        uri: params.uri.clone(),
        request_id: params.request_id.clone(),
        mime_type: Some("text/html;profile=mcp-app".into()),
        body: "<!doctype html>".into(),
        total_bytes: 15,
        truncated: false,
    };
    let value = serde_json::to_value(&result).unwrap();
    assert_eq!(value["requestId"], serde_json::json!("app-1"));
    assert_eq!(value["mimeType"], serde_json::json!("text/html;profile=mcp-app"));
    assert_eq!(value["totalBytes"], serde_json::json!(15));
    let round_trip: McpAppReadResult = serde_json::from_value(value).unwrap();
    assert_eq!(round_trip, result);
}

#[test]
fn negotiation_selects_server_version_and_rejects_major_mismatch() {
    let range = ProtocolRange {
        minimum: ProtocolVersion { major: 1, minor: 0 },
        maximum: PROTOCOL_VERSION,
    };
    assert_eq!(range.negotiate(PROTOCOL_VERSION), Ok(PROTOCOL_VERSION));
    let tracked_only: WorkspaceReviewParams =
        serde_json::from_value(serde_json::json!({"requestId":"old-client"})).unwrap();
    assert!(!tracked_only.include_untracked);
    let with_untracked: WorkspaceReviewParams =
        serde_json::from_value(serde_json::json!({"requestId":"new-client","includeUntracked":true})).unwrap();
    assert!(with_untracked.include_untracked);
    assert!(matches!(
        ProtocolRange {
            minimum: ProtocolVersion { major: 2, minor: 0 },
            maximum: ProtocolVersion { major: 2, minor: 1 }
        }
        .negotiate(PROTOCOL_VERSION),
        Err(ProtocolError::IncompatibleVersion { .. })
    ));
}

#[test]
fn cli_settings_requests_keep_target_versions_and_typed_values() {
    let request = ConfigSettingsWriteParams {
        target: ConfigTarget::Workspace,
        path: "agent.max_conversation_turns".into(),
        value: serde_json::json!(24),
        expected_layer_version: Some("version-a".into()),
    };
    let serialized = serde_json::to_value(&request).expect("serialize request");
    assert_eq!(serialized["target"], "workspace");
    assert_eq!(serialized["expectedLayerVersion"], "version-a");
    assert_eq!(serialized["value"], 24);
    assert_eq!(
        serde_json::from_value::<ConfigSettingsWriteParams>(serialized).expect("deserialize request"),
        request
    );
    assert!(
        serde_json::from_value::<ConfigSettingsWriteParams>(serde_json::json!({
            "target": "workspace",
            "path": "agent.enabled",
            "value": true,
            "unexpected": "field"
        }))
        .is_err()
    );
}

#[test]
fn legacy_cli_settings_rows_default_to_editable_without_metadata() {
    let setting: ConfigSetting = serde_json::from_value(serde_json::json!({
        "path": "agent.default_model",
        "value": "model-a",
        "origin": "workspace"
    }))
    .expect("legacy settings row");
    assert!(setting.editable);
    assert!(setting.description.is_none());
    assert!(setting.options.is_empty());
}

#[test]
fn interaction_responses_are_session_turn_scoped_and_single_use() {
    let mut registry = InteractionRegistry::default();
    let interaction = InteractionRequest {
        session_id: "session-a".to_string(),
        turn_id: "turn-9".to_string(),
        interaction_id: "interaction-x".to_string(),
        kind: InteractionKind::Approval,
        wizard: false,
        title: "Approve tool".to_string(),
        details: vec![],
        choices: vec![InteractionChoice {
            id: "runtime-choice-allow".to_string(),
            label: "Allow once".to_string(),
            description: None,
            recommended: true,
        }],
        steps: Vec::new(),
        selected_choice_id: None,
        selected_choice_ids: Vec::new(),
    };
    registry.insert(&interaction).expect("register request");
    let stale = InteractionResponse {
        session_id: "session-a".to_string(),
        turn_id: "turn-8".to_string(),
        interaction_id: "interaction-x".to_string(),
        choice_id: Some("runtime-choice-allow".to_string()),
        choice_ids: Vec::new(),
        text: None,
        answer_texts: Vec::new(),
        cancelled: false,
    };
    assert_eq!(registry.validate(&stale), Err(ProtocolError::StaleInteraction));
    assert_eq!(registry.resolve(&stale), Err(ProtocolError::StaleInteraction));
    let valid = InteractionResponse { turn_id: "turn-9".to_string(), ..stale };
    assert_eq!(registry.validate(&valid), Ok(()));
    assert_eq!(registry.validate(&valid), Ok(()));
    assert_eq!(registry.resolve(&valid), Ok(()));
    assert_eq!(registry.validate(&valid), Err(ProtocolError::UnknownInteraction));
    assert_eq!(registry.resolve(&valid), Err(ProtocolError::UnknownInteraction));
}

#[tokio::test]
async fn bounded_line_reader_accepts_crlf_and_final_unterminated_frame() -> Result<(), Box<dyn Error>> {
    let input = b"{\"jsonrpc\":\"2.0\"}\r\n{}";
    let mut reader = tokio::io::BufReader::new(&input[..]);
    assert_eq!(read_frame(&mut reader).await?, Some(br#"{"jsonrpc":"2.0"}"#.to_vec()));
    assert_eq!(read_frame(&mut reader).await?, Some(b"{}".to_vec()));
    assert_eq!(read_frame(&mut reader).await?, None);
    Ok(())
}

#[tokio::test]
async fn bounded_line_reader_rejects_oversized_input() {
    let input = vec![b'x'; MAX_FRAME_BYTES + 1];
    let mut reader = tokio::io::BufReader::new(&input[..]);
    let error = read_frame(&mut reader).await.expect_err("oversized frame");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("exceeds"));
}

#[test]
fn protocol_methods_and_wire_structs_use_camel_case() -> Result<(), Box<dyn Error>> {
    let params = InteractionResponse {
        session_id: "s".to_string(),
        turn_id: "t".to_string(),
        interaction_id: "i".to_string(),
        choice_id: Some("choice-opaque".to_string()),
        choice_ids: Vec::new(),
        text: None,
        answer_texts: Vec::new(),
        cancelled: false,
    };
    let theme = ThemePreferenceSelectParams { theme: "ciapre-blue".into() };
    let theme_json = serde_json::to_value(theme)?;
    assert_eq!(theme_json, serde_json::json!({ "theme": "ciapre-blue" }));
    assert!(
        serde_json::from_value::<ThemePreferenceSelectParams>(
            serde_json::json!({ "theme": "ciapre-blue", "path": "/tmp/unsafe" })
        )
        .is_err()
    );
    let value = serde_json::to_value(params)?;
    assert_eq!(value["sessionId"], "s");
    assert_eq!(value["interactionId"], "i");
    assert_eq!(value["choiceId"], "choice-opaque");
    assert_eq!(method::SESSION_CREATE, "session/create");

    let webmcp_pair: WebmcpPairParams = serde_json::from_value(serde_json::json!({
        "origin": "https://vtcode.vinhnx.chatgpt.site"
    }))?;
    assert_eq!(webmcp_pair.origin, "https://vtcode.vinhnx.chatgpt.site");
    assert!(!webmcp_pair.replace);
    let webmcp_result = serde_json::to_value(WebmcpPairResult {
        endpoint: "ws://127.0.0.1:4000/webmcp".into(),
        origin: "https://vtcode.vinhnx.chatgpt.site".into(),
        pairing_code: "ephemeral-code".into(),
        expires_in_secs: 60,
    })?;
    assert_eq!(webmcp_result["pairingCode"], "ephemeral-code");
    assert_eq!(webmcp_result["expiresInSecs"], 60);
    assert_eq!(method::WEBMCP_PAIR, "webmcp/pair");
    Ok(())
}

#[test]
fn optional_protocol_extensions_are_camel_cased() -> Result<(), Box<dyn Error>> {
    // A 1.0-era snapshot without the additive fields still deserializes.
    let snapshot: ConfigurationSnapshot = serde_json::from_value(serde_json::json!({
        "provider": "ollama",
        "model": "gpt-oss:20b",
        "authenticated": true,
        "credentialSetupCommand": "vtcode login ollama",
    }))?;
    assert_eq!(snapshot.sandbox, None);
    assert_eq!(snapshot.context_window, None);
    assert_eq!(snapshot.shared_ui, None);
    assert_eq!(snapshot.secret_store, None);

    let legacy_options: SessionOptionsSnapshot = serde_json::from_value(serde_json::json!({
        "permissionMode": "ask",
        "mode": "build",
        "reasoningEffort": "medium",
        "supportedEfforts": ["low", "medium", "high"],
        "speed": null
    }))?;
    assert!(!legacy_options.fast_mode_supported);
    assert!(!legacy_options.fast_mode_enabled);
    assert_eq!(legacy_options.fast_mode_label, None);
    assert!(legacy_options.available_modes.is_empty());

    let value = serde_json::to_value(ConfigurationSnapshot {
        provider: "ollama".to_string(),
        model: "gpt-oss:20b".to_string(),
        authenticated: true,
        credential_setup_command: "vtcode login ollama".to_string(),
        sandbox: Some("Ask before protected actions".to_string()),
        context_window: Some(131_072),
        shared_ui: None,
        session_options: None,
        secret_store: None,
    })?;
    assert_eq!(value["sandbox"], "Ask before protected actions");
    assert_eq!(value["contextWindow"], 131_072);

    let submit: TurnSubmitParams = serde_json::from_value(serde_json::json!({
        "sessionId": "s",
        "prompt": "hi",
    }))?;
    assert!(submit.attachments.is_empty(), "attachments default for 1.0 clients");
    assert!(!submit.batchable, "batchable defaults for pre-queue clients");

    let value = serde_json::to_value(TurnSubmitParams {
        session_id: "s".to_string(),
        prompt: "hi".to_string(),
        attachments: vec![TurnAttachment {
            path: "src/a.rs".to_string(),
            text: "fn a() {}".to_string(),
        }],
        batchable: true,
    })?;
    assert_eq!(value["attachments"][0]["path"], "src/a.rs");
    assert_eq!(value["batchable"], true);

    let model = serde_json::to_value(ModelInfo {
        provider: "openai".to_string(),
        id: "gpt-6-sol".to_string(),
        display_name: "GPT-6 Sol".to_string(),
        context_window: Some(400_000),
        current: true,
        recommended_rank: Some(2),
        capabilities: Some(ModelCapabilities {
            reasoning: true,
            vision: true,
            tool_call: true,
            structured_output: false,
            is_pro: false,
            input_modalities: vec!["text".into(), "image".into()],
        }),
    })?;
    assert_eq!(model["displayName"], "GPT-6 Sol");
    assert_eq!(model["contextWindow"], 400_000);
    assert_eq!(model["current"], true);
    assert_eq!(model["recommendedRank"], 2);
    assert_eq!(model["capabilities"]["reasoning"], true);
    assert_eq!(model["capabilities"]["inputModalities"], serde_json::json!(["text", "image"]));
    let legacy_model: ModelInfo = serde_json::from_value(serde_json::json!({
        "provider": "openai",
        "id": "gpt-6-sol",
        "displayName": "GPT-6 Sol",
        "current": true
    }))?;
    assert_eq!(legacy_model.capabilities, None);
    assert_eq!(legacy_model.recommended_rank, None);
    assert_eq!(method::CONFIG_MODELS, "config/models");
    assert_eq!(method::CONFIG_SELECT, "config/select");
    Ok(())
}

fn item_update(id: &str, text: &str) -> VersionedThreadEvent {
    use vtcode_exec_events::{AgentMessageItem, ItemUpdatedEvent, ThreadItem, ThreadItemDetails};
    VersionedThreadEvent::new(vtcode_exec_events::ThreadEvent::ItemUpdated(ItemUpdatedEvent {
        item: ThreadItem {
            context: None,
            id: id.to_string(),
            details: ThreadItemDetails::AgentMessage(AgentMessageItem { text: text.to_string() }),
        },
    }))
}

fn plan_delta(item_id: &str, delta: &str) -> VersionedThreadEvent {
    use vtcode_exec_events::PlanDeltaEvent;
    VersionedThreadEvent::new(vtcode_exec_events::ThreadEvent::PlanDelta(Box::new(PlanDeltaEvent {
        thread_id: "thread".to_string(),
        turn_id: "turn".to_string(),
        item_id: item_id.to_string(),
        delta: delta.to_string(),
        recovery: None,
    })))
}

fn turn_completed() -> VersionedThreadEvent {
    use vtcode_exec_events::{TurnCompletedEvent, Usage};
    VersionedThreadEvent::new(vtcode_exec_events::ThreadEvent::TurnCompleted(TurnCompletedEvent {
        final_response_item_id: None,
        completed_at: None,
        usage: Usage {
            input_tokens: 0,
            cached_input_tokens: 0,
            cache_creation_tokens: 0,
            output_tokens: 0,
        },
        in_progress_exec_sessions: Vec::new(),
    }))
}

fn thread_started(id: &str) -> VersionedThreadEvent {
    use vtcode_exec_events::ThreadStartedEvent;
    VersionedThreadEvent::new(vtcode_exec_events::ThreadEvent::ThreadStarted(ThreadStartedEvent {
        thread_id: id.to_string(),
    }))
}

fn tool_output_update(
    id: &str,
    output: &str,
    is_delta: Option<bool>,
    status: vtcode_exec_events::ToolCallStatus,
    exit_code: Option<i32>,
) -> VersionedThreadEvent {
    use vtcode_exec_events::{ItemUpdatedEvent, ThreadItem, ThreadItemDetails, ToolOutputItem};
    VersionedThreadEvent::new(vtcode_exec_events::ThreadEvent::ItemUpdated(ItemUpdatedEvent {
        item: ThreadItem {
            context: None,
            id: id.to_string(),
            details: ThreadItemDetails::ToolOutput(Box::new(ToolOutputItem {
                call_id: format!("{id}-call"),
                tool_call_id: None,
                spool_path: None,
                output: output.to_string(),
                is_delta,
                exit_code,
                status,
                app_previews: Vec::new(),
            })),
        },
    }))
}

fn tool_output_details(event: &VersionedThreadEvent) -> vtcode_exec_events::ToolOutputItem {
    match event.event() {
        vtcode_exec_events::ThreadEvent::ItemUpdated(update) => match &update.item.details {
            vtcode_exec_events::ThreadItemDetails::ToolOutput(output) => output.as_ref().clone(),
            other => panic!("expected tool output, got {other:?}"),
        },
        other => panic!("expected item update, got {other:?}"),
    }
}

#[tokio::test]
async fn saturated_queue_never_fails_the_producer_and_keeps_lifecycle_events() {
    let (sender, receiver) = live_event_channel(8);
    // Fill the queue past capacity with streaming snapshots and deltas.
    for round in 0..64 {
        sender
            .publish(item_update(&format!("item-{}", round % 4), &format!("round {round}")))
            .expect("publish");
        sender.publish(plan_delta("plan", &format!("{round} "))).expect("publish");
    }
    // A lifecycle event must be admitted even while the queue is full.
    sender.publish(turn_completed()).expect("lifecycle publish must not fail");
    // The producer going away ends the stream after draining.
    drop(sender);

    let mut ordered = Vec::new();
    while let Some(event) = receiver.recv().await {
        ordered.push(event);
    }

    assert_eq!(
        ordered.last().expect("drained events"),
        &turn_completed(),
        "lifecycle event must survive saturation"
    );
    let plan_events: Vec<_> = ordered
        .iter()
        .filter(|event| matches!(event.event(), vtcode_exec_events::ThreadEvent::PlanDelta(_)))
        .collect();
    assert_eq!(plan_events.len(), 1, "all plan deltas for one item merge into one event");
    if let vtcode_exec_events::ThreadEvent::PlanDelta(merged) = plan_events[0].event() {
        assert_eq!(merged.delta, (0..64).map(|round| format!("{round} ")).collect::<String>());
    } else {
        panic!("expected merged plan delta");
    }
    assert!(ordered.len() <= 8 + 1, "queue stays bounded: {}", ordered.len());
    assert_eq!(receiver.take_dropped_ordered(), 0, "no ordered event dropped while coalescable entries existed");
}

#[tokio::test]
async fn saturated_ordered_events_drop_oldest_and_count_the_gap() {
    let (sender, receiver) = live_event_channel(4);
    for round in 0..10 {
        sender.publish(thread_started(&round.to_string())).expect("publish");
    }
    drop(sender);
    let mut drained = Vec::new();
    while let Some(event) = receiver.recv().await {
        drained.push(event);
    }
    assert_eq!(drained.len(), 4, "queue stays bounded");
    assert_eq!(receiver.take_dropped_ordered(), 6, "every displaced ordered event is counted");
    if let vtcode_exec_events::ThreadEvent::ThreadStarted(last) = drained[3].event() {
        assert_eq!(last.thread_id, "9", "the newest ordered event must survive");
    } else {
        panic!("expected thread.started");
    }
}

#[tokio::test]
async fn publish_fails_only_after_the_consumer_half_drops() {
    let (sender, receiver) = live_event_channel(4);
    sender.publish(item_update("item", "text")).expect("publish with live consumer");
    drop(receiver);
    assert!(
        sender.publish(item_update("item", "more")).is_err(),
        "publishing after the consumer dropped must fail"
    );
}

#[tokio::test]
async fn tool_output_deltas_for_one_item_merge_without_losing_bytes() {
    use vtcode_exec_events::ToolCallStatus;
    let (sender, receiver) = live_event_channel(4);
    // Asymmetric chunks: differing lengths, unicode split across chunks, and a
    // status transition with an exit code on the final chunk.
    sender
        .publish(tool_output_update("capture", "alpha\n", Some(true), ToolCallStatus::InProgress, None))
        .expect("publish");
    sender
        .publish(tool_output_update("capture", "βeta\n", Some(true), ToolCallStatus::InProgress, None))
        .expect("publish");
    sender
        .publish(tool_output_update("capture", "gamma", Some(true), ToolCallStatus::Failed, Some(3)))
        .expect("publish");
    drop(sender);

    let mut drained = Vec::new();
    while let Some(event) = receiver.recv().await {
        drained.push(event);
    }
    assert_eq!(drained.len(), 1, "pending deltas for one item merge into one event");
    let merged = tool_output_details(&drained[0]);
    assert_eq!(merged.output, "alpha\nβeta\ngamma");
    assert_eq!(merged.is_delta, Some(true));
    assert_eq!(merged.status, ToolCallStatus::Failed, "newest status wins");
    assert_eq!(merged.exit_code, Some(3), "newest exit code wins");
    assert_eq!(merged.call_id, "capture-call");
}

#[tokio::test]
async fn tool_output_snapshots_still_replace_pending_updates() {
    use vtcode_exec_events::ToolCallStatus;
    let (sender, receiver) = live_event_channel(4);
    // Both spellings of "snapshot": absent flag and explicit false.
    sender
        .publish(tool_output_update("capture", "first", None, ToolCallStatus::InProgress, None))
        .expect("publish");
    sender
        .publish(tool_output_update("capture", "second", Some(false), ToolCallStatus::InProgress, None))
        .expect("publish");
    drop(sender);

    let mut drained = Vec::new();
    while let Some(event) = receiver.recv().await {
        drained.push(event);
    }
    assert_eq!(drained.len(), 1, "snapshots keep latest-wins replacement");
    let latest = tool_output_details(&drained[0]);
    assert_eq!(latest.output, "second");
    assert_eq!(latest.is_delta, Some(false));
}

#[tokio::test]
async fn tool_output_delta_and_snapshot_for_one_item_do_not_coalesce() {
    use vtcode_exec_events::ToolCallStatus;
    let (sender, receiver) = live_event_channel(4);
    sender
        .publish(tool_output_update("capture", "chunk-", Some(true), ToolCallStatus::InProgress, None))
        .expect("publish");
    sender
        .publish(tool_output_update("capture", "snapshot", Some(false), ToolCallStatus::Completed, None))
        .expect("publish");
    drop(sender);

    let mut drained = Vec::new();
    while let Some(event) = receiver.recv().await {
        drained.push(event);
    }
    assert_eq!(drained.len(), 2, "delta and snapshot slots coexist without merging");
    assert_eq!(tool_output_details(&drained[0]).output, "chunk-");
    assert_eq!(tool_output_details(&drained[1]).output, "snapshot");
}

#[tokio::test]
async fn interleaved_tool_output_deltas_merge_per_item_in_queue_order() {
    use vtcode_exec_events::ToolCallStatus;
    let (sender, receiver) = live_event_channel(4);
    sender
        .publish(tool_output_update("capture-a", "a1-", Some(true), ToolCallStatus::InProgress, None))
        .expect("publish");
    sender
        .publish(tool_output_update("capture-b", "b1", Some(true), ToolCallStatus::InProgress, None))
        .expect("publish");
    sender
        .publish(tool_output_update("capture-a", "a2", Some(true), ToolCallStatus::InProgress, None))
        .expect("publish");
    drop(sender);

    let mut drained = Vec::new();
    while let Some(event) = receiver.recv().await {
        drained.push(event);
    }
    assert_eq!(drained.len(), 2, "each stream merges independently");
    assert_eq!(tool_output_details(&drained[0]).output, "a1-a2");
    assert_eq!(tool_output_details(&drained[1]).output, "b1");
}

#[test]
fn update_status_snapshot_round_trips_and_older_payloads_decode() {
    let snapshot = UpdateStatusSnapshot {
        current_version: "0.180.5".into(),
        latest_version: Some("0.181.0".into()),
        update_available: true,
        release_url: Some("https://github.com/vinhnx/vtcode/releases/tag/v0.181.0".into()),
        channel: Some("stable".into()),
        check_due: false,
    };
    let decoded: UpdateStatusSnapshot = serde_json::from_value(serde_json::to_value(&snapshot).unwrap()).unwrap();
    assert_eq!(decoded, snapshot);

    // Asymmetric fixtures: an up-to-date service omits every optional field,
    // and a legacy payload carries only the running version.
    let quiet = UpdateStatusSnapshot {
        current_version: "0.181.0".into(),
        latest_version: None,
        update_available: false,
        release_url: None,
        channel: None,
        check_due: true,
    };
    let decoded_quiet: UpdateStatusSnapshot = serde_json::from_value(serde_json::to_value(&quiet).unwrap()).unwrap();
    assert_eq!(decoded_quiet, quiet);

    let legacy: UpdateStatusSnapshot =
        serde_json::from_value(serde_json::json!({ "currentVersion": "0.179.0" })).unwrap();
    assert_eq!(legacy.current_version, "0.179.0");
    assert!(legacy.latest_version.is_none());
    assert!(!legacy.update_available);
    assert!(legacy.release_url.is_none());
    assert!(legacy.channel.is_none());
    assert!(!legacy.check_due);
}

#[test]
fn secret_store_types_round_trip_with_camel_case_and_redacted_debug() {
    let params = SecretStoreParams {
        provider: "meta".into(),
        api_key: "sentinel-key".into(),
    };
    let wire = serde_json::to_value(&params).unwrap();
    assert_eq!(wire["provider"], "meta");
    assert_eq!(wire["apiKey"], "sentinel-key");
    let decoded: SecretStoreParams = serde_json::from_value(wire).unwrap();
    assert_eq!(decoded, params);
    let rendered = format!("{params:?}");
    assert!(rendered.contains("meta"), "provider stays visible: {rendered}");
    assert!(!rendered.contains("sentinel-key"), "key must not leak through Debug: {rendered}");

    let snapshot: ConfigurationSnapshot = serde_json::from_value(serde_json::json!({
        "provider": "meta",
        "model": "muse-spark-1.3",
        "authenticated": false,
        "credentialSetupCommand": "vtcode secret add meta",
        "secretStore": {"provider": "meta", "envVar": "META_API_KEY"},
    }))
    .unwrap();
    assert_eq!(
        snapshot.secret_store,
        Some(SecretStoreInfo {
            provider: "meta".into(),
            env_var: "META_API_KEY".into()
        })
    );
    let result = SecretStoreResult {
        configuration: snapshot.clone(),
        session_reset: true,
    };
    let decoded: SecretStoreResult = serde_json::from_value(serde_json::to_value(&result).unwrap()).unwrap();
    assert_eq!(decoded, result);
    assert_eq!(method::CONFIG_SECRET_STORE, "config/secret/store");
}

#[test]
fn storage_dtos_round_trip_with_camel_case_keys() {
    let summary = StorageSummary {
        state_dir: "/state".into(),
        workspace: "/workspace".into(),
        sessions_bytes: 10,
        sessions_count: 2,
        worktrees_bytes: 20,
        worktrees_count: 1,
        memory_bytes: 30,
        other_bytes: 40,
        total_bytes: 100,
    };
    let value = serde_json::to_value(&summary).unwrap();
    assert_eq!(value.get("sessionsBytes").and_then(|v| v.as_u64()), Some(10));
    assert_eq!(value.get("totalBytes").and_then(|v| v.as_u64()), Some(100));
    let decoded: StorageSummary = serde_json::from_value(value).unwrap();
    assert_eq!(decoded, summary);

    let entry = WorktreeEntry {
        name: "loop-1".into(),
        head: Some("abc123".into()),
        is_dirty: true,
    };
    let decoded: WorktreeEntry = serde_json::from_value(serde_json::to_value(&entry).unwrap()).unwrap();
    assert_eq!(decoded, entry);

    assert_eq!(method::STORAGE_SUMMARY, "storage/summary");
    assert_eq!(method::SESSION_DELETE, "session/delete");
    assert_eq!(method::WORKTREE_LIST, "worktree/list");
    assert_eq!(method::WORKTREE_REMOVE, "worktree/remove");
}

#[tokio::test]
async fn cloned_senders_keep_the_stream_alive_until_the_last_one_drops() {
    let (sender, receiver) = live_event_channel(4);
    let clone = sender.clone();
    drop(sender);
    // A second sender keeps the stream reachable.
    clone.publish(item_update("item", "text")).expect("publish with live clone");
    drop(clone);
    assert!(receiver.recv().await.is_some());
    assert_eq!(receiver.recv().await, None, "drained queue with no senders ends the stream");
}

#[test]
fn turn_queue_types_round_trip_with_camel_case_and_legacy_defaults() -> Result<(), Box<dyn Error>> {
    assert_eq!(method::TURN_QUEUE_TAKE_LATEST, "turn/queue/take-latest");
    assert_eq!(method::TURN_QUEUE_STATUS, "turn/queue-status");

    let legacy_accepted: TurnAccepted = serde_json::from_value(serde_json::json!({
        "sessionId": "s",
        "turnId": "t",
    }))?;
    assert!(!legacy_accepted.queued, "queued defaults for pre-queue services");
    assert_eq!(legacy_accepted.queue_depth, 0);
    assert_eq!(legacy_accepted.dropped, 0);

    let accepted = TurnAccepted {
        session_id: "s".to_string(),
        turn_id: "t".to_string(),
        queued: true,
        queue_depth: 2,
        dropped: 1,
    };
    let value = serde_json::to_value(&accepted)?;
    assert_eq!(value["queued"], true);
    assert_eq!(value["queueDepth"], 2);
    assert_eq!(value["dropped"], 1);
    assert_eq!(serde_json::from_value::<TurnAccepted>(value)?, accepted);

    let empty_take = TurnQueueTakeLatestResult {
        session_id: "s".to_string(),
        text: None,
        queue_depth: 0,
    };
    let value = serde_json::to_value(&empty_take)?;
    assert_eq!(value["text"], Value::Null);
    assert_eq!(serde_json::from_value::<TurnQueueTakeLatestResult>(value)?, empty_take);

    let legacy_status: TurnQueueStatus = serde_json::from_value(serde_json::json!({
        "sessionId": "s",
        "queueDepth": 3,
    }))?;
    assert!(legacy_status.previews.is_empty());
    assert_eq!(legacy_status.dropped_total, 0);

    let status = TurnQueueStatus {
        session_id: "s".to_string(),
        queue_depth: 3,
        previews: vec!["first".to_string(), "second".to_string()],
        dropped_total: 4,
    };
    let value = serde_json::to_value(&status)?;
    assert_eq!(value["queueDepth"], 3);
    assert_eq!(value["previews"], serde_json::json!(["first", "second"]));
    assert_eq!(value["droppedTotal"], 4);
    assert_eq!(serde_json::from_value::<TurnQueueStatus>(value)?, status);

    let undone = TurnUndoResult {
        session_id: "s".to_string(),
        turn_id: "t".to_string(),
        undone: true,
        files: vec![TurnUndoFile {
            path: "src/main.rs".to_string(),
            kind: vtcode_exec_events::PatchChangeKind::Update,
            additions: 2,
            deletions: 1,
        }],
        reason: String::new(),
    };
    let value = serde_json::to_value(&undone)?;
    assert_eq!(value["sessionId"], "s");
    assert_eq!(value["undone"], true);
    assert_eq!(value["files"][0]["kind"], "update");
    assert_eq!(value["files"][0]["path"], "src/main.rs");
    assert_eq!(serde_json::from_value::<TurnUndoResult>(value)?, undone);

    let legacy_refusal: TurnUndoResult = serde_json::from_value(serde_json::json!({
        "sessionId": "s",
        "turnId": "t",
        "undone": false,
        "reason": "no recorded changes",
    }))?;
    assert!(legacy_refusal.files.is_empty());
    Ok(())
}

#[test]
fn queue_preview_label_truncates_at_char_boundaries() {
    assert_eq!(queue_preview_label(""), "");
    assert_eq!(queue_preview_label("short"), "short");

    let exact = "a".repeat(MAX_QUEUE_PREVIEW_CHARS);
    assert_eq!(queue_preview_label(&exact), exact, "exact fit keeps the full text");

    let over = "b".repeat(MAX_QUEUE_PREVIEW_CHARS + 1);
    let expected = format!("{}…", "b".repeat(MAX_QUEUE_PREVIEW_CHARS));
    assert_eq!(queue_preview_label(&over), expected, "one over truncates with ellipsis");

    let emoji = "\u{1F600}".repeat(MAX_QUEUE_PREVIEW_CHARS + 5);
    let preview = queue_preview_label(&emoji);
    assert_eq!(
        preview.chars().count(),
        MAX_QUEUE_PREVIEW_CHARS + 1,
        "multi-byte truncation counts chars, not bytes"
    );
    assert!(preview.ends_with('…'));
    assert!(preview.is_char_boundary(preview.len() - '…'.len_utf8()));

    assert_eq!(queue_preview_label("line one\nline two"), "line one\nline two", "short multiline text is preserved");
}

#[test]
fn recent_prompts_params_default_to_project_scope_and_round_trip() {
    let defaulted: RecentPromptsParams = serde_json::from_value(serde_json::json!({})).unwrap();
    assert_eq!(defaulted.scope, vtcode_commons::history_scope::HistoryScope::Project);

    let params = RecentPromptsParams {
        scope: vtcode_commons::history_scope::HistoryScope::Everywhere,
    };
    let encoded = serde_json::to_value(&params).unwrap();
    assert_eq!(encoded, serde_json::json!({ "scope": "everywhere" }));
    let decoded: RecentPromptsParams = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded, params);

    assert!(serde_json::from_value::<RecentPromptsParams>(serde_json::json!({ "scope": "galaxy" })).is_err());
    assert!(
        serde_json::from_value::<RecentPromptsParams>(serde_json::json!({ "scope": "project", "other": 1 })).is_err()
    );

    let prompt = RecentPrompt {
        content: "Fix the login bug".into(),
        session_id: "session-1".into(),
    };
    let decoded: RecentPrompt = serde_json::from_value(serde_json::to_value(&prompt).unwrap()).unwrap();
    assert_eq!(decoded, prompt);
}

#[test]
fn session_list_params_default_to_max_and_clamp() {
    let defaulted: SessionListParams = serde_json::from_value(serde_json::json!({})).unwrap();
    assert_eq!(defaulted.limit, None);
    assert_eq!(defaulted.effective_limit(), MAX_SESSION_LIST_LIMIT);

    for (limit, expected) in [
        (Some(0), 1),
        (Some(1), 1),
        (Some(50), 50),
        (Some(10_000), MAX_SESSION_LIST_LIMIT),
    ] {
        let params = SessionListParams { limit };
        assert_eq!(params.effective_limit(), expected, "limit {limit:?}");
        let decoded: SessionListParams = serde_json::from_value(serde_json::to_value(&params).unwrap()).unwrap();
        assert_eq!(decoded, params);
    }

    // Lenient shape: old clients send `{}`, future fields must not break old servers.
    assert!(serde_json::from_value::<SessionListParams>(serde_json::json!({ "limit": 50, "other": 1 })).is_ok());
}
