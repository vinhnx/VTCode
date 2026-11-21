use super::*;

#[tokio::test]
async fn previous_agent_event_cycles_primary_agent_backward() {
    let (handle, mut renderer) = renderer_with_handle();
    let (ctrl_c_state, ctrl_c_notify) = ctrl_c_handles();
    let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = None;
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_ui::tui::app::InlineHeaderContext::default();
    let mut history = Vec::<uni::Message>::new();
    let mut session_stats = SessionStats::default();
    let mut context_manager = ContextManager::default_for_test();
    let mut context = InlineEventContext::new(
        &mut renderer,
        &handle,
        interrupts,
        &mut ctrl_c_notice_displayed,
        &mut header_context,
        &mut model_picker_state,
        &mut palette_state,
        &mut config,
        &mut vt_cfg,
        &mut provider_client,
        &ctrl_c_state,
        &ctrl_c_notify,
        &session_bootstrap,
        false,
        &mut history,
        &mut session_stats,
        &mut context_manager,
        "test-session",
        "test-thread",
        None,
        None,
    );
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

    let action = context
        .process_event(InlineEvent::CyclePrimaryAgentPrevious, &mut queue)
        .await
        .expect("process previous primary agent");
    assert!(matches!(action, InlineLoopAction::CyclePrimaryAgentPrevious));
}
#[tokio::test]
async fn settings_editor_selection_submits_editor_config_command() {
    let (handle, mut renderer) = renderer_with_handle();
    let (ctrl_c_state, ctrl_c_notify) = ctrl_c_handles();
    let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = Some(ActivePalette::Settings {
        state: Box::new(SettingsPaletteState {
            workspace: std::path::PathBuf::from("."),
            source_path: std::path::PathBuf::from("vtcode.toml"),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: Some("tools".to_string()),
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            pending_secret: None,
            secret_config: None,
            status: None,
        }),
        esc_armed: false,
    });
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_ui::tui::app::InlineHeaderContext::default();
    let mut history = Vec::<uni::Message>::new();
    let mut session_stats = SessionStats::default();
    let mut context_manager = ContextManager::default_for_test();
    let mut context = InlineEventContext::new(
        &mut renderer,
        &handle,
        interrupts,
        &mut ctrl_c_notice_displayed,
        &mut header_context,
        &mut model_picker_state,
        &mut palette_state,
        &mut config,
        &mut vt_cfg,
        &mut provider_client,
        &ctrl_c_state,
        &ctrl_c_notify,
        &session_bootstrap,
        false,
        &mut history,
        &mut session_stats,
        &mut context_manager,
        "test-session",
        "test-thread",
        None,
        None,
    );
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

    let action = context
        .process_event(
            InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Selection(
                InlineListSelection::ConfigAction(ACTION_CONFIGURE_EDITOR.to_string()),
            ))),
            &mut queue,
        )
        .await
        .expect("process configure editor selection");

    assert!(matches!(
        action,
        InlineLoopAction::Submit(ref command) if command.text == "/config tools.editor"
    ));
    assert!(palette_state.is_none());
}
#[tokio::test]
async fn settings_string_selection_opens_value_editor() {
    let temp = tempfile::tempdir().expect("temp workspace");
    let (handle, mut commands, mut renderer) = renderer_with_handle_and_commands();
    let (ctrl_c_state, ctrl_c_notify) = ctrl_c_handles();
    let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = Some(ActivePalette::Settings {
        state: Box::new(SettingsPaletteState {
            workspace: temp.path().to_path_buf(),
            source_path: temp.path().join("vtcode.toml"),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: Some("tools.editor".to_string()),
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            pending_secret: None,
            secret_config: None,
            status: None,
        }),
        esc_armed: false,
    });
    let mut config = runtime_config();
    config.workspace = temp.path().to_path_buf();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_ui::tui::app::InlineHeaderContext::default();
    let mut history = Vec::<uni::Message>::new();
    let mut session_stats = SessionStats::default();
    let mut context_manager = ContextManager::default_for_test();
    let mut context = InlineEventContext::new(
        &mut renderer,
        &handle,
        interrupts,
        &mut ctrl_c_notice_displayed,
        &mut header_context,
        &mut model_picker_state,
        &mut palette_state,
        &mut config,
        &mut vt_cfg,
        &mut provider_client,
        &ctrl_c_state,
        &ctrl_c_notify,
        &session_bootstrap,
        false,
        &mut history,
        &mut session_stats,
        &mut context_manager,
        "test-session",
        "test-thread",
        None,
        None,
    );
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

    let action = context
        .process_event(
            InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Selection(
                InlineListSelection::ConfigAction(format!("{ACTION_PREFIX_EDIT}tools.editor.preferred_editor")),
            ))),
            &mut queue,
        )
        .await
        .expect("open string settings editor");
    assert!(matches!(action, InlineLoopAction::Continue));

    match commands.recv().await.expect("settings editor wizard command") {
        InlineCommand::ShowTransient { request } => match *request {
            TransientRequest::Wizard(request) => {
                assert_eq!(request.title, "Edit setting");
                assert_eq!(request.steps.len(), 1);
                assert_eq!(request.steps[0].freeform_default.as_deref(), Some(""));
            }
            other => panic!("expected settings editor wizard, got {other:?}"),
        },
        other => panic!("expected settings editor transient, got {}", other_name(&other)),
    }

    let action = context
        .process_event(
            InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Wizard(vec![
                InlineListSelection::RequestUserInputAnswer {
                    question_id: "settings_value".to_string(),
                    selected: Vec::new(),
                    other: Some("code --wait".to_string()),
                },
            ]))),
            &mut queue,
        )
        .await
        .expect("apply string settings edit");
    assert!(matches!(action, InlineLoopAction::Continue));
    assert!(
        std::iter::from_fn(|| commands.try_recv().ok())
            .any(|command| matches!(command, InlineCommand::ShowTransient { .. }))
    );
    assert!(matches!(
        palette_state,
        Some(ActivePalette::Settings { state, .. })
            if state.pending_edit_path.is_none() && state.draft.tools.editor.preferred_editor == "code --wait"
    ));
}
#[tokio::test]
async fn mode_palette_selection_selects_primary_agent() {
    let (handle, mut renderer) = renderer_with_handle();
    let (ctrl_c_state, ctrl_c_notify) = ctrl_c_handles();
    let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = Some(ActivePalette::Mode);
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_ui::tui::app::InlineHeaderContext::default();
    let mut history = Vec::<uni::Message>::new();
    let mut session_stats = SessionStats::default();
    let mut context_manager = ContextManager::default_for_test();
    let mut context = InlineEventContext::new(
        &mut renderer,
        &handle,
        interrupts,
        &mut ctrl_c_notice_displayed,
        &mut header_context,
        &mut model_picker_state,
        &mut palette_state,
        &mut config,
        &mut vt_cfg,
        &mut provider_client,
        &ctrl_c_state,
        &ctrl_c_notify,
        &session_bootstrap,
        false,
        &mut history,
        &mut session_stats,
        &mut context_manager,
        "test-session",
        "test-thread",
        None,
        None,
    );
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

    let action = context
        .process_event(
            InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Selection(
                InlineListSelection::ConfigAction(format!("{MODE_ACTION_PREFIX}auto")),
            ))),
            &mut queue,
        )
        .await
        .expect("process mode palette selection");

    assert!(matches!(
        action,
        InlineLoopAction::SelectPrimaryAgent { name: Some(ref name) } if name == "auto"
    ));
    assert!(palette_state.is_none());
}

#[tokio::test]
#[serial_test::serial]
async fn settings_secret_wizard_restores_palette_and_removes_shadowed_key() {
    use crate::agent::runloop::unified::settings_interactive::{ACTION_BACK, create_settings_palette_state};
    use vtcode_auth::AuthCredentialsStoreMode;
    use vtcode_config::api_keys::{CredentialSource, load_stored_credential_with_mode, resolve_credential_with_mode};

    const PROVIDER: &str = "test-secret-company";
    const ENV_KEY: &str = "VTCODE_TEST_SECRET_COMPANY_KEY";
    const VALUE: &str = "disposable-fixture-a73";
    const SHADOW: &str = "disposable-fixture-b29";

    let temp = tempfile::tempdir().expect("temp workspace");
    let env = vtcode_commons::env_lock::lock();
    let previous_config = std::env::var_os("VTCODE_CONFIG");
    let previous_key = std::env::var_os(ENV_KEY);
    env.set_var("VTCODE_CONFIG", temp.path().join("auth"));
    env.remove_var(ENV_KEY);
    let mut vt_cfg = Some(VTCodeConfig::default());
    let snapshot = vt_cfg.as_mut().unwrap();
    snapshot.agent.credential_storage_mode = AuthCredentialsStoreMode::File;
    snapshot.auth.openai.preferred_method = vtcode_auth::OpenAIPreferredMethod::Chatgpt;
    snapshot.custom_providers.push(vtcode_config::core::CustomProviderConfig {
        name: PROVIDER.to_string(),
        display_name: "Test Company".to_string(),
        base_url: "https://example.invalid/v1".to_string(),
        api_key_env: ENV_KEY.to_string(),
        ..Default::default()
    });
    let mut state = create_settings_palette_state(temp.path(), vt_cfg.as_ref()).unwrap();
    // The session uses a CLI override that the Settings disk draft lacks.
    state.draft = VTCodeConfig::default();
    state.view_path = Some("secrets".to_string());
    let mut palette_state = Some(ActivePalette::Settings { state: Box::new(state), esc_armed: false });

    let (handle, mut commands, mut renderer) = renderer_with_handle_and_commands();
    let (ctrl_c_state, ctrl_c_notify) = ctrl_c_handles();
    let mut notice = false;
    let mut header_context = vtcode_ui::tui::app::InlineHeaderContext::default();
    let mut model_picker_state = None;
    let mut config = runtime_config();
    config.workspace = temp.path().to_path_buf();
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let mut history = Vec::new();
    let mut stats = SessionStats::default();
    let mut manager = ContextManager::default_for_test();
    let bootstrap = SessionBootstrap::default();
    let mut queue_items = VecDeque::new();
    let mut prefer_latest = false;
    let mut queue = InlineQueueState::new(&handle, &mut queue_items, &mut prefer_latest);
    let mut context = InlineEventContext::new(
        &mut renderer,
        &handle,
        InlineInterruptCoordinator::new(ctrl_c_state.as_ref()),
        &mut notice,
        &mut header_context,
        &mut model_picker_state,
        &mut palette_state,
        &mut config,
        &mut vt_cfg,
        &mut provider_client,
        &ctrl_c_state,
        &ctrl_c_notify,
        &bootstrap,
        false,
        &mut history,
        &mut stats,
        &mut manager,
        "secret-test",
        "secret-test",
        None,
        None,
    );

    let select = |action: &str| {
        InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Selection(
            InlineListSelection::ConfigAction(action.to_string()),
        )))
    };
    let answer = |value: &str| {
        InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Wizard(vec![
            InlineListSelection::RequestUserInputAnswer {
                question_id: "settings_secret".to_string(),
                selected: Vec::new(),
                other: Some(value.to_string()),
            },
        ])))
    };
    let provider_action = format!("settings:secrets:provider:{PROVIDER}");
    let cancel = || InlineEvent::Transient(TransientEvent::Cancelled);
    let file_mode = AuthCredentialsStoreMode::File;
    let stored = || load_stored_credential_with_mode(PROVIDER, ENV_KEY, file_mode).expect("isolated credential store");
    let mut next_request = || {
        std::iter::from_fn(|| commands.try_recv().ok())
            .find_map(|command| match command {
                InlineCommand::ShowTransient { request } => Some(*request),
                _ => None,
            })
            .expect("Settings should render a transient")
    };
    let list_items = |request| match request {
        TransientRequest::List(list) => (list.lines, list.items),
        _ => panic!("expected a Settings list"),
    };

    context
        .process_event(select("settings:open:secrets/add"), &mut queue)
        .await
        .unwrap();
    let (_, items) = list_items(next_request());
    assert!(
        items
            .iter()
            .any(|item| item.title == "Test Company" && item.selection.is_some())
    );
    assert!(
        items
            .iter()
            .any(|item| item.title == Provider::Copilot.label() && item.selection.is_none())
    );

    context.process_event(select(&provider_action), &mut queue).await.unwrap();
    let TransientRequest::Wizard(wizard) = next_request() else {
        panic!("expected API key wizard")
    };
    assert!(wizard.steps[0].mask_input);
    assert!(wizard.steps[0].question.contains("Test Company"));
    assert_eq!(wizard.steps[0].freeform_default.as_deref(), Some(""));
    context.process_event(cancel(), &mut queue).await.unwrap();
    let (lines, _) = list_items(next_request());
    assert!(lines.iter().any(|line| line.contains("Secrets › Add or replace a key")));
    context.process_event(answer(VALUE), &mut queue).await.unwrap();
    assert!(stored().is_none(), "late wizard submission must not store a key after cancel");

    std::fs::write(temp.path().join(".env"), format!("export {ENV_KEY}={SHADOW}\nKEEP_VAR=untouched\n")).unwrap();
    context.process_event(select(&provider_action), &mut queue).await.unwrap();
    assert!(matches!(next_request(), TransientRequest::Wizard(_)));
    context.process_event(answer(VALUE), &mut queue).await.unwrap();
    assert!(stored().as_deref() == Some(VALUE), "stored credential did not round trip");
    assert_eq!(std::fs::read_to_string(temp.path().join(".env")).unwrap(), "KEEP_VAR=untouched\n");
    let (lines, items) = list_items(next_request());
    assert!(lines.iter().any(|line| line == "Settings › Secrets."));
    assert!(
        items.iter().any(|item| item.title == "Test Company"
            && item.subtitle.as_deref() == Some("Ready • OS keyring / encrypted file"))
    );

    env.set_var(ENV_KEY, SHADOW);
    let resolved = resolve_credential_with_mode(PROVIDER, ENV_KEY, Some(temp.path()), file_mode)
        .unwrap()
        .unwrap();
    assert_eq!(resolved.source, CredentialSource::Env);
    context
        .process_event(select("settings:open:secrets/delete"), &mut queue)
        .await
        .unwrap();
    let (_, items) = list_items(next_request());
    assert!(
        items
            .iter()
            .any(|item| item.title == "Test Company" && item.selection.is_some())
    );

    context.process_event(select(&provider_action), &mut queue).await.unwrap();
    assert!(matches!(next_request(), TransientRequest::Wizard(_)));
    context.process_event(answer("con\nfirm"), &mut queue).await.unwrap();
    let (lines, _) = list_items(next_request());
    assert!(lines.iter().any(|line| line.contains("Secrets › Delete a key")));
    assert!(stored().is_some(), "embedded whitespace must fail confirmation");

    context.process_event(select(&provider_action), &mut queue).await.unwrap();
    assert!(matches!(next_request(), TransientRequest::Wizard(_)));
    context.process_event(answer(" confirm "), &mut queue).await.unwrap();
    assert!(stored().is_none());
    let (lines, items) = list_items(next_request());
    assert!(lines.iter().any(|line| line == "Settings › Secrets."));
    assert!(
        items.iter().any(
            |item| item.title == "Test Company" && item.subtitle.as_deref() == Some("Ready • Environment variable")
        )
    );

    context
        .process_event(select("settings:open:secrets/add"), &mut queue)
        .await
        .unwrap();
    assert!(matches!(next_request(), TransientRequest::List(_)));
    context.process_event(select(ACTION_BACK), &mut queue).await.unwrap();
    let (lines, _) = list_items(next_request());
    assert!(lines.iter().any(|line| line == "Settings › Secrets."));
    context
        .process_event(select("settings:open:secrets/delete"), &mut queue)
        .await
        .unwrap();
    assert!(matches!(next_request(), TransientRequest::List(_)));
    context.process_event(cancel(), &mut queue).await.unwrap();
    let (lines, _) = list_items(next_request());
    assert!(lines.iter().any(|line| line == "Settings › Secrets."));
    context.process_event(cancel(), &mut queue).await.unwrap();
    let (_, items) = list_items(next_request());
    assert!(items.iter().any(|item| item.title == "Secrets"));
    assert!(history.is_empty(), "credential input must not reach the transcript");
    assert!(!temp.path().join("vtcode.toml").exists(), "credential input must not persist the settings draft");

    env.restore_var(ENV_KEY, previous_key);
    env.restore_var("VTCODE_CONFIG", previous_config);
}
