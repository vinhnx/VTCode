use std::sync::Arc;

use tokio::sync::{Notify, mpsc};
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_ui::tui::app::{InlineEvent, InlineHandle, TransientEvent, TransientSubmission};

use crate::agent::runloop::model_picker::{ModelPickerProgress, ModelPickerStart, ModelPickerState};
use crate::agent::runloop::unified::session_settings::SessionSettingsControl;
use crate::agent::runloop::unified::state::CtrlCState;

enum Picker {
    Model(Box<ModelPickerState>),
}

pub(super) async fn run(
    mut events: mpsc::UnboundedReceiver<InlineEvent>,
    settings: mpsc::UnboundedSender<SessionSettingsControl>,
    handle: InlineHandle,
    config: AgentConfig,
    vt_cfg: Option<VTCodeConfig>,
    ctrl_c_state: Arc<CtrlCState>,
    ctrl_c_notify: Arc<Notify>,
) {
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let mut picker = None;
    let mut provider = config.provider;
    let mut model = config.model;
    let mut effort = config.reasoning_effort;
    while let Some(event) = events.recv().await {
        match event {
            InlineEvent::Steer(input) => {
                let command = input.text.trim();
                let (is_model, model_has_args) = match command.strip_prefix("/model") {
                    Some(rest) => (rest.is_empty() || rest.starts_with(char::is_whitespace), !rest.is_empty()),
                    None => (false, false),
                };
                if is_model {
                    if picker.is_some() {
                        let _ =
                            renderer.line(MessageStyle::Error, "Complete or cancel the current settings picker first.");
                        continue;
                    }
                    if model_has_args {
                        let _ = renderer.line(
                            MessageStyle::Info,
                            "The busy-turn model picker takes no arguments; opening the picker.",
                        );
                    }
                    match ModelPickerState::new(
                        &mut renderer,
                        vt_cfg.clone(),
                        effort,
                        vt_cfg.as_ref().and_then(|cfg| cfg.provider.openai.service_tier),
                        Some(config.workspace.clone()),
                        provider.clone(),
                        model.clone(),
                        Some(Arc::clone(&ctrl_c_state)),
                        Some(Arc::clone(&ctrl_c_notify)),
                    )
                    .await
                    {
                        Ok(ModelPickerStart::InProgress(state)) => picker = Some(Picker::Model(Box::new(state))),
                        Ok(ModelPickerStart::Completed { state, selection }) => {
                            provider.clone_from(&selection.provider);
                            model.clone_from(&selection.model);
                            effort = selection.reasoning;
                            let _ = settings.send(SessionSettingsControl::Model { picker: Box::new(state), selection });
                            let _ = renderer.line(MessageStyle::Info, "Model selected; pending the next request.");
                        }
                        Ok(ModelPickerStart::Exit) => {}
                        Err(error) => {
                            let _ =
                                renderer.line(MessageStyle::Error, &format!("Failed to start model picker: {error:#}"));
                        }
                    }
                }
            }
            InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Selection(selection))) => {
                match picker.take() {
                    Some(Picker::Model(mut state)) => match state.handle_list_selection(&mut renderer, selection) {
                        Ok(ModelPickerProgress::Completed(selection)) => {
                            provider.clone_from(&selection.provider);
                            model.clone_from(&selection.model);
                            effort = selection.reasoning;
                            let _ = settings.send(SessionSettingsControl::Model { picker: state, selection });
                            let _ = renderer.line(MessageStyle::Info, "Model selected; pending the next request.");
                        }
                        Ok(ModelPickerProgress::NeedsRefresh) => {
                            if let Err(error) = state.refresh_dynamic_models(&mut renderer).await {
                                let _ = renderer
                                    .line(MessageStyle::Error, &format!("Model list refresh failed: {error:#}"));
                            }
                            picker = Some(Picker::Model(state));
                        }
                        Ok(ModelPickerProgress::InProgress) => picker = Some(Picker::Model(state)),
                        Ok(ModelPickerProgress::Cancelled | ModelPickerProgress::Exit) => {}
                        Err(error) => {
                            let _ = renderer.line(MessageStyle::Error, &format!("Invalid model selection: {error:#}"));
                            picker = Some(Picker::Model(state));
                        }
                    },
                    None => {}
                }
            }
            InlineEvent::Transient(TransientEvent::Cancelled) => picker = None,
            _ => {}
        }
    }
}
