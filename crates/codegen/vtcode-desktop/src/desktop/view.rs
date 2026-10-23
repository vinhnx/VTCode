use std::path::PathBuf;
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel};
use std::time::Duration;

use gpui_kit::base::{Disableable, StyledExt};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Enter, Textarea, TextareaState};
use gpui_kit::component::theme::{ActiveTheme, Theme, ThemeMode};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Context, Entity, PathPromptOptions, Render, TestSupportExt, Window, WindowOptions, div, list, px,
};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use vtcode_desktop_protocol as protocol;
use vtcode_desktop_protocol::{
    ConfigurationSnapshot, InteractionChoice, InteractionRequest, InteractionResponse, InteractionStep,
    SessionHistoryMessage, SessionLoadResult, SessionSummary, ThreadEventNotification,
};

use super::markdown::render_markdown;
use super::transcript::{Transcript, TranscriptItem};
use super::transport::{BackendCommand, BackendEvent, BackendHandle, UI_EVENT_CAPACITY, launch_backend};

const MAX_UI_EVENTS_PER_TICK: usize = 64;

pub(crate) fn run() {
    gpui_kit::application().run(|cx: &mut App| {
        gpui_kit::init(cx);
        let (window, view) = match gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            let theme_mode = ThemeMode::from(window.appearance());
            Theme::change(theme_mode, Some(window), cx);
            cx.new(|cx| DesktopView::new(window, cx, theme_mode))
        }) {
            Ok(result) => result,
            Err(error) => {
                vtcode_commons::diagnostics::stderr(format_args!("failed to open VT Code Desktop window: {error}"));
                return;
            }
        };
        let weak_view = view.downgrade();
        cx.spawn(async move |cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(24)).await;
                if weak_view.update(cx, |view, cx| view.drain_events(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        let _ = window;
    });
}

struct DesktopView {
    workspace: Option<PathBuf>,
    pending_workspace: Option<PathBuf>,
    workspace_picker_open: bool,
    backend: Option<BackendHandle>,
    event_rx: Receiver<BackendEvent>,
    event_tx: SyncSender<BackendEvent>,
    sessions: Vec<SessionSummary>,
    selected_session_id: Option<String>,
    transcript: Transcript,
    active_turn_id: Option<String>,
    cancel_requested: bool,
    cancel_request_sent: bool,
    running: bool,
    status: String,
    error: Option<String>,
    configuration: Option<ConfigurationSnapshot>,
    interaction: Option<InteractionRequest>,
    interaction_choice_ids: Vec<Option<String>>,
    interaction_answer_texts: Vec<Option<String>>,
    interaction_step_index: usize,
    composer: Entity<TextareaState>,
    question_answer: Entity<TextareaState>,
    theme_mode: ThemeMode,
}

impl DesktopView {
    fn new(window: &mut Window, cx: &mut Context<Self>, theme_mode: ThemeMode) -> Self {
        let (event_tx, event_rx) = sync_channel(UI_EVENT_CAPACITY);
        Self {
            workspace: None,
            pending_workspace: None,
            workspace_picker_open: false,
            backend: None,
            event_rx,
            event_tx,
            sessions: Vec::new(),
            selected_session_id: None,
            transcript: Transcript::default(),
            active_turn_id: None,
            cancel_requested: false,
            cancel_request_sent: false,
            running: false,
            status: "Choose a workspace to get started".to_string(),
            error: None,
            configuration: None,
            interaction: None,
            interaction_choice_ids: Vec::new(),
            interaction_answer_texts: Vec::new(),
            interaction_step_index: 0,
            composer: cx.new(|cx| TextareaState::new(window, cx).auto_grow(2, 7).submit_on_enter(true)),
            question_answer: cx.new(|cx| TextareaState::new(window, cx).auto_grow(1, 4)),
            theme_mode,
        }
    }

    fn choose_workspace(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace_picker_open {
            return;
        }
        if !self.workspace_switch_idle() {
            self.error = Some("Cancel the active turn and wait for cleanup before switching workspaces.".to_string());
            cx.notify();
            return;
        }

        self.workspace_picker_open = true;
        let paths = cx.prompt_for_paths(workspace_picker_options());
        cx.notify();
        cx.spawn(async move |view, cx| {
            let result = paths.await;
            let _updated = view.update(cx, |view, cx| {
                view.workspace_picker_open = false;
                match result {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.into_iter().next() {
                            view.select_workspace(path, cx);
                        }
                    }
                    Ok(Err(error)) => view.error = Some(format!("Unable to open the workspace picker: {error}")),
                    Ok(Ok(None)) => {}
                    Err(_) => view.error = Some("The workspace picker closed without returning a result.".to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn workspace_switch_idle(&self) -> bool {
        !self.running && self.pending_workspace.is_none()
    }

    fn select_workspace(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        // A turn may have started while the asynchronous picker was open.
        if !self.workspace_switch_idle() {
            self.error = Some("Cancel the active turn and wait for cleanup before switching workspaces.".to_string());
            cx.notify();
            return;
        }
        let canonical = match vtcode_commons::paths::canonicalize(&path) {
            Ok(path) if path.is_dir() => path,
            _ => {
                self.error = Some("Choose an existing workspace folder.".to_string());
                cx.notify();
                return;
            }
        };
        if self.workspace.as_ref() == Some(&canonical) {
            return;
        }
        if let Some(backend) = self.backend.as_ref() {
            self.pending_workspace = Some(canonical);
            self.status = "Shutting down the current workspace service…".to_string();
            if backend.commands.try_send(BackendCommand::Stop).is_err() {
                self.pending_workspace = None;
                self.error = Some("The local service request queue is full. Try again.".to_string());
            }
            cx.notify();
            return;
        }
        self.start_workspace(canonical, cx);
    }

    fn start_workspace(&mut self, workspace: PathBuf, cx: &mut Context<Self>) {
        self.workspace = Some(workspace.clone());
        self.pending_workspace = None;
        self.sessions.clear();
        self.selected_session_id = None;
        self.transcript.clear();
        self.configuration = None;
        self.active_turn_id = None;
        self.cancel_requested = false;
        self.cancel_request_sent = false;
        self.running = false;
        self.error = None;
        self.status = "Starting local VT Code service…".to_string();

        match launch_backend(workspace, self.event_tx.clone()) {
            Ok(backend) => {
                let protocol_range = protocol::ProtocolRange {
                    minimum: protocol::PROTOCOL_VERSION,
                    maximum: protocol::PROTOCOL_VERSION,
                };
                let initialize = protocol::InitializeParams {
                    protocol: protocol_range,
                    client_name: "VT Code Desktop".to_string(),
                    client_version: env!("CARGO_PKG_VERSION").to_string(),
                };
                let mut sent = send_request(&backend.commands, protocol::method::INITIALIZE, json!(initialize));
                sent &= send_request(&backend.commands, protocol::method::CONFIG_DISCOVER, json!({}));
                sent &= send_request(&backend.commands, protocol::method::SESSION_LIST, json!({}));
                self.backend = Some(backend);
                if !sent {
                    self.error = Some("The local service request queue is unavailable.".to_string());
                }
            }
            Err(error) => self.error = Some(error),
        }
        cx.notify();
    }

    fn create_session(&mut self, cx: &mut Context<Self>) {
        if self.running || self.backend.is_none() {
            return;
        }
        self.clear_error();
        if !self.request(protocol::method::SESSION_CREATE, json!({ "title": null })) {
            self.error = Some("The local service request queue is full. Try again.".to_string());
        }
        cx.notify();
    }

    fn load_session(&mut self, session_id: String, cx: &mut Context<Self>) {
        if self.running || self.backend.is_none() {
            return;
        }
        self.clear_error();
        if !self.request(protocol::method::SESSION_LOAD, json!({ "sessionId": session_id })) {
            self.error = Some("The local service request queue is full. Try again.".to_string());
        }
        cx.notify();
    }

    fn submit_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.running || self.interaction.is_some() || self.backend.is_none() {
            return;
        }
        let Some(session_id) = self.selected_session_id.clone() else {
            self.create_session(cx);
            return;
        };
        let prompt = self.composer.read(cx).value().to_string();
        if prompt.trim().is_empty() {
            return;
        }
        self.clear_error();
        let sent = self.request(protocol::method::TURN_SUBMIT, json!({ "sessionId": session_id, "prompt": prompt }));
        if !sent {
            self.error = Some("The local service request queue is full. Try again.".to_string());
        } else {
            self.transcript.push(TranscriptItem::new(
                format!("desktop-user-{}", uuid_like_id()),
                "user".to_string(),
                "You".to_string(),
                prompt,
            ));
            self.transcript.follow_latest();
            self.running = true;
            self.active_turn_id = None;
            self.cancel_requested = false;
            self.cancel_request_sent = false;
            self.status = "Submitting turn…".to_string();
            self.composer.update(cx, |state, cx| state.set_value("", window, cx));
        }
        cx.notify();
    }

    fn cancel_turn(&mut self, cx: &mut Context<Self>) {
        if !self.running {
            return;
        }
        self.cancel_requested = true;
        self.status = "Cancelling turn and waiting for cleanup…".to_string();
        self.try_send_pending_cancel();
        cx.notify();
    }

    fn try_send_pending_cancel(&mut self) {
        if !self.cancel_requested || self.cancel_request_sent || !self.running {
            return;
        }
        let (Some(session_id), Some(turn_id)) = (self.selected_session_id.clone(), self.active_turn_id.clone()) else {
            self.status = "Cancellation queued; waiting for the service to accept the turn…".to_string();
            return;
        };
        if self.request(protocol::method::TURN_CANCEL, json!({ "sessionId": session_id, "turnId": turn_id })) {
            self.cancel_request_sent = true;
        } else {
            self.error = Some("The cancellation request could not be queued.".to_string());
        }
    }

    fn handle_cancel_state_after_turn_event(&mut self) {
        if !self.running {
            self.cancel_requested = false;
            self.cancel_request_sent = false;
            return;
        }
        self.try_send_pending_cancel();
    }

    fn submit_interaction(
        &mut self,
        choice_ids: Vec<String>,
        cancelled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(interaction) = self.interaction.clone() else {
            return;
        };
        let mut answer_texts = self.interaction_answer_texts.clone();
        if !interaction.steps.is_empty() {
            let current_answer = self.question_answer.read(cx).value().to_string();
            if let Some(answer) = answer_texts.get_mut(self.interaction_step_index) {
                *answer = (!current_answer.trim().is_empty()).then_some(current_answer);
            }
        }
        let response = InteractionResponse {
            session_id: interaction.session_id,
            turn_id: interaction.turn_id,
            interaction_id: interaction.interaction_id,
            choice_id: choice_ids.first().cloned(),
            choice_ids,
            text: answer_texts.iter().flatten().next().cloned(),
            answer_texts,
            cancelled,
        };
        if self.request(protocol::method::INTERACTION_RESPOND, json!(response)) {
            self.interaction = None;
            self.interaction_choice_ids.clear();
            self.interaction_answer_texts.clear();
            self.interaction_step_index = 0;
            self.question_answer.update(cx, |state, cx| state.set_value("", window, cx));
            self.status = if cancelled {
                "Interaction cancelled"
            } else {
                "Decision sent"
            }
            .to_string();
        } else {
            self.error = Some("The decision could not be sent to the local service.".to_string());
        }
        cx.notify();
    }

    fn choose_interaction_option(
        &mut self,
        step_index: usize,
        choice_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(interaction) = self.interaction.as_ref() else {
            return;
        };
        if interaction.kind == protocol::InteractionKind::Question && !interaction.steps.is_empty() {
            if let Some(selected) = self.interaction_choice_ids.get_mut(step_index) {
                *selected = Some(choice_id);
            }
            cx.notify();
            return;
        }
        self.submit_interaction(vec![choice_id], false, window, cx);
    }

    fn advance_interaction_step(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(interaction) = self.interaction.as_ref() else {
            return;
        };
        let answer = self.question_answer.read(cx).value().to_string();
        if let Some(stored) = self.interaction_answer_texts.get_mut(self.interaction_step_index) {
            *stored = (!answer.trim().is_empty()).then_some(answer);
        }
        let final_step = self.interaction_step_index + 1 >= interaction.steps.len();
        if final_step {
            let selections = self.interaction_choice_ids.iter().filter_map(Clone::clone).collect::<Vec<_>>();
            if selections.len() != interaction.steps.len() {
                self.error = Some("Choose an option for every question before submitting.".to_string());
                cx.notify();
                return;
            }
            self.submit_interaction(selections, false, window, cx);
        } else {
            self.interaction_step_index += 1;
            self.question_answer.update(cx, |state, cx| state.set_value("", window, cx));
            cx.notify();
        }
    }

    fn drain_events(&mut self, cx: &mut Context<Self>) {
        let mut changed = false;
        for _ in 0..MAX_UI_EVENTS_PER_TICK {
            match self.event_rx.try_recv() {
                Ok(event) => {
                    changed = true;
                    self.handle_backend_event(event, cx);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.backend = None;
                    self.running = false;
                    self.status = "Local service disconnected".to_string();
                    changed = true;
                    break;
                }
            }
        }
        if changed {
            cx.notify();
        }
    }

    fn handle_backend_event(&mut self, event: BackendEvent, cx: &mut Context<Self>) {
        match event {
            BackendEvent::Response { method, result, error } => {
                if let Some(error) = error {
                    self.error = Some(error);
                    if method == protocol::method::TURN_SUBMIT {
                        self.running = false;
                        self.cancel_requested = false;
                        self.cancel_request_sent = false;
                    } else if method == protocol::method::TURN_CANCEL {
                        self.cancel_request_sent = false;
                    }
                    self.status = "Service request failed".to_string();
                    return;
                }
                let result = result.unwrap_or(Value::Null);
                match method.as_str() {
                    protocol::method::INITIALIZE => {
                        self.status = "Connected to the local VT Code service".to_string();
                    }
                    protocol::method::CONFIG_DISCOVER => {
                        self.configuration = serde_json::from_value::<ConfigurationSnapshot>(result).ok();
                        if self
                            .configuration
                            .as_ref()
                            .is_some_and(|configuration| !configuration.authenticated)
                        {
                            self.error = Some(
                                "No provider credentials are configured. In Terminal, run `vtcode auth` or `vtcode login <provider>`, then restart the desktop app.".to_string(),
                            );
                        }
                    }
                    protocol::method::SESSION_LIST => {
                        self.sessions = serde_json::from_value::<Vec<SessionSummary>>(result).unwrap_or_default();
                    }
                    protocol::method::SESSION_CREATE => {
                        if let Ok(session) = serde_json::from_value::<SessionSummary>(result) {
                            self.selected_session_id = Some(session.session_id.clone());
                            self.sessions.insert(0, session);
                            self.transcript.clear();
                        }
                    }
                    protocol::method::SESSION_LOAD => {
                        if let Ok(loaded) = serde_json::from_value::<SessionLoadResult>(result) {
                            self.load_history(loaded.messages);
                            self.selected_session_id = Some(loaded.session.session_id.clone());
                            self.status = "Session loaded. Submit a prompt to continue.".to_string();
                        }
                    }
                    protocol::method::TURN_SUBMIT => {
                        self.active_turn_id = result.get("turnId").and_then(Value::as_str).map(str::to_owned);
                        self.running = true;
                        self.status = "Running".to_string();
                        self.try_send_pending_cancel();
                    }
                    protocol::method::TURN_CANCEL => {
                        self.status = "Cancellation accepted; waiting for cleanup…".to_string();
                    }
                    protocol::method::INTERACTION_RESPOND => {
                        self.status = "Decision accepted".to_string();
                    }
                    _ => {}
                }
            }
            BackendEvent::ThreadEvent(notification) => {
                self.handle_thread_event(notification);
                self.handle_cancel_state_after_turn_event();
            }
            BackendEvent::Interaction(interaction) => {
                self.interaction_choice_ids =
                    interaction.steps.iter().map(|step| step.selected_choice_id.clone()).collect();
                if self.interaction_choice_ids.is_empty() {
                    self.interaction_choice_ids = interaction.selected_choice_ids.iter().cloned().map(Some).collect();
                }
                self.interaction_answer_texts = vec![None; interaction.steps.len()];
                self.interaction_step_index = 0;
                self.interaction = Some(interaction);
                self.status = "Waiting for your decision".to_string();
            }
            BackendEvent::TurnStatus(status) => {
                if let Some(label) = status.get("status").and_then(Value::as_str) {
                    self.status = label.to_string();
                }
            }
            BackendEvent::Exited { status, authentication_required } => {
                self.backend = None;
                self.running = false;
                self.active_turn_id = None;
                self.cancel_requested = false;
                self.cancel_request_sent = false;
                if let Some(workspace) = self.pending_workspace.take() {
                    self.start_workspace(workspace, cx);
                } else if authentication_required {
                    self.status = "Provider credentials required".to_string();
                    self.error = Some(
                        "VT Code could not authenticate the configured provider. In Terminal, run `vtcode auth` or `vtcode login <provider>`, then restart the desktop app.".to_string(),
                    );
                } else if self.status != "Connected to the local VT Code service" {
                    self.status = match status {
                        Some(code) => format!("Local service exited with code {code}"),
                        None => "Local service exited".to_string(),
                    };
                }
            }
            BackendEvent::LaunchFailed(error) => {
                self.error = Some(error);
                self.status = "Could not connect to the local VT Code service".to_string();
                self.backend = None;
                self.running = false;
                self.cancel_requested = false;
                self.cancel_request_sent = false;
            }
        }
    }

    fn handle_thread_event(&mut self, notification: ThreadEventNotification) {
        let versioned = serde_json::to_value(notification.event).unwrap_or(Value::Null);
        let event = versioned.get("event").cloned().unwrap_or(versioned);
        let kind = event.get("type").and_then(Value::as_str).unwrap_or_default();
        match kind {
            "turn.started" => {
                self.running = true;
                self.active_turn_id = notification.turn_id;
                self.status = "Running".to_string();
            }
            "turn.completed" => {
                self.running = false;
                self.active_turn_id = None;
                self.cancel_requested = false;
                self.cancel_request_sent = false;
                self.status = "Completed".to_string();
            }
            "turn.failed" | "turn.blocked" => {
                self.running = false;
                self.active_turn_id = None;
                self.cancel_requested = false;
                self.cancel_request_sent = false;
                self.status = "Turn ended without verified completion".to_string();
                if let Some(message) = event.pointer("/message").and_then(Value::as_str) {
                    self.transcript.push(TranscriptItem::new(
                        format!("event-{}", uuid_like_id()),
                        "notice".to_string(),
                        "Run status".to_string(),
                        message.to_string(),
                    ));
                }
            }
            "plan.delta" => {
                let id = event.get("item_id").and_then(Value::as_str).unwrap_or("plan").to_string();
                let delta = event.get("delta").and_then(Value::as_str).unwrap_or_default();
                self.append_or_update_item(id, "plan", "Plan", delta, true);
            }
            "item.started" | "item.updated" | "item.completed" => {
                if let Some(item) = event.get("item") {
                    self.update_runtime_item(item);
                }
            }
            "error" => {
                if let Some(message) = event.pointer("/message").and_then(Value::as_str) {
                    self.transcript.push(TranscriptItem::new(
                        format!("runtime-error-{}", uuid_like_id()),
                        "notice".to_string(),
                        "Runtime error".to_string(),
                        message.to_string(),
                    ));
                }
            }
            _ => {}
        }
    }

    fn update_runtime_item(&mut self, item: &Value) {
        let id = item.get("id").and_then(Value::as_str).unwrap_or("runtime-item").to_string();
        let item_type = item.get("type").and_then(Value::as_str).unwrap_or("activity");
        if item_type == "agent_message" || item_type == "plan" || item_type == "reasoning" || item_type == "decision" {
            let text = item.get("text").and_then(Value::as_str).unwrap_or_default().to_string();
            let title = match item_type {
                "plan" => "Plan",
                "reasoning" => "Planning update",
                "decision" => "Decision",
                _ => "VT Code",
            };
            self.append_or_update_item(id, "assistant", title, &text, false);
            return;
        }

        let title = item
            .get("tool_name")
            .or_else(|| item.get("command"))
            .or_else(|| item.get("name"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| item_type.replace('_', " "));
        let summary = item
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("activity")
            .replace('_', " ");
        let details = serde_json::to_string_pretty(item).unwrap_or_else(|_| item.to_string());
        let text = format!("{summary}\n\n{details}");
        self.append_or_update_item(id, "tool", &title, &text, true);
    }

    fn append_or_update_item(&mut self, id: String, role: &str, title: &str, text: &str, expandable: bool) {
        self.transcript.upsert(TranscriptItem::new(
            id,
            if expandable {
                role.to_string()
            } else {
                "assistant".to_string()
            },
            title.to_string(),
            text.to_string(),
        ));
    }

    fn load_history(&mut self, messages: Vec<SessionHistoryMessage>) {
        self.transcript.clear();
        for (index, message) in messages.into_iter().enumerate() {
            let title = match message.role.as_str() {
                "user" => "You".to_string(),
                "assistant" => "VT Code".to_string(),
                "tool" => "Tool result".to_string(),
                _ => message.role.clone(),
            };
            self.transcript
                .push(TranscriptItem::new(format!("history-{index}"), message.role, title, message.text));
        }
        self.transcript.follow_latest();
    }

    fn request(&self, method: &str, params: Value) -> bool {
        self.backend.as_ref().is_some_and(|backend| {
            backend
                .commands
                .try_send(BackendCommand::Request { method: method.to_string(), params })
                .is_ok()
        })
    }

    fn clear_error(&mut self) {
        self.error = None;
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let mut sidebar = div()
            .w(px(270.))
            .h_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .border_r_1()
            .border_color(theme.colors.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_xl().font_semibold().child("VT Code"))
                    .child(
                        Button::new("choose-workspace")
                            .label(if self.workspace.is_some() { "Change" } else { "Open" })
                            .ghost()
                            .disabled(self.workspace_picker_open || !self.workspace_switch_idle())
                            .on_click(cx.listener(|this, _, window, cx| this.choose_workspace(window, cx))),
                    ),
            );

        let workspace = self
            .workspace
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "No workspace selected".to_string());
        sidebar = sidebar.child(div().text_sm().text_color(theme.colors.muted_foreground).child(workspace));

        sidebar = sidebar.child(
            Button::new("new-session")
                .label("New session")
                .primary()
                .disabled(self.workspace.is_none() || self.running || self.backend.is_none())
                .on_click(cx.listener(|this, _, _, cx| this.create_session(cx))),
        );

        sidebar = sidebar.child(div().mt_2().text_sm().font_semibold().child("Recent sessions"));
        sidebar = sidebar.child(
            div()
                .id("session-list")
                .flex_1()
                .w_full()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_1()
                .children(self.sessions.iter().enumerate().map(|(index, session)| {
                    let session_id = session.session_id.clone();
                    let selected = self.selected_session_id.as_deref() == Some(&session.session_id);
                    let label = session.title.clone().unwrap_or_else(|| {
                        format!("Session {}", session.session_id.chars().take(8).collect::<String>())
                    });
                    div().w_full().child(
                        Button::new(format!("session-{index}"))
                            .label(label)
                            .ghost()
                            .disabled(self.running || self.backend.is_none())
                            .when(selected, |button| button.primary())
                            .on_click(cx.listener(move |this, _, _, cx| this.load_session(session_id.clone(), cx))),
                    )
                })),
        );

        sidebar.child(
            div()
                .mt_auto()
                .pt_3()
                .border_t_1()
                .border_color(theme.colors.border)
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(self.status.clone()),
        )
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let workspace = self
            .workspace
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("Workspace");
        let configuration = self.configuration.as_ref();
        let provider = configuration.map(|config| config.provider.as_str()).unwrap_or("Provider");
        let model = configuration.map(|config| config.model.as_str()).unwrap_or("Model");
        let theme_mode = if self.theme_mode == ThemeMode::Light {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        };
        let weak = cx.entity().downgrade();
        div()
            .w_full()
            .h(px(58.))
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .px_5()
            .border_b_1()
            .border_color(theme.colors.border)
            .child(div().text_lg().font_semibold().child(workspace.to_string()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(badge(provider.to_string(), theme.colors.accent, theme.colors.accent_foreground))
                    .child(badge(model.to_string(), theme.colors.secondary, theme.colors.secondary_foreground))
                    .child(
                        Button::new("toggle-theme")
                            .label(if self.theme_mode == ThemeMode::Light {
                                "Dark"
                            } else {
                                "Light"
                            })
                            .ghost()
                            .on_click(move |_, _, app| {
                                Theme::change(theme_mode, None, app);
                                let _updated = weak.update(app, |view, cx| {
                                    view.theme_mode = theme_mode;
                                    view.transcript.remeasure();
                                    cx.notify();
                                });
                            }),
                    ),
            )
    }

    fn render_transcript(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        let content = if self.transcript.is_empty() {
            div()
                .size_full()
                .flex()
                .px_8()
                .py_6()
                .child(
                    div()
                        .m_auto()
                        .max_w(px(640.))
                        .text_center()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(div().text_2xl().font_semibold().child("What should we work on?"))
                        .child(div().text_color(colors.muted_foreground).child(
                            "Choose a workspace, then start a task. VT Code will ask before protected actions.",
                        )),
                )
                .into_any_element()
        } else {
            list(
                self.transcript.list_state(),
                cx.processor(|view, index, _, cx| {
                    if let Some(item) = view.transcript.get(index) {
                        div()
                            .id(format!("transcript-row-{}", item.id))
                            .role(gpui_kit::Role::ListItem)
                            .test_support()
                            .w_full()
                            .px_8()
                            .pb_4()
                            .child(view.render_transcript_item(item, cx))
                            .into_any_element()
                    } else {
                        div().into_any_element()
                    }
                }),
            )
            .size_full()
            .py_6()
            .into_any_element()
        };
        div()
            .id("transcript-scroll")
            .role(gpui_kit::Role::List)
            .test_support()
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_hidden()
            .child(content)
    }

    fn render_transcript_item(&self, item: &TranscriptItem, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        if item.role == "tool" {
            let id = item.id.clone();
            let mut panel = div()
                .w_full()
                .max_w(px(860.))
                .border_1()
                .border_color(colors.border)
                .rounded_md()
                .overflow_hidden()
                .child(
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .justify_between()
                        .px_3()
                        .py_2()
                        .bg(colors.secondary)
                        .child(div().font_medium().child(item.title.clone()))
                        .child(
                            Button::new(format!("tool-{}", item.id))
                                .label(if item.expanded { "Hide details" } else { "Show details" })
                                .ghost()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.transcript.toggle_tool(&id);
                                    cx.notify();
                                })),
                        ),
                );
            if item.expanded {
                panel = panel.child(div().px_3().py_2().text_sm().child(item.text.clone()));
            }
            return panel.into_any_element();
        }

        let (label, bg) = match item.role.as_str() {
            "user" => ("You", colors.accent.opacity(0.12)),
            "notice" => (item.title.as_str(), colors.warning.opacity(0.12)),
            _ => (item.title.as_str(), colors.background),
        };
        let copy_text = item.text.clone();
        let card = div()
            .w_full()
            .max_w(px(860.))
            .flex()
            .flex_col()
            .gap_2()
            .rounded_md()
            .p_3()
            .bg(bg)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_sm().font_semibold().child(label.to_string()))
                    .child(Button::new(format!("copy-message-{}", item.id)).label("Copy").ghost().on_click(
                        move |_, _, app| {
                            app.write_to_clipboard(gpui_kit::ClipboardItem::new_string(copy_text.to_string()))
                        },
                    )),
            );
        card.child(render_markdown(format!("markdown-{}", item.id), item.markdown.clone()))
            .into_any_element()
    }

    fn render_composer(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        let mut panel = div().w_full().flex().flex_col().gap_2().px_8().pb_5().pt_2().child(
            div()
                .w_full()
                .border_1()
                .border_color(colors.border)
                .rounded_md()
                .p_2()
                // The composer submits on plain Enter, so the textarea
                // propagates the unconsumed `Enter` action up to this wrapper
                // instead of inserting a newline; Shift+Enter stays a newline.
                .on_action(cx.listener(|view, _: &Enter, window, cx| view.submit_prompt(window, cx)))
                .child(
                    Textarea::new(&self.composer)
                        .accessibility_id("composer")
                        .aria_label("Message to VT Code")
                        .h(px(100.)),
                ),
        );

        if let Some(error) = self.error.as_ref() {
            panel = panel.child(div().w_full().text_sm().text_color(colors.danger).child(error.clone()));
        }
        if let Some(interaction) = self.interaction.clone() {
            panel = panel.child(self.render_interaction(&interaction, window, cx));
        }

        let can_submit = self.selected_session_id.is_some() && !self.running && self.interaction.is_none();
        let mut buttons = div().w_full().flex().items_center().justify_between().child(
            div()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child("Enter to send, Shift+Enter for a new line. Tool access follows your VT Code security settings."),
        );
        if self.running {
            buttons = buttons.child(
                Button::new("cancel-turn")
                    .label("Cancel run")
                    .danger()
                    .on_click(cx.listener(|this, _, _, cx| this.cancel_turn(cx))),
            );
        } else {
            buttons = buttons.child(
                Button::new("send-prompt")
                    .label("Send")
                    .primary()
                    .disabled(!can_submit)
                    .on_click(cx.listener(|this, _, window, cx| this.submit_prompt(window, cx))),
            );
        }
        panel.child(buttons)
    }

    fn render_interaction(
        &self,
        interaction: &InteractionRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors;
        let mut panel = div()
            .w_full()
            .max_w(px(860.))
            .flex()
            .flex_col()
            .gap_2()
            .border_1()
            .border_color(colors.warning)
            .rounded_md()
            .p_4()
            .bg(colors.warning.opacity(0.08))
            .child(div().font_semibold().child(interaction.title.clone()));
        for detail in &interaction.details {
            panel = panel.child(div().text_sm().child(detail.clone()));
        }

        if interaction.kind == protocol::InteractionKind::Question && !interaction.steps.is_empty() {
            let step_index = self.interaction_step_index.min(interaction.steps.len().saturating_sub(1));
            if let Some(step) = interaction.steps.get(step_index) {
                panel = panel.child(self.render_question_step(step, step_index, interaction.steps.len(), window, cx));
            }
        } else {
            let choices = interaction
                .steps
                .first()
                .map(|step| step.choices.as_slice())
                .unwrap_or(interaction.choices.as_slice());
            for choice in choices {
                panel = panel.child(self.render_choice(choice, 0, window, cx));
            }
        }

        panel.child(
            div()
                .w_full()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    Button::new("cancel-interaction").label("Cancel request").ghost().on_click(
                        cx.listener(|this, _, window, cx| this.submit_interaction(Vec::new(), true, window, cx)),
                    ),
                )
                .when(
                    interaction.kind == protocol::InteractionKind::Question && !interaction.steps.is_empty(),
                    |row| {
                        row.child(
                            Button::new("continue-question")
                                .label(if self.interaction_step_index + 1 == interaction.steps.len() {
                                    "Submit answers"
                                } else {
                                    "Next question"
                                })
                                .primary()
                                .on_click(cx.listener(|this, _, window, cx| this.advance_interaction_step(window, cx))),
                        )
                    },
                ),
        )
    }

    fn render_question_step(
        &self,
        step: &InteractionStep,
        index: usize,
        total: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors;
        let mut content = div()
            .w_full()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(format!("Question {} of {total}", index + 1)),
            )
            .child(div().font_medium().child(step.question.clone()));
        for choice in &step.choices {
            content = content.child(self.render_choice(choice, index, window, cx));
        }
        if step.allow_freeform {
            content = content.child(
                div().w_full().border_1().border_color(colors.border).rounded_sm().p_2().child(
                    Textarea::new(&self.question_answer)
                        .accessibility_id("question-answer")
                        .aria_label("Your answer")
                        .h(px(72.)),
                ),
            );
        }
        content
    }

    fn render_choice(
        &self,
        choice: &InteractionChoice,
        step_index: usize,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let choice_id = choice.id.clone();
        let label = if choice.recommended {
            format!("{} (Recommended)", choice.label)
        } else {
            choice.label.clone()
        };
        let mut button = Button::new(format!("choice-{}", choice.id)).label(label).w_full().ghost();
        if let Some(description) = choice.description.as_ref() {
            button = button.tooltip(description.clone());
        }
        let _ = step_index;
        div().w_full().child(button.on_click(cx.listener(move |this, _, window, cx| {
            this.choose_interaction_option(step_index, choice_id.clone(), window, cx);
        })))
    }
}

impl Render for DesktopView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        let mut body = div()
            .size_full()
            .flex()
            .bg(colors.background)
            .text_color(colors.foreground)
            .child(self.render_sidebar(cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(self.render_toolbar(cx))
                    .child(self.render_transcript(cx))
                    .child(self.render_composer(window, cx)),
            );
        if self.workspace.is_none() {
            body = body.child(
                div().absolute().inset_0().flex().items_center().justify_center().child(
                    Button::new("open-first-workspace")
                        .label("Choose a workspace folder")
                        .primary()
                        .disabled(self.workspace_picker_open || !self.workspace_switch_idle())
                        .on_click(cx.listener(|this, _, window, cx| this.choose_workspace(window, cx))),
                ),
            );
        }
        body
    }
}

fn send_request(commands: &mpsc::Sender<BackendCommand>, method: &str, params: Value) -> bool {
    commands
        .try_send(BackendCommand::Request { method: method.to_string(), params })
        .is_ok()
}

fn badge(text: String, background: gpui_kit::Hsla, foreground: gpui_kit::Hsla) -> impl IntoElement {
    div()
        .rounded_full()
        .px_2()
        .py_1()
        .bg(background)
        .text_xs()
        .text_color(foreground)
        .child(text)
}

fn workspace_picker_options() -> PathPromptOptions {
    PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some("Choose workspace".into()),
    }
}

fn uuid_like_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    NEXT_ID.fetch_add(1, Ordering::Relaxed).to_string()
}

#[cfg(test)]
mod tests;
