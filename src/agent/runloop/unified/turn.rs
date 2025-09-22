use anyhow::{Context, Result};
use futures::StreamExt;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::Notify;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::task;
use tokio::time::sleep;

use vtcode_core::config::constants::defaults;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::decision_tracker::{Action as DTAction, DecisionOutcome};
use vtcode_core::core::router::{Router, TaskClass};
use vtcode_core::llm::error_display;
use vtcode_core::llm::provider::{self as uni, LLMStreamEvent, MessageRole};
use vtcode_core::tools::registry::{ToolErrorType, ToolExecutionError, ToolPermissionDecision};
use vtcode_core::ui::ratatui::{
    RatatuiEvent, RatatuiHandle, convert_style as convert_ratatui_style, spawn_session,
    theme_from_styles,
};
use vtcode_core::ui::theme;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use crate::agent::runloop::context::{
    apply_aggressive_trim_unified, enforce_unified_context_window, prune_unified_tool_responses,
};
use crate::agent::runloop::git::confirm_changes_with_git_diff;
use crate::agent::runloop::is_context_overflow_error;
use crate::agent::runloop::prompt::refine_user_prompt_if_enabled;
use crate::agent::runloop::slash_commands::{SlashCommandOutcome, handle_slash_command};
use crate::agent::runloop::text_tools::detect_textual_tool_call;
use crate::agent::runloop::tool_output::render_tool_output;
use crate::agent::runloop::ui::render_session_banner;

use super::display::{display_user_message, ensure_turn_bottom_gap, persist_theme_preference};
use super::session_setup::{SessionState, initialize_session};
use super::shell::{derive_recent_tool_output, should_short_circuit_shell};

#[derive(Default)]
struct SessionStats {
    tools: BTreeSet<String>,
}

impl SessionStats {
    fn record_tool(&mut self, name: &str) {
        self.tools.insert(name.to_string());
    }

    fn render_summary(&self, renderer: &mut AnsiRenderer, history: &[uni::Message]) -> Result<()> {
        let total_chars: usize = history.iter().map(|msg| msg.content.chars().count()).sum();
        let approx_tokens = (total_chars + 3) / 4;
        let user_turns = history
            .iter()
            .filter(|msg| matches!(msg.role, MessageRole::User))
            .count();
        let assistant_turns = history
            .iter()
            .filter(|msg| matches!(msg.role, MessageRole::Assistant))
            .count();

        renderer.line_if_not_empty(MessageStyle::Info)?;
        renderer.line(MessageStyle::Info, "Session summary")?;
        renderer.line(
            MessageStyle::Output,
            &format!(
                "   * User turns: {} · Agent turns: {} · ~{} tokens",
                user_turns, assistant_turns, approx_tokens
            ),
        )?;
        if self.tools.is_empty() {
            renderer.line(MessageStyle::Output, "   * Tools used: none")?;
        } else {
            let joined = self.tools.iter().cloned().collect::<Vec<_>>().join(", ");
            renderer.line(
                MessageStyle::Output,
                &format!("   * Tools used: {}", joined),
            )?;
        }
        renderer.line(MessageStyle::Info, "Goodbye!")?;

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HitlDecision {
    Approved,
    Denied,
    Exit,
    Interrupt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolPermissionFlow {
    Approved,
    Denied,
    Exit,
    Interrupted,
}

struct PlaceholderGuard {
    handle: RatatuiHandle,
    restore: Option<String>,
}

impl PlaceholderGuard {
    fn new(handle: &RatatuiHandle, restore: Option<String>) -> Self {
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

async fn prompt_tool_permission(
    tool_name: &str,
    renderer: &mut AnsiRenderer,
    handle: &RatatuiHandle,
    events: &mut UnboundedReceiver<RatatuiEvent>,
    ctrl_c_flag: &Arc<AtomicBool>,
    ctrl_c_notify: &Arc<Notify>,
    default_placeholder: Option<String>,
) -> Result<HitlDecision> {
    renderer.line_if_not_empty(MessageStyle::Info)?;
    renderer.line(
        MessageStyle::Info,
        &format!("Tool '{}' requires approval.", tool_name),
    )?;
    renderer.line(
        MessageStyle::Info,
        "Type 'yes' to allow, 'no' to deny, or press Esc to cancel.",
    )?;
    renderer.line(MessageStyle::Info, "")?;

    let _placeholder_guard = PlaceholderGuard::new(handle, default_placeholder);
    let prompt_placeholder = Some(format!("Approve '{}' tool? yes/no", tool_name));
    handle.set_placeholder(prompt_placeholder);

    // Yield once so the UI processes the prompt lines and placeholder update
    // before we start listening for user input. Without this the question would
    // only appear after a subsequent event (like cancel) fired.
    task::yield_now().await;

    loop {
        if ctrl_c_flag.load(Ordering::SeqCst) {
            return Ok(HitlDecision::Interrupt);
        }

        let notify = ctrl_c_notify.clone();
        let maybe_event = tokio::select! {
            _ = notify.notified(), if !ctrl_c_flag.load(Ordering::SeqCst) => None,
            event = events.recv() => event,
        };

        let Some(event) = maybe_event else {
            if ctrl_c_flag.load(Ordering::SeqCst) {
                return Ok(HitlDecision::Interrupt);
            }
            return Ok(HitlDecision::Exit);
        };

        match event {
            RatatuiEvent::Submit(input) => {
                let normalized = input.trim().to_lowercase();
                if normalized.is_empty() {
                    renderer.line(MessageStyle::Info, "Please respond with 'yes' or 'no'.")?;
                    continue;
                }

                if matches!(normalized.as_str(), "y" | "yes" | "approve" | "allow") {
                    return Ok(HitlDecision::Approved);
                }

                if matches!(normalized.as_str(), "n" | "no" | "deny" | "cancel" | "stop") {
                    return Ok(HitlDecision::Denied);
                }

                renderer.line(
                    MessageStyle::Info,
                    "Respond with 'yes' to approve or 'no' to deny.",
                )?;
            }
            RatatuiEvent::Cancel => {
                return Ok(HitlDecision::Denied);
            }
            RatatuiEvent::Exit => {
                return Ok(HitlDecision::Exit);
            }
            RatatuiEvent::Interrupt => {
                return Ok(HitlDecision::Interrupt);
            }
            RatatuiEvent::ScrollLineUp
            | RatatuiEvent::ScrollLineDown
            | RatatuiEvent::ScrollPageUp
            | RatatuiEvent::ScrollPageDown => {}
        }
    }
}

async fn ensure_tool_permission(
    tool_registry: &mut vtcode_core::tools::registry::ToolRegistry,
    tool_name: &str,
    renderer: &mut AnsiRenderer,
    handle: &RatatuiHandle,
    events: &mut UnboundedReceiver<RatatuiEvent>,
    default_placeholder: Option<String>,
    ctrl_c_flag: &Arc<AtomicBool>,
    ctrl_c_notify: &Arc<Notify>,
) -> Result<ToolPermissionFlow> {
    match tool_registry.evaluate_tool_policy(tool_name)? {
        ToolPermissionDecision::Allow => Ok(ToolPermissionFlow::Approved),
        ToolPermissionDecision::Deny => Ok(ToolPermissionFlow::Denied),
        ToolPermissionDecision::Prompt => {
            let decision = prompt_tool_permission(
                tool_name,
                renderer,
                handle,
                events,
                ctrl_c_flag,
                ctrl_c_notify,
                default_placeholder,
            )
            .await?;
            match decision {
                HitlDecision::Approved => {
                    tool_registry.mark_tool_preapproved(tool_name);
                    Ok(ToolPermissionFlow::Approved)
                }
                HitlDecision::Denied => Ok(ToolPermissionFlow::Denied),
                HitlDecision::Exit => Ok(ToolPermissionFlow::Exit),
                HitlDecision::Interrupt => Ok(ToolPermissionFlow::Interrupted),
            }
        }
    }
}

fn apply_prompt_style(handle: &RatatuiHandle) {
    let styles = theme::active_styles();
    let style = convert_ratatui_style(styles.primary);
    handle.set_prompt("❯ ".to_string(), style);
}

const PLACEHOLDER_SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

struct PlaceholderSpinner {
    handle: RatatuiHandle,
    restore_hint: Option<String>,
    active: Arc<AtomicBool>,
    task: task::JoinHandle<()>,
}

impl PlaceholderSpinner {
    fn new(
        handle: &RatatuiHandle,
        restore_hint: Option<String>,
        message: impl Into<String>,
    ) -> Self {
        let message = message.into();
        let active = Arc::new(AtomicBool::new(true));
        let spinner_active = active.clone();
        let spinner_handle = handle.clone();
        let restore_on_stop = restore_hint.clone();

        let task = task::spawn(async move {
            let mut index = 0usize;
            let frame_count = PLACEHOLDER_SPINNER_FRAMES.len().max(1);
            while spinner_active.load(Ordering::SeqCst) {
                let frame = PLACEHOLDER_SPINNER_FRAMES[index % frame_count];
                spinner_handle.set_placeholder(Some(format!("{frame} {message}")));
                index = (index + 1) % frame_count;
                sleep(Duration::from_millis(120)).await;
            }
            spinner_handle.set_placeholder(restore_on_stop);
        });

        Self {
            handle: handle.clone(),
            restore_hint,
            active,
            task,
        }
    }

    fn finish(&self) {
        if self.active.swap(false, Ordering::SeqCst) {
            self.handle.set_placeholder(self.restore_hint.clone());
        }
    }
}

impl Drop for PlaceholderSpinner {
    fn drop(&mut self) {
        self.finish();
        self.task.abort();
    }
}

fn map_render_error(provider_name: &str, err: anyhow::Error) -> uni::LLMError {
    let formatted_error = error_display::format_llm_error(
        provider_name,
        &format!("Failed to render streaming output: {}", err),
    );
    uni::LLMError::Provider(formatted_error)
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

        if chunk.ends_with('\n') {
            let text = &chunk[..chunk.len() - 1];
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

async fn stream_and_render_response(
    provider: &dyn uni::LLMProvider,
    request: uni::LLMRequest,
    spinner: &PlaceholderSpinner,
    renderer: &mut AnsiRenderer,
) -> Result<(uni::LLMResponse, bool), uni::LLMError> {
    let mut stream = provider.stream(request).await?;
    let provider_name = provider.name();
    let mut final_response: Option<uni::LLMResponse> = None;
    let mut aggregated = String::new();
    let mut spinner_active = true;
    let supports_streaming_markdown = renderer.supports_streaming_markdown();
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

    while let Some(event_result) = stream.next().await {
        match event_result {
            Ok(LLMStreamEvent::Token { delta }) => {
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
            Ok(LLMStreamEvent::Reasoning { .. }) => {}
            Ok(LLMStreamEvent::Completed { response }) => {
                final_response = Some(response);
            }
            Err(err) => {
                finish_spinner(&mut spinner_active);
                return Err(err);
            }
        }
    }

    finish_spinner(&mut spinner_active);

    let response = final_response.ok_or_else(|| {
        let formatted_error = error_display::format_llm_error(
            provider_name,
            "Stream ended without a completion event",
        );
        uni::LLMError::Provider(formatted_error)
    })?;

    if aggregated.is_empty() {
        if let Some(content) = response.content.clone() {
            if !content.is_empty() {
                aggregated.push_str(&content);
            }
        }
    }

    if !aggregated.is_empty() {
        if !emitted_tokens {
            if supports_streaming_markdown {
                let _ = renderer
                    .stream_markdown_response(&aggregated, rendered_line_count)
                    .map_err(|err| map_render_error(provider_name, err))?;
            } else {
                renderer
                    .line(MessageStyle::Response, &aggregated)
                    .map_err(|err| map_render_error(provider_name, err))?;
            }
            emitted_tokens = true;
        } else if !supports_streaming_markdown && !aggregated.ends_with('\n') {
            renderer
                .line_if_not_empty(MessageStyle::Response)
                .map_err(|err| map_render_error(provider_name, err))?;
        }
    }

    Ok((response, emitted_tokens))
}

enum TurnLoopResult {
    Completed,
    Aborted,
    Cancelled,
}

pub(crate) async fn run_single_agent_loop_unified(
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    skip_confirmations: bool,
    full_auto: bool,
) -> Result<()> {
    let SessionState {
        session_bootstrap,
        provider_client,
        mut tool_registry,
        tools,
        trim_config,
        mut conversation_history,
        mut ledger,
        trajectory: traj,
        base_system_prompt,
        full_auto_allowlist,
    } = initialize_session(config, vt_cfg, full_auto).await?;

    let active_styles = theme::active_styles();
    let theme_spec = theme_from_styles(&active_styles);
    let default_placeholder = session_bootstrap.placeholder.clone();
    let session = spawn_session(theme_spec.clone(), default_placeholder.clone())
        .context("failed to launch ratatui session")?;
    let handle = session.handle.clone();
    let highlight_config = vt_cfg
        .map(|cfg| cfg.syntax_highlighting.clone())
        .unwrap_or_default();
    let mut renderer = AnsiRenderer::with_ratatui(handle.clone(), highlight_config);

    handle.set_theme(theme_spec);
    apply_prompt_style(&handle);
    handle.set_placeholder(default_placeholder.clone());

    let trust_mode_flag = vt_cfg
        .map(|cfg| cfg.security.human_in_the_loop)
        .or(session_bootstrap.human_in_the_loop)
        .unwrap_or(true);
    let trust_mode_label = if trust_mode_flag {
        "HITL"
    } else {
        "Autonomous"
    };
    let reasoning_label = vt_cfg
        .map(|cfg| cfg.agent.reasoning_effort.as_str().to_string())
        .unwrap_or_else(|| config.reasoning_effort.as_str().to_string());
    let center_status = format!(
        "Model: {} · Trust: {} · Reasoning: {}",
        config.model, trust_mode_label, reasoning_label
    );
    handle.update_status_bar(None, Some(center_status), None);

    render_session_banner(&mut renderer, config, &session_bootstrap)?;
    if let Some(text) = session_bootstrap.welcome_text.as_ref() {
        renderer.line(MessageStyle::Response, text)?;
        renderer.line_if_not_empty(MessageStyle::Output)?;
    }

    if full_auto {
        if let Some(allowlist) = full_auto_allowlist.as_ref() {
            if allowlist.is_empty() {
                renderer.line(
                    MessageStyle::Info,
                    "Full-auto mode enabled with no tool permissions; tool calls will be skipped.",
                )?;
            } else {
                renderer.line(
                    MessageStyle::Info,
                    &format!(
                        "Full-auto mode enabled. Permitted tools: {}",
                        allowlist.join(", ")
                    ),
                )?;
            }
        }
    }

    let ctrl_c_flag = Arc::new(AtomicBool::new(false));
    let ctrl_c_notify = Arc::new(Notify::new());
    {
        let flag = ctrl_c_flag.clone();
        let notify = ctrl_c_notify.clone();
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                flag.store(true, Ordering::SeqCst);
                notify.notify_waiters();
            }
        });
    }

    let mut session_stats = SessionStats::default();
    let mut events = session.events;
    loop {
        if ctrl_c_flag.load(Ordering::SeqCst) {
            session_stats.render_summary(&mut renderer, &conversation_history)?;
            break;
        }

        let maybe_event = tokio::select! {
            biased;

            _ = ctrl_c_notify.notified() => None,
            event = events.recv() => event,
        };

        let Some(event) = maybe_event else {
            if ctrl_c_flag.load(Ordering::SeqCst) {
                session_stats.render_summary(&mut renderer, &conversation_history)?;
            }
            break;
        };

        let submitted = match event {
            RatatuiEvent::Submit(text) => text,
            RatatuiEvent::Cancel => {
                renderer.line(
                    MessageStyle::Info,
                    "Cancellation request noted. No active run to stop.",
                )?;
                continue;
            }
            RatatuiEvent::Exit => {
                renderer.line(MessageStyle::Info, "Goodbye!")?;
                break;
            }
            RatatuiEvent::Interrupt => {
                session_stats.render_summary(&mut renderer, &conversation_history)?;
                break;
            }
            RatatuiEvent::ScrollLineUp
            | RatatuiEvent::ScrollLineDown
            | RatatuiEvent::ScrollPageUp
            | RatatuiEvent::ScrollPageDown => continue,
        };

        let input_owned = submitted.trim().to_string();

        if input_owned.is_empty() {
            continue;
        }

        match input_owned.as_str() {
            "" => continue,
            "exit" | "quit" => {
                renderer.line(MessageStyle::Info, "Goodbye!")?;
                break;
            }
            "help" => {
                renderer.line(MessageStyle::Info, "Commands: exit, help")?;
                continue;
            }
            _ => {}
        }

        if let Some(command_input) = input_owned.strip_prefix('/') {
            match handle_slash_command(command_input, &mut renderer)? {
                SlashCommandOutcome::Handled => {
                    continue;
                }
                SlashCommandOutcome::ThemeChanged(theme_id) => {
                    persist_theme_preference(&mut renderer, &theme_id)?;
                    let styles = theme::active_styles();
                    handle.set_theme(theme_from_styles(&styles));
                    apply_prompt_style(&handle);
                    continue;
                }
                SlashCommandOutcome::ExecuteTool { name, args } => {
                    match ensure_tool_permission(
                        &mut tool_registry,
                        &name,
                        &mut renderer,
                        &handle,
                        &mut events,
                        default_placeholder.clone(),
                        &ctrl_c_flag,
                        &ctrl_c_notify,
                    )
                    .await
                    {
                        Ok(ToolPermissionFlow::Approved) => {
                            let tool_spinner = PlaceholderSpinner::new(
                                &handle,
                                default_placeholder.clone(),
                                format!("Running tool: {}", name),
                            );
                            match tool_registry.execute_tool(&name, args.clone()).await {
                                Ok(tool_output) => {
                                    tool_spinner.finish();
                                    session_stats.record_tool(&name);
                                    traj.log_tool_call(
                                        conversation_history.len(),
                                        &name,
                                        &args,
                                        true,
                                    );
                                    render_tool_output(
                                        &mut renderer,
                                        Some(name.as_str()),
                                        &tool_output,
                                    )?;
                                }
                                Err(err) => {
                                    tool_spinner.finish();
                                    traj.log_tool_call(
                                        conversation_history.len(),
                                        &name,
                                        &args,
                                        false,
                                    );
                                    renderer.line(
                                        MessageStyle::Error,
                                        &format!("Tool '{}' failed: {}", name, err),
                                    )?;
                                }
                            }
                        }
                        Ok(ToolPermissionFlow::Denied) => {
                            session_stats.record_tool(&name);
                            let denial = ToolExecutionError::new(
                                name.clone(),
                                ToolErrorType::PolicyViolation,
                                format!("Tool '{}' execution denied by policy", name),
                            )
                            .to_json_value();
                            traj.log_tool_call(conversation_history.len(), &name, &args, false);
                            render_tool_output(&mut renderer, Some(name.as_str()), &denial)?;
                            continue;
                        }
                        Ok(ToolPermissionFlow::Exit) => {
                            renderer.line(MessageStyle::Info, "Goodbye!")?;
                            break;
                        }
                        Ok(ToolPermissionFlow::Interrupted) => {
                            session_stats.render_summary(&mut renderer, &conversation_history)?;
                            break;
                        }
                        Err(err) => {
                            traj.log_tool_call(conversation_history.len(), &name, &args, false);
                            renderer.line(
                                MessageStyle::Error,
                                &format!("Failed to evaluate policy for tool '{}': {}", name, err),
                            )?;
                        }
                    }
                    continue;
                }
                SlashCommandOutcome::Exit => {
                    renderer.line(MessageStyle::Info, "Goodbye!")?;
                    break;
                }
            }
        }

        let input = input_owned.as_str();

        let refined_user = refine_user_prompt_if_enabled(input, config, vt_cfg).await;
        // Display the user message with ratatui border decoration
        display_user_message(&mut renderer, &refined_user)?;
        conversation_history.push(uni::Message::user(refined_user));
        let _pruned_tools = prune_unified_tool_responses(
            &mut conversation_history,
            trim_config.preserve_recent_turns,
        );
        // Removed: Tool response pruning message
        let trim_result = enforce_unified_context_window(&mut conversation_history, trim_config);
        if trim_result.is_trimmed() {
            renderer.line(
                MessageStyle::Info,
                &format!(
                    "Trimmed {} earlier messages to respect the context window (~{} tokens).",
                    trim_result.removed_messages, trim_config.max_tokens,
                ),
            )?;
        }

        let mut working_history = conversation_history.clone();
        let max_tool_loops = vt_cfg
            .map(|cfg| cfg.tools.max_tool_loops)
            .filter(|&value| value > 0)
            .unwrap_or(defaults::DEFAULT_MAX_TOOL_LOOPS);

        let mut loop_guard = 0usize;
        let mut any_write_effect = false;
        let mut last_tool_stdout: Option<String> = None;
        let mut bottom_gap_applied = false;

        let turn_result = 'outer: loop {
            if ctrl_c_flag.load(Ordering::SeqCst) {
                break TurnLoopResult::Cancelled;
            }
            if loop_guard == 0 {
                renderer.line_if_not_empty(MessageStyle::Output)?;
            }
            loop_guard += 1;
            if loop_guard >= max_tool_loops {
                if !bottom_gap_applied {
                    renderer.line(MessageStyle::Output, "")?;
                }
                let notice = format!(
                    "I reached the configured tool-call limit of {} for this turn and paused further tool execution. Increase `tools.max_tool_loops` in vtcode.toml if you need more, then ask me to continue.",
                    max_tool_loops
                );
                renderer.line(MessageStyle::Error, &notice)?;
                ensure_turn_bottom_gap(&mut renderer, &mut bottom_gap_applied)?;
                working_history.push(uni::Message::assistant(notice));
                break TurnLoopResult::Completed;
            }

            let _ = enforce_unified_context_window(&mut working_history, trim_config);

            let decision = if let Some(cfg) = vt_cfg.filter(|cfg| cfg.router.enabled) {
                Router::route_async(cfg, config, &config.api_key, input).await
            } else {
                Router::route(&VTCodeConfig::default(), config, input)
            };
            traj.log_route(
                working_history.len(),
                &decision.selected_model,
                match decision.class {
                    TaskClass::Simple => "simple",
                    TaskClass::Standard => "standard",
                    TaskClass::Complex => "complex",
                    TaskClass::CodegenHeavy => "codegen_heavy",
                    TaskClass::RetrievalHeavy => "retrieval_heavy",
                },
                &input.chars().take(120).collect::<String>(),
            );

            let active_model = decision.selected_model;
            let (max_tokens_opt, parallel_cfg_opt) = if let Some(vt) = vt_cfg {
                let key = match decision.class {
                    TaskClass::Simple => "simple",
                    TaskClass::Standard => "standard",
                    TaskClass::Complex => "complex",
                    TaskClass::CodegenHeavy => "codegen_heavy",
                    TaskClass::RetrievalHeavy => "retrieval_heavy",
                };
                let budget = vt.router.budgets.get(key);
                let max_tokens = budget.and_then(|b| b.max_tokens).map(|value| value as u32);
                let parallel = budget.and_then(|b| b.max_parallel_tools).map(|value| {
                    vtcode_core::llm::provider::ParallelToolConfig {
                        disable_parallel_tool_use: value <= 1,
                        max_parallel_tools: Some(value),
                        encourage_parallel: value > 1,
                    }
                });
                (max_tokens, parallel)
            } else {
                (None, None)
            };

            let (lg_enabled, lg_max, lg_include) = vt_cfg
                .map(|cfg| {
                    (
                        cfg.context.ledger.enabled,
                        cfg.context.ledger.max_entries,
                        cfg.context.ledger.include_in_prompt,
                    )
                })
                .unwrap_or((true, 12, true));

            ledger.start_turn(
                working_history.len(),
                working_history
                    .last()
                    .map(|message| message.content.clone()),
            );
            let tool_names: Vec<String> = tools
                .iter()
                .map(|tool| tool.function.name.clone())
                .collect();
            ledger.update_available_tools(tool_names);

            let system_prompt = if lg_enabled && lg_include {
                format!(
                    "{}\n\n[Decision Ledger]\n{}",
                    base_system_prompt,
                    ledger.render_ledger_brief(lg_max)
                )
            } else {
                base_system_prompt.clone()
            };

            let mut attempt_history = working_history.clone();
            let mut retry_attempts = 0usize;
            let (response, response_streamed) = loop {
                retry_attempts += 1;
                let _ = enforce_unified_context_window(&mut attempt_history, trim_config);

                let use_streaming = provider_client.supports_streaming();
                let reasoning_effort = vt_cfg.and_then(|cfg| {
                    if provider_client.supports_reasoning_effort(&active_model) {
                        Some(cfg.agent.reasoning_effort.as_str().to_string())
                    } else {
                        None
                    }
                });
                let request = uni::LLMRequest {
                    messages: attempt_history.clone(),
                    system_prompt: Some(system_prompt.clone()),
                    tools: Some(tools.clone()),
                    model: active_model.clone(),
                    max_tokens: max_tokens_opt.or(Some(2000)),
                    temperature: Some(0.7),
                    stream: use_streaming,
                    tool_choice: Some(uni::ToolChoice::auto()),
                    parallel_tool_calls: None,
                    parallel_tool_config: parallel_cfg_opt.clone(),
                    reasoning_effort,
                };

                let thinking_spinner =
                    PlaceholderSpinner::new(&handle, default_placeholder.clone(), "Thinking...");
                let mut spinner_active = true;
                task::yield_now().await;
                let result = if use_streaming {
                    let outcome = stream_and_render_response(
                        provider_client.as_ref(),
                        request,
                        &thinking_spinner,
                        &mut renderer,
                    )
                    .await;
                    spinner_active = false;
                    outcome
                } else {
                    provider_client
                        .generate(request)
                        .await
                        .map(|resp| (resp, false))
                };

                if spinner_active {
                    thinking_spinner.finish();
                }

                match result {
                    Ok((result, streamed_tokens)) => {
                        working_history = attempt_history.clone();
                        break (result, streamed_tokens);
                    }
                    Err(error) => {
                        if ctrl_c_flag.load(Ordering::SeqCst) {
                            break 'outer TurnLoopResult::Cancelled;
                        }
                        let error_text = error.to_string();
                        if is_context_overflow_error(&error_text)
                            && retry_attempts <= vtcode_core::config::constants::context::CONTEXT_ERROR_RETRY_LIMIT
                        {
                            let removed_tool_messages = prune_unified_tool_responses(
                                &mut attempt_history,
                                trim_config.preserve_recent_turns,
                            );
                            let removed_turns =
                                apply_aggressive_trim_unified(&mut attempt_history, trim_config);
                            let total_removed = removed_tool_messages + removed_turns;
                            if total_removed > 0 {
                                renderer.line(
                                    MessageStyle::Info,
                                    &format!(
                                        "Context overflow detected; removed {} older messages (retry {}/{}).",
                                        total_removed,
                                        retry_attempts,
                                        vtcode_core::config::constants::context::CONTEXT_ERROR_RETRY_LIMIT,
                                    ),
                                )?;
                                conversation_history.clone_from(&attempt_history);
                                continue;
                            }
                        }

                        let has_tool = working_history
                            .iter()
                            .any(|msg| msg.role == uni::MessageRole::Tool);

                        if has_tool {
                            eprintln!("Provider error (suppressed): {error_text}");
                            let reply = derive_recent_tool_output(&working_history)
                                .unwrap_or_else(|| "Command completed successfully.".to_string());
                            renderer.line(MessageStyle::Response, &reply)?;
                            ensure_turn_bottom_gap(&mut renderer, &mut bottom_gap_applied)?;
                            working_history.push(uni::Message::assistant(reply));
                            let _ = last_tool_stdout.take();
                            break 'outer TurnLoopResult::Completed;
                        } else {
                            renderer.line(
                                MessageStyle::Error,
                                &format!("Provider error: {error_text}"),
                            )?;
                            ensure_turn_bottom_gap(&mut renderer, &mut bottom_gap_applied)?;
                            break 'outer TurnLoopResult::Aborted;
                        }
                    }
                }
            };

            let mut final_text = response.content.clone();
            let mut tool_calls = response.tool_calls.clone().unwrap_or_default();
            let mut interpreted_textual_call = false;

            if tool_calls.is_empty()
                && let Some(text) = final_text.clone()
                && let Some((name, args)) = detect_textual_tool_call(&text)
            {
                let args_display =
                    serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string());
                renderer.line(
                    MessageStyle::Info,
                    &format!(
                        "Interpreting textual tool request as {} {}",
                        &name, &args_display
                    ),
                )?;
                let call_id = format!("call_textual_{}", working_history.len());
                tool_calls.push(uni::ToolCall::function(
                    call_id.clone(),
                    name.clone(),
                    args_display.clone(),
                ));
                interpreted_textual_call = true;
                final_text = None;
            }

            if tool_calls.is_empty()
                && let Some(text) = final_text.clone()
            {
                working_history.push(uni::Message::assistant(text));
            } else {
                let assistant_text = if interpreted_textual_call {
                    String::new()
                } else {
                    final_text.clone().unwrap_or_default()
                };
                working_history.push(uni::Message::assistant_with_tools(
                    assistant_text,
                    tool_calls.clone(),
                ));
                for call in &tool_calls {
                    let name = call.function.name.as_str();
                    let args_val = call
                        .parsed_arguments()
                        .unwrap_or_else(|_| serde_json::json!({}));
                    let args_display =
                        serde_json::to_string(&args_val).unwrap_or_else(|_| "{}".to_string());
                    renderer.line(
                        MessageStyle::Tool,
                        &format!("[TOOL] {} {}", name, args_display),
                    )?;
                    let dec_id = ledger.record_decision(
                        format!("Execute tool '{}' to progress task", name),
                        DTAction::ToolCall {
                            name: name.to_string(),
                            args: args_val.clone(),
                            expected_outcome: "Use tool output to decide next step".to_string(),
                        },
                        None,
                    );

                    match ensure_tool_permission(
                        &mut tool_registry,
                        name,
                        &mut renderer,
                        &handle,
                        &mut events,
                        default_placeholder.clone(),
                        &ctrl_c_flag,
                        &ctrl_c_notify,
                    )
                    .await
                    {
                        Ok(ToolPermissionFlow::Approved) => {
                            let tool_spinner = PlaceholderSpinner::new(
                                &handle,
                                default_placeholder.clone(),
                                format!("Running tool: {}", name),
                            );
                            match tool_registry.execute_tool(name, args_val.clone()).await {
                                Ok(tool_output) => {
                                    tool_spinner.finish();
                                    session_stats.record_tool(name);
                                    traj.log_tool_call(
                                        working_history.len(),
                                        name,
                                        &args_val,
                                        true,
                                    );
                                    render_tool_output(&mut renderer, Some(name), &tool_output)?;
                                    last_tool_stdout = tool_output
                                        .get("stdout")
                                        .and_then(|value| value.as_str())
                                        .map(|s| s.trim().to_string())
                                        .filter(|s| !s.is_empty());
                                    let modified_files: Vec<String> = if let Some(files) =
                                        tool_output
                                            .get("modified_files")
                                            .and_then(|value| value.as_array())
                                    {
                                        files
                                            .iter()
                                            .filter_map(|file| {
                                                file.as_str().map(|value| value.to_string())
                                            })
                                            .collect()
                                    } else {
                                        vec![]
                                    };

                                    if matches!(
                                        name,
                                        "write_file"
                                            | "edit_file"
                                            | "create_file"
                                            | "delete_file"
                                            | "srgn"
                                    ) {
                                        any_write_effect = true;
                                    }

                                    if !modified_files.is_empty()
                                        && confirm_changes_with_git_diff(
                                            &modified_files,
                                            skip_confirmations,
                                        )
                                        .await?
                                    {
                                        renderer.line(
                                            MessageStyle::Info,
                                            "Changes applied successfully.",
                                        )?;
                                    } else if !modified_files.is_empty() {
                                        renderer.line(MessageStyle::Info, "Changes discarded.")?;
                                    }

                                    let content = serde_json::to_string(&tool_output)
                                        .unwrap_or("{}".to_string());
                                    working_history.push(uni::Message::tool_response(
                                        call.id.clone(),
                                        content,
                                    ));
                                    ledger.record_outcome(
                                        &dec_id,
                                        DecisionOutcome::Success {
                                            result: "tool_ok".to_string(),
                                            metrics: Default::default(),
                                        },
                                    );

                                    if should_short_circuit_shell(input, name, &args_val) {
                                        let reply = last_tool_stdout.clone().unwrap_or_else(|| {
                                            "Command completed successfully.".to_string()
                                        });
                                        renderer.line(MessageStyle::Response, &reply)?;
                                        ensure_turn_bottom_gap(
                                            &mut renderer,
                                            &mut bottom_gap_applied,
                                        )?;
                                        working_history.push(uni::Message::assistant(reply));
                                        let _ = last_tool_stdout.take();
                                        break 'outer TurnLoopResult::Completed;
                                    }
                                }
                                Err(error) => {
                                    tool_spinner.finish();
                                    session_stats.record_tool(name);
                                    renderer.line(
                                        MessageStyle::Tool,
                                        &format!("Tool {} failed.", name),
                                    )?;
                                    traj.log_tool_call(
                                        working_history.len(),
                                        name,
                                        &args_val,
                                        false,
                                    );
                                    renderer.line(
                                        MessageStyle::Error,
                                        &format!("Tool error: {error}"),
                                    )?;
                                    let err = serde_json::json!({ "error": error.to_string() });
                                    let content = err.to_string();
                                    working_history.push(uni::Message::tool_response(
                                        call.id.clone(),
                                        content,
                                    ));
                                    let _ = last_tool_stdout.take();
                                    ledger.record_outcome(
                                        &dec_id,
                                        DecisionOutcome::Failure {
                                            error: error.to_string(),
                                            recovery_attempts: 0,
                                            context_preserved: true,
                                        },
                                    );
                                }
                            }
                        }
                        Ok(ToolPermissionFlow::Denied) => {
                            session_stats.record_tool(name);
                            let denial = ToolExecutionError::new(
                                name.to_string(),
                                ToolErrorType::PolicyViolation,
                                format!("Tool '{}' execution denied by policy", name),
                            )
                            .to_json_value();
                            traj.log_tool_call(working_history.len(), name, &args_val, false);
                            render_tool_output(&mut renderer, Some(name), &denial)?;
                            let content =
                                serde_json::to_string(&denial).unwrap_or("{}".to_string());
                            working_history
                                .push(uni::Message::tool_response(call.id.clone(), content));
                            ledger.record_outcome(
                                &dec_id,
                                DecisionOutcome::Failure {
                                    error: format!("Tool '{}' execution denied by policy", name),
                                    recovery_attempts: 0,
                                    context_preserved: true,
                                },
                            );
                            continue;
                        }
                        Ok(ToolPermissionFlow::Exit) => {
                            renderer.line(MessageStyle::Info, "Goodbye!")?;
                            break 'outer TurnLoopResult::Cancelled;
                        }
                        Ok(ToolPermissionFlow::Interrupted) => {
                            break 'outer TurnLoopResult::Cancelled;
                        }
                        Err(err) => {
                            traj.log_tool_call(working_history.len(), name, &args_val, false);
                            renderer.line(
                                MessageStyle::Error,
                                &format!("Failed to evaluate policy for tool '{}': {}", name, err),
                            )?;
                            let err_json = serde_json::json!({
                                "error": format!(
                                    "Policy evaluation error for '{}' : {}",
                                    name, err
                                )
                            });
                            working_history.push(uni::Message::tool_response(
                                call.id.clone(),
                                err_json.to_string(),
                            ));
                            let _ = last_tool_stdout.take();
                            ledger.record_outcome(
                                &dec_id,
                                DecisionOutcome::Failure {
                                    error: format!(
                                        "Failed to evaluate policy for tool '{}': {}",
                                        name, err
                                    ),
                                    recovery_attempts: 0,
                                    context_preserved: true,
                                },
                            );
                            continue;
                        }
                    }
                }
                continue;
            }

            if let Some(mut text) = final_text.clone() {
                let do_review = vt_cfg
                    .map(|cfg| cfg.agent.enable_self_review)
                    .unwrap_or(false);
                let review_passes = vt_cfg
                    .map(|cfg| cfg.agent.max_review_passes)
                    .unwrap_or(1)
                    .max(1);
                if do_review {
                    let review_system = "You are the agent's critical code reviewer. Improve clarity, correctness, and add missing test or validation guidance. Return only the improved final answer (no meta commentary).".to_string();
                    for _ in 0..review_passes {
                        let review_req = uni::LLMRequest {
                            messages: vec![uni::Message::user(format!(
                                "Please review and refine the following response. Return only the improved response.\n\n{}",
                                text
                            ))],
                            system_prompt: Some(review_system.clone()),
                            tools: None,
                            model: config.model.clone(),
                            max_tokens: Some(2000),
                            temperature: Some(0.5),
                            stream: false,
                            tool_choice: Some(uni::ToolChoice::none()),
                            parallel_tool_calls: None,
                            parallel_tool_config: None,
                            reasoning_effort: vt_cfg.and_then(|cfg| {
                                if provider_client.supports_reasoning_effort(&active_model) {
                                    Some(cfg.agent.reasoning_effort.as_str().to_string())
                                } else {
                                    None
                                }
                            }),
                        };
                        let rr = provider_client.generate(review_req).await.ok();
                        if let Some(r) = rr.and_then(|result| result.content)
                            && !r.trim().is_empty()
                        {
                            text = r;
                        }
                    }
                }
                let trimmed = text.trim();
                let suppress_response = trimmed.is_empty()
                    || last_tool_stdout
                        .as_ref()
                        .map(|stdout| stdout == trimmed)
                        .unwrap_or(false);

                let streamed_matches_output = response_streamed
                    && response
                        .content
                        .as_ref()
                        .map(|original| original == &text)
                        .unwrap_or(false);

                if !suppress_response && !streamed_matches_output {
                    renderer.line(MessageStyle::Response, &text)?;
                }
                ensure_turn_bottom_gap(&mut renderer, &mut bottom_gap_applied)?;
                working_history.push(uni::Message::assistant(text));
                let _ = last_tool_stdout.take();
            } else {
                ensure_turn_bottom_gap(&mut renderer, &mut bottom_gap_applied)?;
            }
            break TurnLoopResult::Completed;
        };

        match turn_result {
            TurnLoopResult::Cancelled => {
                session_stats.render_summary(&mut renderer, &conversation_history)?;
                break;
            }
            TurnLoopResult::Aborted => {
                let _ = conversation_history.pop();
                continue;
            }
            TurnLoopResult::Completed => {
                conversation_history = working_history;

                let _pruned_after_turn = prune_unified_tool_responses(
                    &mut conversation_history,
                    trim_config.preserve_recent_turns,
                );
                // Removed: Tool response pruning message after completion
                let post_trim =
                    enforce_unified_context_window(&mut conversation_history, trim_config);
                if post_trim.is_trimmed() {
                    renderer.line(
                        MessageStyle::Info,
                        &format!(
                            "Trimmed {} earlier messages to respect the context window (~{} tokens).",
                            post_trim.removed_messages, trim_config.max_tokens,
                        ),
                    )?;
                }

                if let Some(last) = conversation_history.last()
                    && last.role == uni::MessageRole::Assistant
                {
                    let text = &last.content;
                    let claims_write = text.contains("I've updated")
                        || text.contains("I have updated")
                        || text.contains("updated the `");
                    if claims_write && !any_write_effect {
                        renderer.line_if_not_empty(MessageStyle::Output)?;
                        renderer.line(
                            MessageStyle::Info,
                            "Note: The assistant mentioned edits but no write tool ran.",
                        )?;
                    }
                }
            }
        }
    }

    handle.shutdown();
    Ok(())
}
