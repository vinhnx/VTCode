use std::sync::Arc;

use tokio::sync::{Notify, mpsc};
use vtcode_core::config::types::AgentConfig;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_ui::tui::app::{InlineEvent, InlineHandle, TransientEvent, TransientSubmission};

use crate::agent::runloop::model_picker::{ModelPickerProgress, ModelPickerStart, ModelPickerState};
use crate::agent::runloop::unified::session_settings::{
    SessionSettingsControl, SettingsSnapshot, SharedSettingsSnapshot, publish_settings_if_changed,
    read_settings_snapshot,
};
use crate::agent::runloop::unified::state::CtrlCState;

enum Picker {
    Model(Box<ModelPickerState>),
}

pub(super) async fn run(
    mut events: mpsc::UnboundedReceiver<InlineEvent>,
    settings: mpsc::UnboundedSender<SessionSettingsControl>,
    handle: InlineHandle,
    config: AgentConfig,
    ctrl_c_state: Arc<CtrlCState>,
    ctrl_c_notify: Arc<Notify>,
    settings_snapshot: SharedSettingsSnapshot,
) {
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let mut picker = None;
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
                    // Read the shared snapshot at open time: it carries any
                    // idle `/model` change applied by the interaction loop, and
                    // the live config refreshed on reload, so the picker opens
                    // with current values rather than the spawn-time seed.
                    let SettingsSnapshot { provider, model, effort, vt_cfg } =
                        read_settings_snapshot(&settings_snapshot);
                    match ModelPickerState::new(
                        &mut renderer,
                        vt_cfg.as_deref().cloned(),
                        effort,
                        vt_cfg.as_ref().and_then(|cfg| cfg.provider.openai.service_tier),
                        Some(config.workspace.clone()),
                        provider,
                        model,
                        Some(Arc::clone(&ctrl_c_state)),
                        Some(Arc::clone(&ctrl_c_notify)),
                    )
                    .await
                    {
                        Ok(ModelPickerStart::InProgress(state)) => picker = Some(Picker::Model(Box::new(state))),
                        Ok(ModelPickerStart::Completed { state, selection }) => {
                            publish_settings_if_changed(
                                &settings_snapshot,
                                &selection.provider,
                                &selection.model,
                                selection.reasoning,
                            );
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
                            // Publish optimistically so a second busy picker
                            // opened before the request boundary is applied is
                            // already fresh.
                            publish_settings_if_changed(
                                &settings_snapshot,
                                &selection.provider,
                                &selection.model,
                                selection.reasoning,
                            );
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
