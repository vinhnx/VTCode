use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvError, SyncSender, sync_channel};
use std::sync::{Arc, atomic::AtomicBool};

use crate::tui::config::KeyboardProtocolConfig;
use crate::tui::config::UiSurfacePreference;
use hashbrown::HashMap;

use crate::tui::core_tui::app::session::AppSession;
use crate::tui::core_tui::app::types::{
    FocusChangeCallback, InlineCommand, InlineCommandSender, InlineEvent, InlineEventCallback, InlineHandle,
    InlineListSelection, InlineSession, InlineTheme, PreviewCallback, SlashCommandItem, TransientActivitySignal,
    TransientEvent, TransientRequest, TransientSubmission,
};
use crate::tui::core_tui::log;
use crate::tui::core_tui::runner::{TuiOptions, run_tui};
use crate::tui::core_tui::session::action::BindingStore;
use crate::tui::core_tui::session::config::AppearanceConfig;
use crate::tui::options::{FullscreenInteractionSettings, KeyboardProtocolSettings, SessionSurface};

const HOST_COMMAND_QUEUE_CAPACITY: usize = 128;

/// Standalone session launch options for reusable integrations.
#[derive(Clone)]
pub struct SessionOptions {
    pub placeholder: Option<String>,
    pub surface_preference: SessionSurface,
    pub inline_rows: u16,
    pub event_callback: Option<InlineEventCallback>,
    pub focus_callback: Option<FocusChangeCallback>,
    pub active_pty_sessions: Option<Arc<std::sync::atomic::AtomicUsize>>,
    pub input_activity_counter: Option<Arc<std::sync::atomic::AtomicU64>>,
    pub keyboard_protocol: KeyboardProtocolSettings,
    pub fullscreen: FullscreenInteractionSettings,
    pub workspace_root: Option<PathBuf>,
    pub slash_commands: Vec<SlashCommandItem>,
    pub appearance: Option<AppearanceConfig>,
    pub app_name: String,
    pub non_interactive_hint: Option<String>,
    /// User-customizable keybindings (action_name → key spec list).
    /// Merged on top of built-in defaults.
    pub key_bindings: HashMap<String, Vec<String>>,
    pub preview_callback: Option<PreviewCallback>,
}

/// Channels for embedding the VT Code session runtime in a non-terminal host.
///
/// The host owns `input` and `commands`; the runloop owns `session`. This
/// constructor creates no terminal worker and performs no terminal setup.
pub struct HostSession {
    /// Session object consumed by the unified runloop.
    pub session: InlineSession,
    /// Send runtime inputs such as prompt submission, cancellation, and
    /// interaction responses.
    pub input: tokio::sync::mpsc::UnboundedSender<InlineEvent>,
    /// Receive presentation and transient-interaction commands from the
    /// runtime.
    pub commands: HostCommandReceiver,
}

/// Bounded runtime-to-host command receiver for non-terminal session hosts.
pub struct HostCommandReceiver {
    receiver: Receiver<InlineCommand>,
    closed: Arc<AtomicBool>,
}

impl HostCommandReceiver {
    /// Wait for the next presentation or interaction command.
    pub fn recv(&self) -> Result<InlineCommand, RecvError> {
        self.receiver.recv()
    }
}

impl Drop for HostCommandReceiver {
    fn drop(&mut self) {
        self.closed.store(true, std::sync::atomic::Ordering::Release);
    }
}

/// Interaction category exposed to non-terminal hosts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostInteractionKind {
    Approval,
    PlanApproval,
    Question,
    Choice,
    Other,
}

/// One display option paired with the original runtime selection.
#[derive(Clone, Debug)]
pub struct HostInteractionChoice {
    /// Stable within this interaction and safe to return over a host protocol.
    pub id: String,
    /// Human-readable label.
    pub label: String,
    /// Supporting details.
    pub description: Option<String>,
    /// Original typed selection. Hosts must return `id`, not a reconstructed label.
    pub selection: InlineListSelection,
}

/// One step in an existing runtime wizard.
#[derive(Clone, Debug)]
pub struct HostInteractionStep {
    pub title: String,
    pub question: String,
    pub choices: Vec<HostInteractionChoice>,
    pub selected_choice_id: Option<String>,
    pub allow_freeform: bool,
}

/// Renderer-neutral view of an existing runtime interaction.
#[derive(Clone, Debug)]
pub struct HostInteraction {
    pub kind: HostInteractionKind,
    pub wizard: bool,
    pub title: String,
    pub details: Vec<String>,
    pub steps: Vec<HostInteractionStep>,
    pub selected_choice_ids: Vec<String>,
}

/// Convert runtime transient UI requests to a small non-terminal interaction view.
pub fn host_interaction_from_command(command: &InlineCommand) -> Option<HostInteraction> {
    let InlineCommand::ShowTransient { request } = command else {
        return None;
    };

    match request.as_ref() {
        TransientRequest::List(request) => {
            let choices = host_choices(&request.items, 0);
            let kind = infer_interaction_kind(choices.iter().map(|choice| &choice.selection));
            let selected_choice_id = request.selected.as_ref().and_then(|selected| {
                choices
                    .iter()
                    .find(|choice| &choice.selection == selected)
                    .map(|choice| choice.id.clone())
            });
            let selected_choice_ids = selected_choice_id.iter().cloned().collect();
            Some(HostInteraction {
                kind,
                wizard: false,
                title: request.title.clone(),
                details: request.lines.clone(),
                steps: vec![HostInteractionStep {
                    title: request.title.clone(),
                    question: request.lines.first().cloned().unwrap_or_default(),
                    choices,
                    selected_choice_id,
                    allow_freeform: false,
                }],
                selected_choice_ids,
            })
        }
        TransientRequest::Wizard(request) => {
            let steps = request
                .steps
                .iter()
                .enumerate()
                .map(|(index, step)| {
                    let choices = host_choices(&step.items, index);
                    let selected_choice_id = step.answer.as_ref().and_then(|selected| {
                        choices
                            .iter()
                            .find(|choice| &choice.selection == selected)
                            .map(|choice| choice.id.clone())
                    });
                    HostInteractionStep {
                        title: step.title.clone(),
                        question: step.question.clone(),
                        choices,
                        selected_choice_id,
                        allow_freeform: step.allow_freeform,
                    }
                })
                .collect::<Vec<_>>();
            let kind = infer_interaction_kind(steps.iter().flat_map(|step| step.choices.iter().map(|c| &c.selection)));
            let selected_choice_ids = steps.iter().filter_map(|step| step.selected_choice_id.clone()).collect();
            Some(HostInteraction {
                kind,
                wizard: true,
                title: request.title.clone(),
                details: Vec::new(),
                steps,
                selected_choice_ids,
            })
        }
        TransientRequest::Modal(request) => Some(HostInteraction {
            kind: HostInteractionKind::Other,
            wizard: false,
            title: request.title.clone(),
            details: request.lines.clone(),
            steps: Vec::new(),
            selected_choice_ids: Vec::new(),
        }),
        _ => None,
    }
}

/// Turn a host response back into the original transient event shape.
pub fn host_interaction_response(
    interaction: &HostInteraction,
    choice_ids: &[String],
    answer_texts: &[Option<String>],
    cancelled: bool,
) -> Option<InlineEvent> {
    if cancelled {
        return Some(InlineEvent::Transient(TransientEvent::Cancelled));
    }

    if interaction.steps.is_empty() {
        return None;
    }

    let mut selections = Vec::with_capacity(interaction.steps.len());
    for (step_index, step) in interaction.steps.iter().enumerate() {
        let choice_id = if choice_ids.len() == interaction.steps.len() {
            choice_ids.get(step_index)
        } else if interaction.steps.len() == 1 {
            choice_ids.first()
        } else {
            return None;
        };
        let answer = answer_texts.get(step_index).and_then(Option::as_deref);
        let mut selection = choice_id
            .and_then(|choice_id| step.choices.iter().find(|choice| &choice.id == choice_id))
            .map(|choice| choice.selection.clone());
        if selection.is_none() && answer.is_some() {
            selection = step.choices.iter().find_map(|choice| {
                matches!(choice.selection, InlineListSelection::RequestUserInputAnswer { .. })
                    .then(|| choice.selection.clone())
            });
        }
        let mut selection = selection?;
        if let Some(answer) = answer {
            apply_freeform_answer(&mut selection, answer);
        }
        selections.push(selection);
    }

    let submission = if interaction.wizard {
        TransientSubmission::Wizard(selections)
    } else {
        TransientSubmission::Selection(selections[0].clone())
    };
    Some(InlineEvent::Transient(TransientEvent::Submitted(submission)))
}

fn host_choices(
    items: &[crate::tui::core_tui::app::types::InlineListItem],
    step_index: usize,
) -> Vec<HostInteractionChoice> {
    items
        .iter()
        .enumerate()
        .filter_map(|(choice_index, item)| {
            let selection = item.selection.clone()?;
            Some(HostInteractionChoice {
                id: format!("step-{step_index}-choice-{choice_index}"),
                label: item.title.clone(),
                description: item.subtitle.clone(),
                selection,
            })
        })
        .collect()
}

fn infer_interaction_kind<'a>(selections: impl Iterator<Item = &'a InlineListSelection>) -> HostInteractionKind {
    let mut kind = HostInteractionKind::Other;
    for selection in selections {
        match selection {
            InlineListSelection::ToolApproval(_)
            | InlineListSelection::ToolApprovalDenyOnce
            | InlineListSelection::ToolApprovalSession
            | InlineListSelection::ToolApprovalPermanent
            | InlineListSelection::ToolApprovalEnable => return HostInteractionKind::Approval,
            InlineListSelection::PlanApprovalExecute
            | InlineListSelection::PlanApprovalFreshContext
            | InlineListSelection::PlanApprovalEditPlan
            | InlineListSelection::PlanApprovalDiscuss
            | InlineListSelection::PlanApprovalAutoAccept
            | InlineListSelection::PlanApprovalSwitchBuild
            | InlineListSelection::PlanApprovalSwitchAuto => return HostInteractionKind::PlanApproval,
            InlineListSelection::RequestUserInputAnswer { .. } | InlineListSelection::AskUserChoice { .. } => {
                kind = HostInteractionKind::Question;
            }
            _ if kind == HostInteractionKind::Other => kind = HostInteractionKind::Choice,
            _ => {}
        }
    }
    kind
}

fn apply_freeform_answer(selection: &mut InlineListSelection, answer: &str) {
    if let InlineListSelection::RequestUserInputAnswer { other, .. } = selection {
        *other = Some(answer.to_string());
    }
}

/// Create an inline session bridge without checking or initializing a TTY.
pub fn new_host_session() -> HostSession {
    let closed = Arc::new(AtomicBool::new(false));
    let (command_sender, command_receiver) = sync_channel(HOST_COMMAND_QUEUE_CAPACITY);
    let (input, events) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_bounded_host(command_sender, Arc::clone(&closed));
    HostSession {
        session: InlineSession { handle: handle.clone(), events, worker: None },
        input,
        commands: HostCommandReceiver { receiver: command_receiver, closed },
    }
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            placeholder: None,
            surface_preference: SessionSurface::default(),
            inline_rows: crate::tui::config::constants::ui::DEFAULT_INLINE_VIEWPORT_ROWS,
            event_callback: None,
            focus_callback: None,
            active_pty_sessions: None,
            input_activity_counter: None,
            keyboard_protocol: KeyboardProtocolSettings::default(),
            fullscreen: FullscreenInteractionSettings::default(),
            workspace_root: None,
            slash_commands: Vec::new(),
            appearance: None,
            app_name: "Agent TUI".to_string(),
            non_interactive_hint: None,
            key_bindings: HashMap::new(),
            preview_callback: None,
        }
    }
}

impl SessionOptions {
    /// Build options from a host adapter's defaults.
    fn from_host(host: &impl crate::tui::host::HostAdapter) -> Self {
        let defaults = host.session_defaults();
        Self {
            surface_preference: defaults.surface_preference,
            inline_rows: defaults.inline_rows,
            keyboard_protocol: defaults.keyboard_protocol,
            fullscreen: defaults.fullscreen,
            workspace_root: host.workspace_root(),
            slash_commands: host.slash_commands(),
            app_name: host.app_name(),
            non_interactive_hint: host.non_interactive_hint(),
            ..Self::default()
        }
    }
}

/// Spawn a session using standalone options and local config types.
pub fn spawn_session_with_options(theme: InlineTheme, options: SessionOptions) -> anyhow::Result<InlineSession> {
    use crossterm::tty::IsTty;

    // Check stdin is a terminal BEFORE spawning the task
    if !std::io::stdin().is_tty() {
        return Err(anyhow::anyhow!(
            "cannot run interactive TUI: stdin is not a terminal (must be run in an interactive terminal)"
        ));
    }

    let (command_tx, command_rx) = tokio::sync::mpsc::unbounded_channel();
    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    let show_logs = log::is_tui_log_capture_enabled();
    let transient_active = Arc::new(TransientActivitySignal::default());
    let transient_active_for_session = transient_active.clone();

    let worker = tokio::spawn(async move {
        if let Err(error) = run_tui(
            command_rx,
            event_tx,
            TuiOptions {
                surface_preference: UiSurfacePreference::from(options.surface_preference),
                inline_rows: options.inline_rows,
                show_logs,
                log_theme: None,
                event_callback: options.event_callback,
                focus_callback: options.focus_callback,
                active_pty_sessions: options.active_pty_sessions,
                input_activity_counter: options.input_activity_counter,
                keyboard_protocol: KeyboardProtocolConfig::from(options.keyboard_protocol),
                fullscreen: options.fullscreen,
                workspace_root: options.workspace_root,
                preview_callback: options.preview_callback.clone(),
            },
            move |rows| {
                let bindings = BindingStore::new(options.key_bindings.clone());
                let mut session = AppSession::new_with_logs_and_bindings(
                    theme,
                    options.placeholder,
                    rows,
                    show_logs,
                    options.appearance,
                    options.slash_commands,
                    options.app_name,
                    bindings,
                );
                session.set_transient_activity_signal(transient_active_for_session.clone());
                session
            },
        )
        .await
        {
            let error_msg = error.to_string();
            if error_msg.contains("stdin is not a terminal") {
                vtcode_commons::diagnostics::stderr(format_args!("Error: Interactive TUI requires a proper terminal."));
                if let Some(hint) = options.non_interactive_hint.as_deref() {
                    eprintln!("{hint}");
                } else {
                    eprintln!("Use a non-interactive mode in your host app for piped input.");
                }
            } else {
                vtcode_commons::diagnostics::stderr(format_args!("Error: TUI startup failed: {error:#}"));
            }
            tracing::error!(%error, "inline session terminated unexpectedly");
        }
    });

    Ok(InlineSession {
        handle: InlineHandle::new_with_transient_signal(InlineCommandSender::Unbounded(command_tx), transient_active),
        events: event_rx,
        worker: Some(worker),
    })
}

/// Spawn a session using defaults from a host adapter.
pub fn spawn_session_with_host(
    theme: InlineTheme,
    host: &impl crate::tui::host::HostAdapter,
) -> anyhow::Result<InlineSession> {
    spawn_session_with_options(theme, SessionOptions::from_host(host))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn host_session_bridges_inputs_and_commands_without_a_tui_worker() {
        let mut host = new_host_session();
        assert!(host.session.worker.is_none());
        host.input
            .send(InlineEvent::Submit("inspect this file".into()))
            .expect("send host input");
        assert!(matches!(
            host.session.next_event().await,
            Some(InlineEvent::Submit(input)) if input.text == "inspect this file"
        ));

        host.session.handle.set_placeholder(Some("ready".to_string()));
        assert!(matches!(
            host.commands.recv(),
            Ok(InlineCommand::SetPlaceholder { hint: Some(hint), .. }) if hint == "ready"
        ));
    }

    #[test]
    fn host_interaction_returns_the_original_approval_choice() {
        let command = InlineCommand::ShowTransient {
            request: Box::new(TransientRequest::List(crate::tui::core_tui::app::types::ListOverlayRequest {
                title: "Approve command".to_string(),
                lines: vec!["Run the requested command?".to_string()],
                footer_hint: None,
                items: vec![crate::tui::core_tui::app::types::InlineListItem::new(
                    "Do not run (Recommended)",
                    Some(InlineListSelection::ToolApproval(false)),
                )],
                selected: Some(InlineListSelection::ToolApproval(false)),
                search: None,
                hotkeys: Vec::new(),
                status: None,
            })),
        };
        let interaction = host_interaction_from_command(&command).expect("interaction view");
        assert_eq!(interaction.kind, HostInteractionKind::Approval);
        assert!(interaction.steps[0].choices[0].label.contains("Recommended"));

        let event =
            host_interaction_response(&interaction, &[interaction.steps[0].choices[0].id.clone()], &[None], false)
                .expect("host response");
        assert!(matches!(
            event,
            InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Selection(
                InlineListSelection::ToolApproval(false)
            )))
        ));
    }

    #[test]
    fn host_interaction_keeps_single_step_question_wizard_shape_and_freeform_text() {
        let question = InlineListSelection::RequestUserInputAnswer {
            question_id: "goal".to_string(),
            selected: Vec::new(),
            other: Some(String::new()),
        };
        let command = InlineCommand::ShowTransient {
            request: Box::new(TransientRequest::Wizard(crate::tui::core_tui::app::types::WizardOverlayRequest {
                title: "Project goal".to_string(),
                steps: vec![crate::tui::core_tui::app::types::WizardStep {
                    title: "Goal".to_string(),
                    question: "What should this change deliver?".to_string(),
                    items: vec![crate::tui::core_tui::app::types::InlineListItem::new(
                        "Custom answer",
                        Some(question),
                    )],
                    completed: false,
                    answer: None,
                    allow_freeform: true,
                    freeform_label: Some("Custom answer".to_string()),
                    freeform_placeholder: None,
                    freeform_default: None,
                }],
                current_step: 0,
                search: None,
                mode: crate::tui::core_tui::app::types::WizardModalMode::MultiStep,
            })),
        };
        let interaction = host_interaction_from_command(&command).expect("interaction view");
        let choice_id = interaction.steps[0].choices[0].id.clone();
        let event =
            host_interaction_response(&interaction, &[choice_id], &[Some("Keep the change scoped".to_string())], false)
                .expect("host response");
        assert!(interaction.wizard);
        assert!(matches!(
            event,
            InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Wizard(selections)))
                if matches!(selections.as_slice(), [InlineListSelection::RequestUserInputAnswer {
                    question_id,
                    selected,
                    other: Some(answer),
                }] if question_id == "goal" && selected.is_empty() && answer == "Keep the change scoped")
        ));
    }

    struct DemoHost;

    impl crate::tui::host::WorkspaceInfoProvider for DemoHost {
        fn workspace_name(&self) -> String {
            "demo".to_string()
        }

        fn workspace_root(&self) -> Option<PathBuf> {
            Some(PathBuf::from("/workspace/demo"))
        }
    }

    impl crate::tui::host::NotificationProvider for DemoHost {
        fn set_terminal_focused(&self, _focused: bool) {}
    }

    impl crate::tui::host::ThemeProvider for DemoHost {
        fn available_themes(&self) -> Vec<String> {
            vec!["default".to_string()]
        }

        fn active_theme_name(&self) -> Option<String> {
            Some("default".to_string())
        }
    }

    impl crate::tui::host::HostAdapter for DemoHost {
        fn session_defaults(&self) -> crate::tui::host::HostSessionDefaults {
            crate::tui::host::HostSessionDefaults {
                surface_preference: SessionSurface::Inline,
                inline_rows: 24,
                keyboard_protocol: KeyboardProtocolSettings::default(),
                fullscreen: FullscreenInteractionSettings::default(),
            }
        }
    }

    // SessionOptions behavior tests.

    #[test]
    fn session_options_from_host_uses_defaults() {
        let options = SessionOptions::from_host(&DemoHost);

        assert_eq!(options.surface_preference, SessionSurface::Inline);
        assert_eq!(options.inline_rows, 24);
        assert_eq!(options.workspace_root, Some(PathBuf::from("/workspace/demo")));
    }

    #[test]
    fn session_options_default_uses_inline_surface() {
        assert_eq!(SessionOptions::default().surface_preference, SessionSurface::Inline);
    }
}
