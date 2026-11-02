use super::*;

#[test]
fn model_catalog_preserves_cli_recommended_model_order() {
    let recommended = ModelId::fallback_models();
    let models = catalog_models("custom-provider", "custom-model");

    for (rank, model) in recommended.iter().enumerate() {
        let expected_provider = model.provider().as_ref().to_string();
        let expected_id = model.as_str();
        let actual = models
            .iter()
            .find(|entry| entry.provider == expected_provider && entry.id == expected_id)
            .expect("recommended model is available in catalog");
        assert_eq!(actual.recommended_rank, Some(u8::try_from(rank).expect("small recommendation rank")));
    }
    assert!(models.iter().any(|model| model.recommended_rank.is_none()));
    let gemini = models
        .iter()
        .find(|model| model.provider == "gemini" && model.id == "gemini-3.8-flash")
        .expect("Gemini model is available in the catalog");
    let capabilities = gemini.capabilities.as_ref().expect("CLI catalog capabilities are projected");
    assert!(capabilities.reasoning && capabilities.vision && capabilities.tool_call);
    assert!(capabilities.structured_output);
    assert!(capabilities.input_modalities.iter().any(|modality| modality == "audio"));
}
#[test]
fn failed_interaction_delivery_keeps_response_retryable() {
    let interaction = InteractionRequest {
        session_id: "session-a".into(),
        turn_id: "turn-9".into(),
        interaction_id: "interaction-x".into(),
        kind: InteractionKind::Question,
        wizard: false,
        title: "Confirm action".into(),
        details: Vec::new(),
        choices: Vec::new(),
        steps: Vec::new(),
        selected_choice_id: None,
        selected_choice_ids: Vec::new(),
    };
    let response = protocol::InteractionResponse {
        session_id: interaction.session_id.clone(),
        turn_id: interaction.turn_id.clone(),
        interaction_id: interaction.interaction_id.clone(),
        choice_id: None,
        choice_ids: Vec::new(),
        text: None,
        answer_texts: Vec::new(),
        cancelled: true,
    };
    let view = HostInteraction {
        kind: vtcode_ui::tui::app::HostInteractionKind::Question,
        wizard: false,
        title: interaction.title.clone(),
        details: Vec::new(),
        steps: Vec::new(),
        selected_choice_ids: Vec::new(),
    };
    let mut state = ServerState::default();
    state.interaction_registry.insert(&interaction).expect("register interaction");
    state.pending_interactions.insert(
        interaction.interaction_id.clone(),
        PendingInteraction {
            session_id: interaction.session_id.clone(),
            turn_id: interaction.turn_id.clone(),
            view,
        },
    );

    let (closed_input, closed_receiver) = mpsc::unbounded_channel();
    drop(closed_receiver);
    let error = dispatch_interaction_response(&mut state, &response, &[], &[], &closed_input)
        .expect_err("closed runtime rejects delivery");
    assert!(error.to_string().contains("runtime has stopped"));
    assert!(state.pending_interactions.contains_key(&interaction.interaction_id));
    assert_eq!(state.interaction_registry.validate(&response), Ok(()));

    let (live_input, mut live_receiver) = mpsc::unbounded_channel();
    dispatch_interaction_response(&mut state, &response, &[], &[], &live_input).expect("retry reaches runtime");
    assert!(matches!(live_receiver.try_recv(), Ok(InlineEvent::Transient(_))));
    assert!(!state.pending_interactions.contains_key(&interaction.interaction_id));
    assert!(state.interaction_registry.validate(&response).is_err());
    assert!(dispatch_interaction_response(&mut state, &response, &[], &[], &live_input).is_err());
    assert!(live_receiver.try_recv().is_err(), "a consumed response must not be delivered twice");
}
#[cfg(unix)]
#[test]
fn restored_review_uses_latest_canonical_items_without_lifecycle_replay() {
    use vtcode_core::exec::events::*;
    let dir = tempfile::tempdir().unwrap();
    let workspace = vtcode_commons::canonicalize(dir.path()).unwrap();
    assert_eq!(restored_inspector_history(&workspace, "legacy").unwrap(), RestoredInspectorHistory::default());
    let log = vtcode_memory::open(&workspace, "review", vtcode_memory::DEFAULT_MAX_EVENTS).unwrap();
    log.append(&ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "review".into() }))
        .unwrap();
    log.append(&ThreadEvent::ItemStarted(ItemStartedEvent { item: review_item("first", "old patch") }))
        .unwrap();
    log.append(&ThreadEvent::ItemCompleted(ItemCompletedEvent { item: review_item("other", "other patch") }))
        .unwrap();
    log.append(&ThreadEvent::ItemCompleted(ItemCompletedEvent { item: review_item("first", "latest patch") }))
        .unwrap();
    log.flush().unwrap();
    let RestoredInspectorHistory { items, incomplete, .. } = restored_inspector_history(&workspace, "review").unwrap();
    assert!(!incomplete);
    assert_eq!(
        items,
        vec![
            review_item("other", "other patch"),
            review_item("first", "latest patch")
        ]
    );
    // An oversized newest patch must not expose an older patch as current.
    log.append(&ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: review_item("huge", &"x".repeat(210 * 1024)),
    }))
    .unwrap();
    log.flush().unwrap();
    let RestoredInspectorHistory { items, incomplete, .. } = restored_inspector_history(&workspace, "review").unwrap();
    assert!(items.is_empty());
    assert!(incomplete);
}
#[cfg(unix)]
#[test]
fn restored_activity_is_latest_ordered_and_cannot_displace_saved_patches() {
    use vtcode_core::exec::events::*;
    let dir = tempfile::tempdir().unwrap();
    let workspace = vtcode_commons::canonicalize(dir.path()).unwrap();
    let log = vtcode_memory::open(&workspace, "activity", vtcode_memory::DEFAULT_MAX_EVENTS).unwrap();
    let command = |id: &str, output: &str, status| ThreadItem {
        id: id.into(),
        context: None,
        details: ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
            command: "cargo check --locked".into(),
            arguments: None,
            aggregated_output: output.into(),
            exit_code: None,
            status,
        })),
    };
    log.append(&ThreadEvent::ItemStarted(ItemStartedEvent {
        item: command("checked", "starting", CommandExecutionStatus::InProgress),
    }))
    .unwrap();
    log.append(&ThreadEvent::ItemCompleted(ItemCompletedEvent { item: review_item("edit", "saved patch") }))
        .unwrap();
    let completed = command("checked", "check passed", CommandExecutionStatus::Completed);
    log.append(&ThreadEvent::ItemCompleted(ItemCompletedEvent { item: completed.clone() }))
        .unwrap();
    let interrupted = command("unfinished", "partial output", CommandExecutionStatus::InProgress);
    log.append(&ThreadEvent::ItemUpdated(ItemUpdatedEvent { item: interrupted.clone() }))
        .unwrap();
    log.flush().unwrap();
    let restored = restored_inspector_history(&workspace, "activity").unwrap();
    assert_eq!(restored.items, vec![review_item("edit", "saved patch")]);
    assert_eq!(restored.activity_items, vec![completed, interrupted]);
    assert!(!restored.activity_incomplete);
    // Oversized activity stops only its own suffix; review remains intact.
    log.append(&ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: command("huge", &"x".repeat(65 * 1024), CommandExecutionStatus::Completed),
    }))
    .unwrap();
    log.flush().unwrap();
    let restored = restored_inspector_history(&workspace, "activity").unwrap();
    assert_eq!(restored.items, vec![review_item("edit", "saved patch")]);
    assert!(!restored.incomplete);
    assert!(restored.activity_items.is_empty());
    assert!(restored.activity_incomplete);
    // The count cap retains a contiguous newest suffix in persisted order.
    for index in 0..81 {
        log.append(&ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: command(&format!("command-{index}"), "ok", CommandExecutionStatus::Completed),
        }))
        .unwrap();
    }
    log.flush().unwrap();
    let restored = restored_inspector_history(&workspace, "activity").unwrap();
    assert_eq!(restored.activity_items.len(), 80);
    assert_eq!(restored.activity_items[0].id, "command-1");
    assert_eq!(restored.activity_items[79].id, "command-80");
    assert!(restored.activity_incomplete);
    assert_eq!(restored.items.len(), 1);
}
