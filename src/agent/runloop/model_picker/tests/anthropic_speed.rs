use super::*;
use vtcode_config::{AnthropicSpeed, OpenAIServiceTier};

fn speed_picker(model: &str) -> ModelPickerState {
    let mut state = base_picker_state("anthropic", model);
    let mut cfg = VTCodeConfig::default();
    cfg.provider.anthropic.speed = Some(AnthropicSpeed::Fast);
    cfg.provider.openai.service_tier = Some(OpenAIServiceTier::Ultrafast);
    cfg.provider_overrides.entry("anthropic".to_owned()).or_default().base_url =
        Some("https://api.anthropic.com/v1".to_owned());
    let mut detail =
        parse_model_selection(MODEL_OPTIONS.as_slice(), &format!("anthropic {model}"), Some(&cfg)).unwrap();
    detail.requires_api_key = false;
    state.selection = Some(detail);
    state.settings.vt_cfg = Some(cfg);
    state
}

#[test]
fn anthropic_speed_picker_transitions_and_preserves_keep_choice() {
    for model in [
        "claude-opus-5-5",
        "claude-opus-5",
        "claude-opus-4-8",
        "claude-opus-5-20261016",
    ] {
        for (input, expected) in [
            ("skip", None),
            ("standard", Some(AnthropicSpeed::Standard)),
            ("fast", Some(AnthropicSpeed::Fast)),
        ] {
            let mut state = speed_picker(model);
            let mut renderer = AnsiRenderer::stdout();
            assert!(matches!(
                state.apply_reasoning_choice(&mut renderer, ReasoningEffortLevel::None).unwrap(),
                ModelPickerProgress::InProgress
            ));
            assert_eq!(state.step, PickerStep::AwaitAnthropicSpeed);
            let progress = state.handle_anthropic_speed(&mut renderer, input).unwrap();
            let ModelPickerProgress::Completed(result) = progress else {
                panic!("speed should complete picker")
            };
            assert_eq!(result.anthropic_speed, expected);
            assert_eq!(state.settings.vt_cfg.as_ref().unwrap().provider.anthropic.speed, Some(AnthropicSpeed::Fast));
            assert_eq!(
                state.settings.vt_cfg.as_ref().unwrap().provider.openai.service_tier,
                Some(OpenAIServiceTier::Ultrafast)
            );
        }
    }
}

#[test]
fn anthropic_speed_picker_inline_and_typed_choices_share_validation() {
    for inline in [false, true] {
        let mut state = speed_picker("claude-opus-5-5");
        let mut renderer = AnsiRenderer::stdout();
        state.step = PickerStep::AwaitAnthropicSpeed;
        state.selection.as_mut().unwrap().model_id = "claude-sonnet-5-5".to_owned();
        let progress = if inline {
            state
                .handle_list_selection(
                    &mut renderer,
                    InlineListSelection::ConfigAction("anthropic-speed:fast".to_owned()),
                )
                .unwrap()
        } else {
            state.handle_anthropic_speed(&mut renderer, "fast").unwrap()
        };
        assert!(matches!(progress, ModelPickerProgress::InProgress));
        assert_eq!(state.selected_anthropic_speed, None);
        state.selected_anthropic_speed = Some(AnthropicSpeed::Fast);
        assert!(state.build_result().is_err());
    }
}

#[test]
fn anthropic_speed_ratatui_cancellation_exits_instead_of_retaining_picker_state() {
    let state = speed_picker("claude-opus-5-5");
    let original = toml::Value::try_from(state.settings.vt_cfg.as_ref().unwrap()).unwrap();
    let mut renderer = AnsiRenderer::stdout();

    // A dismissed follow-up must take new()'s Exit branch, whose callers drop
    // the state, rather than reinstalling an AwaitAnthropicSpeed picker.
    let cancelled = RatatuiOutcome::from_progress(&mut renderer, ModelPickerProgress::Cancelled).unwrap();
    assert!(matches!(cancelled, RatatuiOutcome::Exit));
    let pending = RatatuiOutcome::from_progress(&mut renderer, ModelPickerProgress::InProgress).unwrap();
    assert!(matches!(pending, RatatuiOutcome::InProgress));
    assert_eq!(toml::Value::try_from(state.settings.vt_cfg.as_ref().unwrap()).unwrap(), original);
}

#[tokio::test]
async fn anthropic_speed_picker_back_and_cancel_do_not_change_preferences() {
    let mut state = speed_picker("claude-opus-5-5");
    let original = toml::Value::try_from(state.settings.vt_cfg.as_ref().unwrap()).unwrap();
    state.step = PickerStep::AwaitAnthropicSpeed;
    state.selected_anthropic_speed = Some(AnthropicSpeed::Standard);
    state
        .handle_list_selection(
            &mut AnsiRenderer::stdout(),
            InlineListSelection::ConfigAction(rendering::PICKER_BACK_ACTION.to_owned()),
        )
        .unwrap();
    assert_eq!(state.step, PickerStep::AwaitModel);
    assert_eq!(state.selected_anthropic_speed, None);
    assert_eq!(toml::Value::try_from(state.settings.vt_cfg.as_ref().unwrap()).unwrap(), original);
    state.step = PickerStep::AwaitAnthropicSpeed;
    state.selected_anthropic_speed = Some(AnthropicSpeed::Standard);
    let (handle, mut session) = session_with_channels();
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let guard = crate::agent::runloop::unified::external_url_guard::ExternalUrlGuardContext::new(
        &handle,
        &mut session,
        &ctrl_c_state,
        &ctrl_c_notify,
    );
    let progress = state.handle_input(&mut AnsiRenderer::stdout(), "cancel", guard).await.unwrap();
    assert!(matches!(progress, ModelPickerProgress::Cancelled));
    assert_eq!(toml::Value::try_from(state.settings.vt_cfg.as_ref().unwrap()).unwrap(), original);
}
