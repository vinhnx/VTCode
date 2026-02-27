use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::progress::{ProgressReporter, ProgressState};
#[allow(unused_imports)]
use super::reasoning::{analyze_reasoning, is_giving_up_reasoning};

use anyhow::Result;
use tokio::sync::{Notify, mpsc};
use tokio::task;
use tokio::time::sleep;

use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_tui::InlineHandle;

use super::state::{CtrlCState, SessionStats};

pub(crate) struct SessionStatusContext<'a> {
    pub config: &'a CoreAgentConfig,
    pub message_count: usize,
    pub stats: &'a SessionStats,
    pub available_tools: usize,
}

pub(crate) async fn display_session_status(
    renderer: &mut AnsiRenderer,
    ctx: SessionStatusContext<'_>,
) -> Result<()> {
    renderer.line(MessageStyle::Info, "Session status:")?;
    renderer.line(
        MessageStyle::Info,
        &format!("  Model: {} ({})", ctx.config.model, ctx.config.provider),
    )?;
    renderer.line(
        MessageStyle::Info,
        &format!("  Workspace: {}", ctx.config.workspace.display()),
    )?;
    renderer.line(
        MessageStyle::Info,
        &format!("  Reasoning effort: {}", ctx.config.reasoning_effort),
    )?;
    renderer.line(
        MessageStyle::Info,
        &format!("  Messages so far: {}", ctx.message_count),
    )?;

    let used_tools = ctx.stats.sorted_tools();
    if used_tools.is_empty() {
        renderer.line(
            MessageStyle::Info,
            &format!("  Tools used: 0 / {}", ctx.available_tools),
        )?;
    } else {
        renderer.line(
            MessageStyle::Info,
            &format!(
                "  Tools used: {} / {} ({})",
                used_tools.len(),
                ctx.available_tools,
                used_tools.join(", ")
            ),
        )?;
    }

    Ok(())
}

#[allow(dead_code)]
pub(crate) async fn display_token_cost(
    renderer: &mut AnsiRenderer,
    _max_tokens: usize,
    prefix: &str,
) -> Result<()> {
    renderer.line(
        MessageStyle::Info,
        &format!("{prefix}Token tracking is disabled."),
    )?;
    Ok(())
}

pub(crate) struct PlaceholderGuard {
    handle: InlineHandle,
    restore: Option<String>,
}

impl PlaceholderGuard {
    pub(crate) fn new(handle: &InlineHandle, restore: Option<String>) -> Self {
        Self {
            handle: handle.clone(),
            restore,
        }
    }
}

impl Drop for PlaceholderGuard {
    fn drop(&mut self) {
        self.handle.set_placeholder(self.restore.clone());
    }
}

const SPINNER_UPDATE_INTERVAL_MS: u64 = 150;

#[allow(dead_code)]
pub(crate) struct PlaceholderSpinner {
    handle: InlineHandle,
    restore_left: Option<String>,
    restore_right: Option<String>,
    active: Arc<AtomicBool>,
    task: task::JoinHandle<()>,
    progress_state: Option<Arc<ProgressState>>,
    message_sender: Option<mpsc::UnboundedSender<String>>,
    defer_restore: Arc<AtomicBool>,
}

impl PlaceholderSpinner {
    pub(crate) fn with_progress(
        handle: &InlineHandle,
        restore_left: Option<String>,
        restore_right: Option<String>,
        message: impl Into<String>,
        progress_reporter: Option<&ProgressReporter>,
    ) -> Self {
        let base_message = message.into();
        let message_with_hint = if base_message.is_empty() {
            "Press Ctrl+C to cancel".to_string()
        } else {
            format!("{} (Press Ctrl+C to cancel)", base_message)
        };

        let active = Arc::new(AtomicBool::new(true));
        let spinner_active = active.clone();
        let spinner_handle = handle.clone();
        let restore_on_stop_left = restore_left.clone();
        let restore_on_stop_right = restore_right.clone();
        let status_right = restore_right.clone();
        let progress_reporter_arc = progress_reporter.cloned().map(Arc::new);

        let (message_sender, mut message_receiver) = mpsc::unbounded_channel::<String>();
        let message_sender_clone = message_sender.clone();
        let initial_display = message_with_hint.clone();

        spinner_handle.set_input_status(Some(initial_display.clone()), status_right.clone());

        let task = task::spawn(async move {
            let mut current_message = message_with_hint;
            let mut last_display = initial_display;
            while spinner_active.load(Ordering::SeqCst) {
                while let Ok(new_message) = message_receiver.try_recv() {
                    current_message = if new_message.is_empty() {
                        "Press Ctrl+C to cancel".to_string()
                    } else {
                        format!("{} (Press Ctrl+C to cancel)", new_message)
                    };
                }

                let progress_info = if let Some(progress_reporter) = progress_reporter_arc.as_ref()
                {
                    let progress = progress_reporter.progress_info().await;
                    let mut parts = vec![progress.message.clone()];

                    if progress.total > 0 && progress.percentage > 0 {
                        // Removed progress bar visualization from status bar
                        parts.push(format!("{:.0}%", progress.percentage));
                    }

                    let eta = progress.eta_formatted();
                    if eta != "Calculating..." && eta != "0s" {
                        parts.push(eta);
                    }
                    parts.join("  ")
                } else {
                    String::new()
                };

                let display = if progress_info.is_empty() {
                    current_message.clone()
                } else {
                    format!("{}: {}", current_message, progress_info)
                };

                if display != last_display {
                    spinner_handle.set_input_status(Some(display.clone()), status_right.clone());
                    last_display = display;
                }
                sleep(Duration::from_millis(SPINNER_UPDATE_INTERVAL_MS)).await;
            }

            spinner_handle.set_input_status(restore_on_stop_left, restore_on_stop_right);
        });

        Self {
            handle: handle.clone(),
            restore_left,
            restore_right,
            active,
            task,
            progress_state: progress_reporter.map(|r| r.get_state().clone()),
            message_sender: Some(message_sender_clone),
            defer_restore: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn new(
        handle: &InlineHandle,
        restore_left: Option<String>,
        restore_right: Option<String>,
        message: impl Into<String>,
    ) -> Self {
        let mut spinner = Self::with_progress(handle, restore_left, restore_right, message, None);
        spinner.message_sender = None;
        spinner
    }

    pub(crate) fn set_defer_restore(&self, defer: bool) {
        self.defer_restore.store(defer, Ordering::SeqCst);
    }

    #[allow(dead_code)]
    pub(crate) fn progress_state(&self) -> Option<Arc<ProgressState>> {
        self.progress_state.clone()
    }

    #[allow(dead_code)]
    pub(crate) fn update_message(&self, message: impl Into<String>) {
        if let Some(sender) = &self.message_sender {
            let _ = sender.send(message.into());
        }
    }

    pub(crate) fn finish(&self) {
        self.finish_with_restore(!self.defer_restore.load(Ordering::SeqCst));
    }

    pub(crate) fn set_reasoning_stage(&self, stage: Option<String>) {
        self.handle.set_reasoning_stage(stage);
    }

    pub(crate) fn finish_with_restore(&self, restore: bool) {
        if self.active.swap(false, Ordering::SeqCst) {
            self.task.abort();
            if restore {
                self.handle
                    .set_input_status(self.restore_left.clone(), self.restore_right.clone());
            }
        }
    }
}

impl Drop for PlaceholderSpinner {
    fn drop(&mut self) {
        self.finish();
        self.task.abort();
    }
}

#[derive(Default, Clone, Copy)]
pub(crate) struct StreamSpinnerOptions {
    pub defer_finish: bool,
    pub strip_proposed_plan_blocks: bool,
}

#[allow(dead_code)]
pub(crate) async fn stream_and_render_response(
    provider: &dyn uni::LLMProvider,
    request: uni::LLMRequest,
    spinner: &PlaceholderSpinner,
    renderer: &mut AnsiRenderer,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> Result<(uni::LLMResponse, bool), uni::LLMError> {
    stream_and_render_response_with_options(
        provider,
        request,
        spinner,
        renderer,
        ctrl_c_state,
        ctrl_c_notify,
        StreamSpinnerOptions::default(),
    )
    .await
}

pub(crate) async fn stream_and_render_response_with_options(
    provider: &dyn uni::LLMProvider,
    request: uni::LLMRequest,
    spinner: &PlaceholderSpinner,
    renderer: &mut AnsiRenderer,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    options: StreamSpinnerOptions,
) -> Result<(uni::LLMResponse, bool), uni::LLMError> {
    super::ui_interaction_stream::stream_and_render_response_with_options_impl(
        provider,
        request,
        spinner,
        renderer,
        ctrl_c_state,
        ctrl_c_notify,
        options,
    )
    .await
}
