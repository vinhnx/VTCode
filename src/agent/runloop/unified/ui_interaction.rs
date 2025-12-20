use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::progress::{ProgressReporter, ProgressState};

use anyhow::{Error, Result};
use futures::StreamExt;
use indicatif::ProgressStyle;
use tokio::sync::{Notify, mpsc};
use tokio::task;
use tokio::time::sleep;

use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::token_budget::TokenBudgetManager;
use vtcode_core::llm::error_display;
use vtcode_core::llm::provider::{self as uni, LLMStreamEvent};
use vtcode_core::ui::tui::InlineHandle;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::state::{CtrlCState, SessionStats};

pub(crate) struct SessionStatusContext<'a> {
    pub config: &'a CoreAgentConfig,
    pub message_count: usize,
    pub stats: &'a SessionStats,
    pub token_budget: &'a TokenBudgetManager,
    pub token_budget_enabled: bool,
    pub max_tokens: usize,
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

    display_token_cost(
        renderer,
        ctx.token_budget,
        ctx.token_budget_enabled,
        ctx.max_tokens,
        "  ",
    )
    .await?;

    Ok(())
}

pub(crate) async fn display_token_cost(
    renderer: &mut AnsiRenderer,
    token_budget: &TokenBudgetManager,
    token_budget_enabled: bool,
    max_tokens: usize,
    prefix: &str,
) -> Result<()> {
    if !token_budget_enabled {
        renderer.line(
            MessageStyle::Info,
            &format!("{prefix}Token tracking is disabled for this session."),
        )?;
        return Ok(());
    }

    let stats = token_budget.get_stats().await;
    let remaining = token_budget.remaining_tokens().await;
    let usage = stats.usage_percentage(max_tokens);
    renderer.line(
        MessageStyle::Info,
        &format!(
            "{prefix}Token usage: {} tokens (~{:.1}% of {})",
            stats.total_tokens, usage, max_tokens
        ),
    )?;
    renderer.line(
        MessageStyle::Info,
        &format!(
            "{prefix}Breakdown – system: {} · user: {} · assistant: {} · tool: {} · ledger: {}",
            stats.system_prompt_tokens,
            stats.user_messages_tokens,
            stats.assistant_messages_tokens,
            stats.tool_results_tokens,
            stats.decision_ledger_tokens
        ),
    )?;
    renderer.line(
        MessageStyle::Info,
        &format!("{prefix}Remaining budget (approx): {}", remaining),
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
    /// Create a new spinner with progress reporting support
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

const REASONING_HEADING: &str = "Thinking";
const REASONING_PREFIX: &str = "Thinking: ";
const INLINE_REASONING_RENDER_CHUNK: usize = 120;
/// Maximum reasoning to display; longer reasoning is truncated with ellipsis
const MAX_REASONING_DISPLAY_CHARS: usize = 500;

/// Check if reasoning contains giving-up language
fn is_giving_up_reasoning(reasoning: &str) -> bool {
    let lower = reasoning.to_lowercase();
    // Check for patterns indicating the agent wants to give up
    lower.contains("complex") && lower.contains("stop")
        || lower.contains("probably stop")
        || lower.contains("give up")
        || lower.contains("can't continue")
        || lower.contains("unable to continue")
        || lower.contains("too complex") && lower.contains("stop")
}

/// Replace giving-up reasoning with constructive, action-oriented reasoning
fn get_constructive_reasoning(original: &str) -> String {
    let lower = original.to_lowercase();

    // Provide specific, actionable guidance based on context
    if lower.contains("pdf") || lower.contains("file") || lower.contains("path") {
        "File system issue detected. Solution: 1) Verify directory exists with list_files, 2) Use absolute paths, 3) Create missing directories, 4) Try alternative output locations.".to_string()
    } else if lower.contains("tool")
        || lower.contains("execute")
        || lower.contains("code")
        || lower.contains("python")
    {
        "Tool execution failed. Solution: 1) Check if tool exists with bash 'which command', 2) Install missing dependencies, 3) Verify file permissions, 4) Try alternative tools or approaches.".to_string()
    } else if lower.contains("permission") || lower.contains("access") || lower.contains("denied") {
        "Permission issue detected. Solution: 1) Check file permissions with ls -la, 2) Use sudo for system paths, 3) Work in user-writable directories, 4) Adjust permissions with chmod.".to_string()
    } else if lower.contains("install") || lower.contains("package") || lower.contains("module") {
        "Dependency issue detected. Solution: 1) Check if package is installed, 2) Use pip install <package>, 3) Verify Python environment, 4) Try alternative packages.".to_string()
    } else {
        "Encountered complexity. Solution: 1) Break down the problem, 2) Try 2-3 different approaches, 3) Use alternative strategies, 4) Verify assumptions and constraints.".to_string()
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

#[derive(Default)]
struct StreamingReasoningState {
    aggregated: String,
    inline_line_count: usize,
    last_rendered: Vec<String>,
    cli_prefix_printed: bool,
    cli_pending_indent: bool,
    inline_enabled: bool,
    pending_inline: String,
}

impl StreamingReasoningState {
    fn new(inline_enabled: bool) -> Self {
        Self {
            inline_enabled,
            ..Self::default()
        }
    }

    fn handle_delta(&mut self, renderer: &mut AnsiRenderer, delta: &str) -> Result<()> {
        if delta.trim().is_empty() {
            return Ok(());
        }

        self.append_delta(delta);

        if self.inline_enabled {
            self.pending_inline.push_str(delta);
            if self.pending_inline.len() >= INLINE_REASONING_RENDER_CHUNK
                || self.pending_inline.contains('\n')
            {
                self.pending_inline.clear();
                self.render_inline(renderer)?;
            }
            Ok(())
        } else {
            self.render_cli(renderer, delta)
        }
    }

    fn finalize(
        &mut self,
        renderer: &mut AnsiRenderer,
        final_reasoning: Option<&str>,
    ) -> Result<()> {
        if self.inline_enabled {
            // Render any pending inline content that wasn't rendered yet
            if !self.pending_inline.is_empty() {
                self.render_inline(renderer)?;
            }
            // Clear pending buffer
            self.pending_inline.clear();

            // Update aggregated if final reasoning differs (normalize whitespace for comparison)
            let mut content_changed = false;
            if let Some(reasoning) = final_reasoning.map(str::trim) {
                let normalized_final = Self::normalize_whitespace(reasoning);
                let normalized_agg = Self::normalize_whitespace(self.aggregated.trim());

                if !normalized_final.is_empty() && normalized_final != normalized_agg {
                    self.aggregated = reasoning.to_string();
                    content_changed = true;
                }
            }

            // Only render if content actually changed
            if content_changed {
                self.render_inline(renderer)?;
            }
            Ok(())
        } else {
            // CLI mode: finalize the newline
            self.finalize_cli(renderer)?;

            // Display final reasoning if available and either:
            // 1. It wasn't streamed at all, OR
            // 2. It was truncated during streaming (we have more content now)
            if let Some(reasoning) = final_reasoning.map(str::trim)
                && !reasoning.is_empty()
            {
                let aggregated_trimmed = self.aggregated.trim();
                let is_truncated = aggregated_trimmed.ends_with("...");
                let was_not_streamed = aggregated_trimmed.is_empty();

                // Show the reasoning if it wasn't displayed yet OR if what was displayed was truncated
                if was_not_streamed
                    || (is_truncated && reasoning.len() > aggregated_trimmed.len() - 3)
                {
                    // Only show if it's genuinely new content or is different from what was shown
                    if was_not_streamed
                        || reasoning
                            != &aggregated_trimmed[..aggregated_trimmed.len().saturating_sub(3)]
                    {
                        renderer.line(
                            MessageStyle::Reasoning,
                            &format!("{REASONING_PREFIX}{reasoning}"),
                        )?;
                        self.aggregated = reasoning.to_string();
                    }
                }
            }
            Ok(())
        }
    }

    fn normalize_whitespace(text: &str) -> String {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn handle_stream_failure(&mut self, renderer: &mut AnsiRenderer) -> Result<()> {
        if !self.inline_enabled {
            self.finalize_cli(renderer)?;
        }
        Ok(())
    }

    fn append_delta(&mut self, delta: &str) {
        let delta = if self.aggregated.is_empty() {
            delta.trim_start_matches(['\n', '\r'])
        } else {
            delta
        };

        if delta.is_empty() {
            return;
        }

        self.aggregated.push_str(delta);
    }

    fn render_inline(&mut self, renderer: &mut AnsiRenderer) -> Result<()> {
        let lines = self.display_lines();
        if lines.is_empty() || lines == self.last_rendered {
            return Ok(());
        }

        renderer.render_reasoning_stream(&lines, &mut self.inline_line_count)?;
        self.last_rendered = lines;
        Ok(())
    }

    fn render_cli(&mut self, renderer: &mut AnsiRenderer, delta: &str) -> Result<()> {
        if !self.cli_prefix_printed {
            let indent = MessageStyle::Reasoning.indent();
            if !indent.is_empty() {
                renderer.inline_with_style(MessageStyle::Reasoning, indent)?;
            }
            renderer.inline_with_style(MessageStyle::Reasoning, REASONING_PREFIX)?;
            self.cli_prefix_printed = true;
            self.cli_pending_indent = false;
        }

        // Truncate reasoning tokens to prevent excessive verbosity
        let truncated_delta = if self.aggregated.len() > MAX_REASONING_DISPLAY_CHARS {
            "" // Don't render more if we've exceeded the limit
        } else if self.aggregated.len() + delta.len() > MAX_REASONING_DISPLAY_CHARS {
            let remaining = MAX_REASONING_DISPLAY_CHARS - self.aggregated.len();
            &delta[..remaining.min(delta.len())]
        } else {
            delta
        };

        stream_plain_response_delta(
            renderer,
            MessageStyle::Reasoning,
            MessageStyle::Reasoning.indent(),
            &mut self.cli_pending_indent,
            truncated_delta,
        )
    }

    fn finalize_cli(&mut self, renderer: &mut AnsiRenderer) -> Result<()> {
        if self.cli_prefix_printed && !self.cli_pending_indent {
            renderer.inline_with_style(MessageStyle::Reasoning, "\n")?;
            self.cli_pending_indent = true;
        }
        Ok(())
    }

    /// Flush any pending reasoning display without full finalization
    /// Used to ensure reasoning appears before response content
    fn flush_pending(&mut self, renderer: &mut AnsiRenderer) -> Result<()> {
        if self.inline_enabled && !self.pending_inline.is_empty() {
            self.render_inline(renderer)?;
            self.pending_inline.clear();
        }
        Ok(())
    }

    /// Truncate reasoning to prevent excessive verbosity in streaming output
    fn truncate_for_display(&self) -> String {
        let trimmed = self.aggregated.trim_matches(['\r', '\n']);
        if trimmed.len() > MAX_REASONING_DISPLAY_CHARS {
            format!("{}...", &trimmed[..MAX_REASONING_DISPLAY_CHARS])
        } else {
            trimmed.to_string()
        }
    }

    fn display_lines(&self) -> Vec<String> {
        let trimmed = self.truncate_for_display();
        if trimmed.is_empty() {
            return Vec::new();
        }

        if trimmed.contains('\n') {
            let mut lines = Vec::new();
            lines.push(format!("{REASONING_HEADING}:"));
            for line in trimmed.lines() {
                lines.push(line.trim_end().to_string());
            }
            lines
        } else {
            vec![format!("{REASONING_PREFIX}{}", trimmed.trim())]
        }
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
                reasoning_state
                    .handle_delta(renderer, &delta)
                    .map_err(|err| map_render_error(provider_name, err))?;
            }
            Ok(LLMStreamEvent::Completed { response }) => {
                final_response = Some(response);
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

    let mut response = match final_response {
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

    // Prevent agent from giving up with "Complex. Probably stop." or similar
    if let Some(reasoning) = &response.reasoning
        && is_giving_up_reasoning(reasoning)
    {
        #[cfg(debug_assertions)]
        eprintln!(
            "Detected giving-up reasoning '{}', replacing with constructive reasoning",
            reasoning
        );

        // Log the original reasoning for debugging
        tracing::warn!(
            target = "vtcode::agent::reasoning",
            original_reasoning = %reasoning,
            "Agent attempted to give up, replacing with constructive reasoning"
        );

        // Replace with constructive reasoning that provides specific solutions
        response.reasoning = Some(get_constructive_reasoning(reasoning));
    }

    // Also ensure response content doesn't contain giving-up messages
    if let Some(content) = &response.content {
        let lower = content.to_lowercase();
        if lower.contains("complex") && lower.contains("stop") && !lower.contains("continue") {
            // Replace the content with a more constructive message
            response.content = Some("I'm working through the complexity systematically. Let me try a different approach.".to_string());
        }
    }

    // Handle providers that send only a Completed event with content (no token deltas).
    if aggregated.trim().is_empty() {
        if let Some(content) = response.content.as_deref() {
            if !content.trim().is_empty() {
                aggregated.push_str(content);
                emitted_tokens = true;
                if supports_streaming_markdown {
                    renderer
                        .stream_markdown_response(&aggregated, rendered_line_count)
                        .map_err(|err| map_render_error(provider_name, err))?;
                } else {
                    stream_plain_response_delta(
                        renderer,
                        response_style,
                        response_indent,
                        &mut needs_indent,
                        &aggregated,
                    )
                    .map_err(|err| map_render_error(provider_name, err))?;
                }
            }
        } else if let Some(reasoning) = response.reasoning.as_deref()
            && !reasoning.trim().is_empty()
        {
            aggregated.push_str(reasoning);
            emitted_tokens = true;
            // Prevent double-render in finalize
            response.reasoning = None;
            if supports_streaming_markdown {
                renderer
                    .stream_markdown_response(&aggregated, rendered_line_count)
                    .map_err(|err| map_render_error(provider_name, err))?;
            } else {
                stream_plain_response_delta(
                    renderer,
                    response_style,
                    response_indent,
                    &mut needs_indent,
                    &aggregated,
                )
                .map_err(|err| map_render_error(provider_name, err))?;
            }
        }
    }

    reasoning_state
        .finalize(renderer, response.reasoning.as_deref())
        .map_err(|err| map_render_error(provider_name, err))?;

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
            verbosity: None,
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

    #[test]
    fn test_detects_giving_up_reasoning() {
        assert!(is_giving_up_reasoning("Complex. Probably stop."));
        assert!(is_giving_up_reasoning(
            "This is too complex, I should stop."
        ));
        assert!(is_giving_up_reasoning(
            "I can't continue, it's too complex."
        ));
        assert!(is_giving_up_reasoning("Probably should stop here."));
        assert!(is_giving_up_reasoning(
            "Unable to continue with this complex task."
        ));
    }

    #[test]
    fn test_does_not_detect_normal_reasoning() {
        assert!(!is_giving_up_reasoning("I'm analyzing the file structure."));
        assert!(!is_giving_up_reasoning(
            "Let me check the directory contents."
        ));
        assert!(!is_giving_up_reasoning(
            "Processing the request systematically."
        ));
        assert!(!is_giving_up_reasoning("Continue with the next step."));
    }

    #[test]
    fn test_constructive_reasoning_generation() {
        let original = "Complex. Probably stop.";
        let constructive = get_constructive_reasoning(original);
        assert!(!constructive.is_empty());
        assert!(!constructive.to_lowercase().contains("stop"));
        assert!(
            constructive.to_lowercase().contains("solution")
                || constructive.to_lowercase().contains("try")
        );
    }

    #[test]
    fn test_constructive_reasoning_task_specific() {
        let pdf_reasoning = "Complex PDF generation, probably stop.";
        let pdf_constructive = get_constructive_reasoning(pdf_reasoning);
        assert!(
            pdf_constructive.to_lowercase().contains("file")
                || pdf_constructive.to_lowercase().contains("path")
        );

        let tool_reasoning = "Tool execution too complex, can't continue.";
        let tool_constructive = get_constructive_reasoning(tool_reasoning);
        assert!(
            tool_constructive.to_lowercase().contains("tool")
                || tool_constructive.to_lowercase().contains("execut")
        );
    }
}
