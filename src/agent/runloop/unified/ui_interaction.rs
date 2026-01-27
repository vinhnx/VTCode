use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::progress::{ProgressReporter, ProgressState};
#[allow(unused_imports)]
use super::reasoning::{analyze_reasoning, is_giving_up_reasoning};
use vtcode_core::llm::providers::clean_reasoning_text;

use anyhow::{Error, Result};
use futures::StreamExt;
use indicatif::ProgressStyle;
use tokio::sync::{Notify, mpsc};
use tokio::task;
use tokio::time::sleep;

use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::llm::error_display;
use vtcode_core::llm::provider::{self as uni, LLMStreamEvent};
use vtcode_core::ui::tui::InlineHandle;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

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

const SPINNER_UPDATE_INTERVAL_MS: u64 = 150; // Slightly slower for better performance

/// Create a mini progress bar string using Unicode block characters
fn create_mini_progress_bar(percentage: u8, width: usize) -> String {
    if width == 0 {
        return String::new();
    }

    let filled = (percentage as usize * width) / 100;
    let mut bar = String::with_capacity(width + 4); // Extra space for brackets and partial block

    bar.push('▐'); // Left bracket
    for i in 0..width {
        if i < filled {
            bar.push('█');
        } else if i == filled && !percentage.is_multiple_of(100 / width as u8) {
            // Show partial progress for more precision
            let partial = match (percentage % (100 / width as u8)) * 8 / (100 / width as u8) {
                0..=1 => '▏',
                2..=3 => '▎',
                4..=5 => '▍',
                6..=7 => '▌',
                _ => '▋',
            };
            bar.push(partial);
        } else {
            bar.push('░');
        }
    }
    bar.push('▌'); // Right bracket

    bar
}

/// Create an indeterminate progress indicator that shows activity
fn create_indeterminate_progress_indicator(tick: u64) -> String {
    let patterns = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let pattern_index = (tick / 2 % patterns.len() as u64) as usize;
    patterns[pattern_index].to_string()
}

/// Get context-aware progress style based on operation type
fn get_progress_style_context(message: &str) -> ProgressStyleContext {
    if message.contains("thinking")
        || message.contains("reasoning")
        || message.contains("sending")
        || message.contains("receiving")
    {
        ProgressStyleContext::Llm
    } else if message.contains("tool")
        || message.contains("executing")
        || message.contains("running")
    {
        ProgressStyleContext::Tool
    } else {
        ProgressStyleContext::General
    }
}

#[derive(Clone, Copy)]
enum ProgressStyleContext {
    Llm,
    Tool,
    General,
}

struct SpinnerFrameGenerator {
    style: ProgressStyle,
    tick: u64,
}

impl SpinnerFrameGenerator {
    fn new() -> Self {
        // Use a more elaborate spinner style that's more visible
        let style = ProgressStyle::default_spinner()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]);
        Self { style, tick: 0 }
    }

    fn next_frame(&mut self) -> &str {
        let frame = self.style.get_tick_str(self.tick);
        self.tick = self.tick.wrapping_add(1);
        frame
    }
}

#[allow(dead_code)]
pub(crate) struct PlaceholderSpinner {
    handle: InlineHandle,
    restore_left: Option<String>,
    restore_right: Option<String>,
    active: Arc<AtomicBool>,
    task: task::JoinHandle<()>,
    progress_state: Option<Arc<ProgressState>>,
    message_sender: Option<mpsc::UnboundedSender<String>>,
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

        // Clone the progress reporter if it exists
        let progress_reporter_arc = progress_reporter.cloned().map(Arc::new);

        // Create message update channel
        let (message_sender, mut message_receiver) = mpsc::unbounded_channel::<String>();
        let message_sender_clone = message_sender.clone();

        // Set initial status
        spinner_handle.set_input_status(Some(message_with_hint.clone()), status_right.clone());

        let task = task::spawn(async move {
            let mut frames = SpinnerFrameGenerator::new();
            let mut current_message = message_with_hint;
            while spinner_active.load(Ordering::SeqCst) {
                // Check for message updates
                while let Ok(new_message) = message_receiver.try_recv() {
                    current_message = if new_message.is_empty() {
                        "Press Ctrl+C to cancel".to_string()
                    } else {
                        format!("{} (Press Ctrl+C to cancel)", new_message)
                    };
                }

                // Get progress information if available
                let progress_info = if let Some(progress_reporter) = progress_reporter_arc.as_ref()
                {
                    let progress = progress_reporter.progress_info().await;
                    let context = get_progress_style_context(&progress.message.to_lowercase());
                    let mut parts = vec![progress.message.clone()];

                    if progress.total > 0 && progress.percentage > 0 {
                        // Add mini progress bar (width 8 for more compact display) and percentage
                        let progress_bar = create_mini_progress_bar(progress.percentage, 8);
                        parts.push(format!("{} {:.0}%", progress_bar, progress.percentage));
                    } else if progress.total == 0 && !progress.message.is_empty() {
                        // For indeterminate progress, show context-aware activity indicator
                        let activity_indicator = match context {
                            ProgressStyleContext::Llm => {
                                // Use pulsing dots for LLM operations (thinking/processing)
                                let dots_count = (frames.tick / 3 % 4) as usize;
                                "⠋⠙⠹⠸".chars().nth(dots_count).unwrap_or('⠋').to_string()
                            }
                            ProgressStyleContext::Tool => {
                                // Use spinning indicator for tool operations
                                create_indeterminate_progress_indicator(frames.tick)
                            }
                            ProgressStyleContext::General => {
                                // Default indeterminate indicator
                                create_indeterminate_progress_indicator(frames.tick)
                            }
                        };
                        parts.push(activity_indicator);
                    }

                    let eta = progress.eta_formatted();
                    if eta != "Calculating..." && eta != "0s" {
                        // Only show ETA if it's meaningful (not "Calculating..." or "0s")
                        parts.push(eta);
                    }
                    parts.join("  ")
                } else {
                    String::new()
                };

                let frame = frames.next_frame();
                let display = if progress_info.is_empty() {
                    format!("{} {}", frame, current_message)
                } else {
                    format!("{} {}: {}", frame, current_message, progress_info)
                };

                // Update the status with spinner animation and progress
                spinner_handle.set_input_status(Some(display), status_right.clone());
                sleep(Duration::from_millis(SPINNER_UPDATE_INTERVAL_MS)).await;
            }

            // Restore input status when done
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
        }
    }

    /// Create a new spinner without progress reporting (backward compatibility)
    pub(crate) fn new(
        handle: &InlineHandle,
        restore_left: Option<String>,
        restore_right: Option<String>,
        message: impl Into<String>,
    ) -> Self {
        let mut spinner = Self::with_progress(handle, restore_left, restore_right, message, None);
        // For backward compatibility, we don't expose message_sender for the old API
        spinner.message_sender = None;
        spinner
    }

    /// Get the progress state if available
    #[allow(dead_code)]
    pub(crate) fn progress_state(&self) -> Option<Arc<ProgressState>> {
        self.progress_state.clone()
    }

    /// Update the spinner message dynamically
    #[allow(dead_code)]
    pub(crate) fn update_message(&self, message: impl Into<String>) {
        if let Some(sender) = &self.message_sender {
            let _ = sender.send(message.into());
        }
    }

    pub(crate) fn finish(&self) {
        if self.active.swap(false, Ordering::SeqCst) {
            // Abort the spinner task first to prevent it from updating the input status
            // after we restore it (race condition fix)
            self.task.abort();
            // Restore the UI state
            self.handle
                .set_input_status(self.restore_left.clone(), self.restore_right.clone());
            // Note: We don't change input enabled/visible state since we didn't disable it in the first place
        }
    }
}

impl Drop for PlaceholderSpinner {
    fn drop(&mut self) {
        self.finish();
        self.task.abort();
    }
}

fn map_render_error(provider_name: &str, err: Error) -> uni::LLMError {
    let formatted_error = error_display::format_llm_error(
        provider_name,
        &format!("Failed to render streaming output: {}", err),
    );
    uni::LLMError::Provider {
        message: formatted_error,
        metadata: None,
    }
}

fn stream_plain_response_delta(
    renderer: &mut AnsiRenderer,
    style: MessageStyle,
    indent: &str,
    pending_indent: &mut bool,
    delta: &str,
) -> Result<()> {
    for chunk in delta.split_inclusive('\n') {
        if chunk.is_empty() {
            continue;
        }

        if let Some(text) = chunk.strip_suffix('\n') {
            if !text.is_empty() {
                if *pending_indent && !indent.is_empty() {
                    renderer.inline_with_style(style, indent)?;
                }
                renderer.inline_with_style(style, text)?;
                *pending_indent = false;
            }
            renderer.inline_with_style(style, "\n")?;
            *pending_indent = true;
        } else {
            if *pending_indent && !indent.is_empty() {
                renderer.inline_with_style(style, indent)?;
                *pending_indent = false;
            }
            renderer.inline_with_style(style, chunk)?;
        }
    }

    Ok(())
}

fn reasoning_matches_content(reasoning: &str, content: &str) -> bool {
    let cleaned_reasoning = clean_reasoning_text(reasoning);
    let cleaned_content = clean_reasoning_text(content);
    !cleaned_reasoning.is_empty()
        && !cleaned_content.is_empty()
        && cleaned_reasoning == cleaned_content
}

fn common_prefix_len(a: &str, b: &str) -> usize {
    let mut len = 0;
    for (left, right) in a.chars().zip(b.chars()) {
        if left != right {
            break;
        }
        len += left.len_utf8();
    }
    len
}

#[derive(Default)]
struct StreamingReasoningState {
    // Tracks buffered reasoning delta during streaming
    buffered: String,
    // Whether reasoning should be rendered inline during streaming
    render_inline: bool,
    // Track whether we've started streaming (for prefix)
    started: bool,
    // Whether reasoning output has been rendered
    rendered_any: bool,
}

impl StreamingReasoningState {
    fn new(inline_enabled: bool) -> Self {
        Self {
            buffered: String::new(),
            render_inline: inline_enabled,
            started: false,
            rendered_any: false,
        }
    }

    fn handle_delta(&mut self, renderer: &mut AnsiRenderer, delta: &str) -> Result<()> {
        if !self.render_inline {
            self.buffered.push_str(delta);
            return Ok(());
        }

        // For inline rendering: stream reasoning like response tokens
        renderer.inline_with_style(MessageStyle::Reasoning, delta)?;
        self.rendered_any = true;
        Ok(())
    }

    fn flush_pending(&mut self, renderer: &mut AnsiRenderer) -> Result<()> {
        if !self.buffered.is_empty() {
            let cleaned = clean_reasoning_text(&self.buffered);
            if !cleaned.is_empty() {
                // If we have buffered content and are rendering inline, add newline first
                if self.render_inline && self.started {
                    renderer.inline_with_style(MessageStyle::Reasoning, "\n")?;
                }
                renderer.line(MessageStyle::Reasoning, &cleaned)?;
                self.rendered_any = true;
            }
            self.buffered.clear();
        }
        Ok(())
    }

    fn finalize(
        &mut self,
        renderer: &mut AnsiRenderer,
        final_reasoning: Option<&str>,
        reasoning_already_emitted: bool,
        suppress_reasoning: bool,
    ) -> Result<()> {
        if suppress_reasoning {
            self.buffered.clear();
            return Ok(());
        }

        // Flush any buffered reasoning first
        self.flush_pending(renderer)?;

        // Only render final reasoning if it wasn't already emitted during streaming
        // This prevents duplicate reasoning output
        if !reasoning_already_emitted
            && let Some(reasoning_text) = final_reasoning
            && !reasoning_text.trim().is_empty()
        {
            let cleaned_reasoning = clean_reasoning_text(reasoning_text);
            if !cleaned_reasoning.trim().is_empty() {
                renderer.line(MessageStyle::Reasoning, &cleaned_reasoning)?;
                self.rendered_any = true;

                // Chain-of-Thought Monitoring: Analyze reasoning for concerns
                // This enables early intervention if the agent is going down a wrong path
                use super::reasoning::analyze_reasoning;
                let analysis = analyze_reasoning(&cleaned_reasoning);
                if analysis.has_concerns() {
                    // Log concern for debugging/visibility
                    // In production, this could trigger UI indicators or interrupt
                    tracing::debug!(
                        concern = ?analysis.priority_concern(),
                        "Reasoning concern detected in CoT output"
                    );
                }
            }
        }
        Ok(())
    }

    fn handle_stream_failure(&mut self, _renderer: &mut AnsiRenderer) -> Result<()> {
        self.buffered.clear();
        Ok(())
    }

    fn rendered_reasoning(&self) -> bool {
        self.rendered_any
    }
}

pub(crate) async fn stream_and_render_response(
    provider: &dyn uni::LLMProvider,
    request: uni::LLMRequest,
    spinner: &PlaceholderSpinner,
    renderer: &mut AnsiRenderer,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> Result<(uni::LLMResponse, bool), uni::LLMError> {
    let provider_name = provider.name();

    // Check for cancellation before starting stream
    if ctrl_c_state.is_cancel_requested() {
        spinner.finish();
        return Err(uni::LLMError::Provider {
            message: error_display::format_llm_error(provider_name, "Interrupted by user"),
            metadata: None,
        });
    }

    let supports_streaming_markdown = renderer.supports_streaming_markdown();

    // Start stream with cancellation support
    let stream_future = provider.stream(request);
    tokio::pin!(stream_future);

    if ctrl_c_state.is_cancel_requested() || ctrl_c_state.is_exit_requested() {
        spinner.finish();
        return Err(uni::LLMError::Provider {
            message: error_display::format_llm_error(provider_name, "Interrupted by user"),
            metadata: None,
        });
    }

    let mut stream = tokio::select! {
        biased;
        _ = ctrl_c_notify.notified() => {
            spinner.finish();
            return Err(uni::LLMError::Provider { message: error_display::format_llm_error(provider_name, "Interrupted by user"), metadata: None });
        }
        result = stream_future => result?,
    };

    let mut final_response: Option<uni::LLMResponse> = None;
    let mut aggregated = String::new();
    let mut spinner_active = true;
    let mut rendered_line_count = 0usize;
    let response_style = MessageStyle::Response;
    let response_indent = response_style.indent();
    let mut needs_indent = true;
    let finish_spinner = |active: &mut bool| {
        if *active {
            spinner.finish();
            *active = false;
        }
    };
    let mut emitted_tokens = false;
    let mut reasoning_state = StreamingReasoningState::new(supports_streaming_markdown);
    let mut spinner_message_updated = false;
    let mut reasoning_accumulated = String::new();
    let mut pending_content = String::new();
    let mut content_suppressed = false;
    const MAX_PENDING_CONTENT_BYTES: usize = 4_096;

    // Track streaming progress
    let mut token_count = 0;
    let mut reasoning_token_count = 0;
    let mut last_progress_update = std::time::Instant::now();
    let mut reasoning_emitted = false;

    loop {
        if ctrl_c_state.is_cancel_requested() || ctrl_c_state.is_exit_requested() {
            finish_spinner(&mut spinner_active);
            reasoning_state
                .handle_stream_failure(renderer)
                .map_err(|err| map_render_error(provider_name, err))?;
            return Err(uni::LLMError::Provider {
                message: error_display::format_llm_error(provider_name, "Interrupted by user"),
                metadata: None,
            });
        }

        let maybe_event = tokio::select! {
            biased;

            _ = ctrl_c_notify.notified() => {
                finish_spinner(&mut spinner_active);
                reasoning_state
                    .handle_stream_failure(renderer)
                    .map_err(|err| map_render_error(provider_name, err))?;
                return Err(uni::LLMError::Provider { message: error_display::format_llm_error(provider_name, "Interrupted by user"), metadata: None });
            }
            event = stream.next() => event,
        };

        let Some(event_result) = maybe_event else {
            break;
        };

        match event_result {
            Ok(LLMStreamEvent::Token { delta }) => {
                token_count += 1;

                // Ensure any buffered reasoning is rendered before the first response token
                if !reasoning_emitted && reasoning_token_count > 0 {
                    reasoning_state
                        .flush_pending(renderer)
                        .map_err(|err| map_render_error(provider_name, err))?;
                    reasoning_emitted = true;
                }

                if !spinner_message_updated {
                    spinner.update_message("Receiving response...");
                    spinner_message_updated = true;
                } else if last_progress_update.elapsed() >= std::time::Duration::from_millis(500) {
                    // Update progress message every 500ms with token count
                    spinner
                        .update_message(format!("Receiving response... ({} tokens)", token_count));
                    last_progress_update = std::time::Instant::now();
                }
                finish_spinner(&mut spinner_active);
                if !reasoning_accumulated.trim().is_empty() && !emitted_tokens {
                    pending_content.push_str(&delta);
                    let prefix_len = common_prefix_len(&reasoning_accumulated, &pending_content);
                    let reasoning_prefix = !reasoning_accumulated.is_empty()
                        && prefix_len == reasoning_accumulated.len();
                    let should_flush = !reasoning_prefix
                        || pending_content.len() >= MAX_PENDING_CONTENT_BYTES;

                    if should_flush {
                        let pending = std::mem::take(&mut pending_content);
                        let render_text = if reasoning_prefix {
                            pending
                                .get(prefix_len..)
                                .unwrap_or("")
                                .to_string()
                        } else {
                            pending
                        };

                        if reasoning_prefix
                            && render_text.is_empty()
                            && (reasoning_state.rendered_reasoning() || reasoning_emitted)
                        {
                            content_suppressed = true;
                            continue;
                        }

                        aggregated.push_str(&render_text);
                        if supports_streaming_markdown {
                            rendered_line_count = renderer
                                .stream_markdown_response(&aggregated, rendered_line_count)
                                .map_err(|err| map_render_error(provider_name, err))?;
                        } else {
                            stream_plain_response_delta(
                                renderer,
                                response_style,
                                response_indent,
                                &mut needs_indent,
                                &render_text,
                            )
                            .map_err(|err| map_render_error(provider_name, err))?;
                        }
                        emitted_tokens = true;
                        if reasoning_prefix
                            && (reasoning_state.rendered_reasoning() || reasoning_emitted)
                        {
                            content_suppressed = true;
                        }
                    }
                    continue;
                }

                aggregated.push_str(&delta);
                if supports_streaming_markdown {
                    rendered_line_count = renderer
                        .stream_markdown_response(&aggregated, rendered_line_count)
                        .map_err(|err| map_render_error(provider_name, err))?;
                } else {
                    stream_plain_response_delta(
                        renderer,
                        response_style,
                        response_indent,
                        &mut needs_indent,
                        &delta,
                    )
                    .map_err(|err| map_render_error(provider_name, err))?;
                }
                emitted_tokens = true;
            }
            Ok(LLMStreamEvent::Reasoning { delta }) => {
                reasoning_token_count += 1;
                if !spinner_message_updated {
                    spinner.update_message("Processing reasoning...");
                    spinner_message_updated = true;
                } else if last_progress_update.elapsed() >= std::time::Duration::from_millis(500) {
                    // Update progress message every 500ms with reasoning token count
                    spinner.update_message(format!(
                        "Processing reasoning... ({} tokens)",
                        reasoning_token_count
                    ));
                    last_progress_update = std::time::Instant::now();
                }
                finish_spinner(&mut spinner_active);
                reasoning_accumulated.push_str(&delta);
                reasoning_state
                    .handle_delta(renderer, &delta)
                    .map_err(|err| map_render_error(provider_name, err))?;
                // Mark reasoning as emitted to prevent duplicate rendering in finalize()
                reasoning_emitted = true;
            }
            Ok(LLMStreamEvent::Completed { response }) => {
                final_response = Some(*response);
            }
            Err(err) => {
                finish_spinner(&mut spinner_active);
                reasoning_state
                    .handle_stream_failure(renderer)
                    .map_err(|render_err| map_render_error(provider_name, render_err))?;
                return Err(err);
            }
        }
    }

    finish_spinner(&mut spinner_active);

    let response = match final_response {
        Some(response) => response,
        None => {
            reasoning_state
                .handle_stream_failure(renderer)
                .map_err(|err| map_render_error(provider_name, err))?;
            let formatted_error = error_display::format_llm_error(
                provider_name,
                "Stream ended without a completion event",
            );
            return Err(uni::LLMError::Provider {
                message: formatted_error,
                metadata: None,
            });
        }
    };

    if !pending_content.is_empty() && !content_suppressed {
        let prefix_len = common_prefix_len(&reasoning_accumulated, &pending_content);
        let reasoning_prefix = !reasoning_accumulated.is_empty()
            && prefix_len == reasoning_accumulated.len();
        let pending = std::mem::take(&mut pending_content);
        let render_text = if reasoning_prefix {
            pending.get(prefix_len..).unwrap_or("").to_string()
        } else {
            pending
        };

        if reasoning_prefix
            && render_text.is_empty()
            && (reasoning_state.rendered_reasoning() || reasoning_emitted)
        {
            content_suppressed = true;
        } else {
            aggregated.push_str(&render_text);
            if supports_streaming_markdown {
                let prev_count = if aggregated.trim().is_empty() {
                    0
                } else {
                    rendered_line_count
                };
                renderer
                    .stream_markdown_response(&aggregated, prev_count)
                    .map_err(|err| map_render_error(provider_name, err))?;
            } else {
                stream_plain_response_delta(
                    renderer,
                    response_style,
                    response_indent,
                    &mut needs_indent,
                    &render_text,
                )
                .map_err(|err| map_render_error(provider_name, err))?;
            }
            emitted_tokens = true;
            if reasoning_prefix
                && (reasoning_state.rendered_reasoning() || reasoning_emitted)
            {
                content_suppressed = true;
            }
        }
    }

    // CRITICAL: Ensure content is ALWAYS displayed.
    // If response has content but we didn't stream any tokens, render it now.
    // This handles providers that send content only in the Completed event.
    if !content_suppressed && let Some(content) = response.content.as_deref() {
        let content_trimmed = content.trim();
        if !content_trimmed.is_empty() {
            let reasoning_dupes_content = response
                .reasoning
                .as_deref()
                .map(|reasoning| reasoning_matches_content(reasoning, content))
                .unwrap_or(false);
            if reasoning_dupes_content
                && (reasoning_state.rendered_reasoning()
                    || reasoning_emitted
                    || !reasoning_accumulated.is_empty())
            {
                // Skip rendering duplicated content when reasoning already rendered.
                // Leave `aggregated` untouched to avoid showing content twice.
            } else {
            // Check if we already rendered this content via Token events
            let already_rendered =
                !aggregated.trim().is_empty() && aggregated.trim() == content_trimmed;

            if !already_rendered {
                // Content wasn't rendered yet - render it now
                // First, flush any pending reasoning
                let suppress_reasoning = response
                    .reasoning
                    .as_deref()
                    .map(|reasoning| reasoning_matches_content(reasoning, content))
                    .unwrap_or(false);
                reasoning_state
                    .finalize(
                        renderer,
                        response.reasoning.as_deref(),
                        reasoning_emitted,
                        suppress_reasoning,
                    )
                    .map_err(|err| map_render_error(provider_name, err))?;

                // Now render the actual content
                if supports_streaming_markdown {
                    // If we had some streamed content, replace it; otherwise add new
                    let prev_count = if aggregated.trim().is_empty() {
                        0
                    } else {
                        rendered_line_count
                    };
                    renderer
                        .stream_markdown_response(content, prev_count)
                        .map_err(|err| map_render_error(provider_name, err))?;
                } else {
                    // Add newline after reasoning if needed
                    if !aggregated.is_empty() {
                        renderer
                            .line(MessageStyle::Response, "")
                            .map_err(|err| map_render_error(provider_name, err))?;
                    }
                    renderer
                        .line(MessageStyle::Response, content)
                        .map_err(|err| map_render_error(provider_name, err))?;
                }
                emitted_tokens = true;
                aggregated = content.to_string();
            }
            }
        }
    }

    // Finalize reasoning display (only if we haven't already in the content block above)
    if response.content.is_none() || aggregated.trim().is_empty() {
        let suppress_reasoning = response
            .reasoning
            .as_deref()
            .zip(response.content.as_deref())
            .map(|(reasoning, content)| reasoning_matches_content(reasoning, content))
            .unwrap_or(false);
        reasoning_state
            .finalize(
                renderer,
                response.reasoning.as_deref(),
                reasoning_emitted,
                suppress_reasoning,
            )
            .map_err(|err| map_render_error(provider_name, err))?;
    }

    // Fallback: Some providers only populate `reasoning` (no streamed tokens, no `content`).
    // In that case, render the reasoning as the user-visible response to avoid an empty output.
    if !emitted_tokens
        && aggregated.trim().is_empty()
        && response.content.is_none()
        && !reasoning_state.rendered_reasoning()
        && let Some(reasoning) = response.reasoning.as_deref()
    {
        let reasoning_trimmed = clean_reasoning_text(reasoning.trim());
        if !reasoning_trimmed.is_empty() {
            if supports_streaming_markdown {
                renderer
                    .stream_markdown_response(&reasoning_trimmed, 0)
                    .map_err(|err| map_render_error(provider_name, err))?;
            } else {
                renderer
                    .line(MessageStyle::Response, &reasoning_trimmed)
                    .map_err(|err| map_render_error(provider_name, err))?;
            }
            emitted_tokens = true;
            aggregated = reasoning_trimmed.to_string();
        }
    }

    if !supports_streaming_markdown && !aggregated.trim().is_empty() {
        renderer
            .line(MessageStyle::Response, "")
            .map_err(|err| map_render_error(provider_name, err))?;
    }

    Ok((response, emitted_tokens))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use std::sync::Arc;
    use tokio::sync::{Notify, mpsc};
    use vtcode_core::ui::tui::InlineCommand;

    #[derive(Clone)]
    struct CompletedOnlyProvider {
        content: Option<String>,
        reasoning: Option<String>,
    }

    #[async_trait::async_trait]
    impl uni::LLMProvider for CompletedOnlyProvider {
        fn name(&self) -> &str {
            "test-provider"
        }

        fn supports_streaming(&self) -> bool {
            true
        }

        async fn generate(
            &self,
            _request: uni::LLMRequest,
        ) -> Result<uni::LLMResponse, uni::LLMError> {
            Ok(uni::LLMResponse {
                content: self.content.clone(),
                tool_calls: None,
                usage: None,
                finish_reason: uni::FinishReason::Stop,
                reasoning: self.reasoning.clone(),
                reasoning_details: None,
                organization_id: None,
                request_id: None,
                tool_references: vec![],
            })
        }

        async fn stream(&self, request: uni::LLMRequest) -> Result<uni::LLMStream, uni::LLMError> {
            let response = self.generate(request).await?;
            Ok(Box::pin(stream::once(async {
                Ok(uni::LLMStreamEvent::Completed { response })
            })))
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["test-model".to_string()]
        }

        fn validate_request(&self, _request: &uni::LLMRequest) -> Result<(), uni::LLMError> {
            Ok(())
        }
    }

    fn build_request() -> uni::LLMRequest {
        uni::LLMRequest {
            messages: Vec::new(),
            system_prompt: None,
            tools: None,
            model: "test-model".to_string(),
            max_tokens: None,
            temperature: None,
            stream: true,
            output_format: None,
            tool_choice: None,
            parallel_tool_calls: None,
            parallel_tool_config: None,
            reasoning_effort: None,
            effort: None,
            verbosity: None,
            top_p: None,
            top_k: None,
            presence_penalty: None,
            frequency_penalty: None,
            stop_sequences: None,
            thinking_budget: None,
            betas: None,
            prefill: None,
            character_reinforcement: false,
            character_name: None,
            coding_agent_settings: None,
        }
    }

    fn build_spinner() -> PlaceholderSpinner {
        let (tx, _rx) = mpsc::unbounded_channel::<InlineCommand>();
        let handle = InlineHandle::new_for_tests(tx);
        PlaceholderSpinner::new(&handle, None, None, "")
    }

    #[tokio::test]
    async fn renders_completed_only_content() {
        let provider = CompletedOnlyProvider {
            content: Some("hello world".to_string()),
            reasoning: None,
        };
        let request = build_request();
        let spinner = build_spinner();
        let mut renderer = AnsiRenderer::stdout();
        let ctrl_c_state = CtrlCState::new();
        let ctrl_c_notify = Arc::new(Notify::new());

        let (resp, emitted) = stream_and_render_response(
            &provider,
            request,
            &spinner,
            &mut renderer,
            &Arc::new(ctrl_c_state),
            &ctrl_c_notify,
        )
        .await
        .expect("stream should succeed");

        assert!(
            emitted,
            "should mark emitted tokens when content is rendered"
        );
        assert_eq!(resp.content.as_deref(), Some("hello world"));
    }

    #[tokio::test]
    async fn renders_reasoning_when_no_content() {
        let provider = CompletedOnlyProvider {
            content: None,
            reasoning: Some("because reason".to_string()),
        };
        let request = build_request();
        let spinner = build_spinner();
        let mut renderer = AnsiRenderer::stdout();
        let ctrl_c_state = CtrlCState::new();
        let ctrl_c_notify = Arc::new(Notify::new());

        let (resp, emitted) = stream_and_render_response(
            &provider,
            request,
            &spinner,
            &mut renderer,
            &Arc::new(ctrl_c_state),
            &ctrl_c_notify,
        )
        .await
        .expect("stream should succeed");

        assert!(
            emitted,
            "should mark emitted tokens when reasoning is rendered"
        );
        assert!(resp.content.is_none(), "content should remain none");
    }
}
