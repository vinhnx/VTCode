use anyhow::Result;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

use vtcode_core::config::EditorToolConfig;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::threads::ArchivedSessionIntent;
use vtcode_core::llm::provider as uni;
use vtcode_core::scheduler::{DurableTaskStore, SchedulerDaemon};
use vtcode_core::tools::continuation::{PtyContinuationArgs, ReadChunkContinuationArgs};
use vtcode_core::tools::terminal_app::{EditorLaunchConfig, TerminalAppLauncher};
use vtcode_core::ui::theme;
use vtcode_core::ui::{inline_theme_from_core_styles, to_tui_appearance};
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::prompt::refine_and_enrich_prompt;
use crate::agent::runloop::unified::async_mcp_manager::{
    AsyncMcpManager, approval_policy_from_human_in_the_loop,
};
use crate::agent::runloop::unified::inline_events::InlineLoopAction;
use crate::agent::runloop::unified::interactive_features::{
    PromptSuggestionSource, generate_inline_prompt_suggestion,
};
use crate::agent::runloop::unified::session_setup::{
    apply_ide_context_snapshot, ide_context_status_label_from_bridge,
};
use crate::agent::runloop::unified::status_line::InputStatusState;
use crate::agent::runloop::unified::turn::session::slash_commands::run_with_event_loop_suspended;

use super::super::interaction_loop::{
    InteractionLoopContext, InteractionOutcome, InteractionState,
};

const FALLBACK_ARGS_PREVIEW_LIMIT: usize = 240;
const REVIEW_SCROLLBACK_EXIT_HINT: &str =
    "[Native scrollback view. Press Esc, q, or Alt+O to return to fullscreen.]";

#[derive(Debug, Deserialize)]
struct ToolErrorPayloadHint {
    #[serde(default)]
    fallback_tool: Option<String>,
    #[serde(default)]
    fallback_tool_args: Option<Value>,
    #[serde(default)]
    is_recoverable: Option<bool>,
}

#[derive(Default)]
pub(super) struct LiveIdeContextUpdate {
    pub(super) snapshot: Option<vtcode_core::EditorContextSnapshot>,
    pub(super) changed: bool,
}

pub(super) enum InlineLoopActionResolution {
    ContinueLoop,
    Submit(String),
    Outcome(InteractionOutcome),
}

pub(super) fn extract_recent_follow_up_hint(history: &[uni::Message]) -> Option<(String, Value)> {
    let mut saw_trailing_tool = false;
    for message in history.iter().rev().take(60) {
        if message.is_tool_response() {
            saw_trailing_tool = true;
        } else if message.role == uni::MessageRole::System {
            continue;
        } else {
            break;
        }

        if !message.is_tool_response() {
            continue;
        }

        let content = message.get_text_content();
        let Some(parsed) = serde_json::from_str::<Value>(content.as_ref()).ok() else {
            continue;
        };
        let Some(obj) = parsed.as_object() else {
            continue;
        };

        if let Some(next_continue) = obj
            .get("next_continue_args")
            .and_then(PtyContinuationArgs::from_value)
        {
            return Some((
                tool_names::UNIFIED_EXEC.to_string(),
                json!({
                    "action": "continue",
                    "session_id": next_continue.session_id
                }),
            ));
        }

        if let Some(next_read) = obj
            .get("next_read_args")
            .and_then(ReadChunkContinuationArgs::from_value)
        {
            return Some((
                tool_names::UNIFIED_FILE.to_string(),
                json!({
                    "action": "read",
                    "path": next_read.path,
                    "offset": next_read.offset,
                    "limit": next_read.limit
                }),
            ));
        }

        let content_ref: &str = content.as_ref();
        if content_ref.contains("\"fallback_tool\"")
            && content_ref.contains("\"fallback_tool_args\"")
        {
            let Some(parsed) = serde_json::from_str::<ToolErrorPayloadHint>(content_ref).ok()
            else {
                continue;
            };
            let Some(fallback_tool) = parsed.fallback_tool else {
                continue;
            };
            let Some(fallback_args) = parsed.fallback_tool_args else {
                continue;
            };
            if parsed.is_recoverable == Some(false) {
                continue;
            }
            return Some((fallback_tool, fallback_args));
        }
    }

    if saw_trailing_tool {
        tracing::debug!("No continuation hint found in trailing tool responses");
    }

    None
}

fn review_editor_launch_config(editor_config: &EditorToolConfig) -> EditorLaunchConfig {
    EditorLaunchConfig {
        preferred_editor: (!editor_config.preferred_editor.trim().is_empty())
            .then(|| editor_config.preferred_editor.clone()),
        wait_for_editor: true,
    }
}

async fn open_transcript_review_in_editor(
    ctx: &mut InteractionLoopContext<'_>,
    text: String,
) -> Result<()> {
    let editor_config = ctx
        .vt_cfg
        .as_ref()
        .map(|config| config.tools.editor.clone())
        .unwrap_or_default();
    if !editor_config.enabled {
        ctx.renderer.line(
            MessageStyle::Warning,
            "External editor is disabled (`tools.editor.enabled = false`).",
        )?;
        return Ok(());
    }

    let mut temp = tempfile::NamedTempFile::new()?;
    temp.write_all(text.as_bytes())?;
    temp.flush()?;
    let (_, path) = temp.keep()?;
    let launcher = TerminalAppLauncher::new(ctx.config.workspace.clone());
    let launch_config = review_editor_launch_config(&editor_config);
    let result = run_with_event_loop_suspended(ctx.handle, editor_config.suspend_tui, || {
        launcher.launch_editor_with_config(Some(path.clone()), launch_config)
    })
    .await;
    let cleanup = fs::remove_file(&path);
    ctx.handle.force_redraw();

    match result {
        Ok(_) => {
            ctx.renderer
                .line(MessageStyle::Info, "Transcript review opened in editor.")?;
        }
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to open transcript review in editor: {err}"),
            )?;
        }
    }

    if let Err(err) = cleanup {
        tracing::debug!(%err, path = %path.display(), "failed to remove transcript review temp file");
    }

    Ok(())
}

fn show_transcript_review_in_scrollback(text: &str, mouse_capture: bool) -> Result<()> {
    use ratatui::crossterm::{
        event::{
            self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture,
            EnableBracketedPaste, EnableFocusChange, EnableMouseCapture, Event, KeyCode,
            KeyEventKind, KeyModifiers,
        },
        execute,
        terminal::{Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
    };

    let mut stderr = io::stderr();
    execute!(stderr, LeaveAlternateScreen)?;
    if mouse_capture {
        let _ = execute!(stderr, DisableMouseCapture);
    }
    let _ = execute!(stderr, DisableFocusChange, DisableBracketedPaste);
    write!(stderr, "{text}")?;
    if !text.ends_with('\n') {
        writeln!(stderr)?;
    }
    writeln!(stderr)?;
    writeln!(stderr, "{REVIEW_SCROLLBACK_EXIT_HINT}")?;
    stderr.flush()?;

    loop {
        match event::read()? {
            Event::Key(key)
                if matches!(key.kind, KeyEventKind::Press)
                    && (matches!(
                        key.code,
                        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q')
                    ) || (key.modifiers.contains(KeyModifiers::ALT)
                        && matches!(key.code, KeyCode::Char('o') | KeyCode::Char('O')))) =>
            {
                break;
            }
            _ => {}
        }
    }

    execute!(stderr, EnterAlternateScreen, Clear(ClearType::All))?;
    let _ = execute!(stderr, EnableBracketedPaste, EnableFocusChange);
    if mouse_capture {
        let _ = execute!(stderr, EnableMouseCapture);
    }
    stderr.flush()?;
    Ok(())
}

async fn open_transcript_review_scrollback(
    ctx: &mut InteractionLoopContext<'_>,
    text: String,
) -> Result<()> {
    let mouse_capture = ctx
        .vt_cfg
        .as_ref()
        .map(|config| config.ui.fullscreen.mouse_capture)
        .unwrap_or(true);
    let result = run_with_event_loop_suspended(ctx.handle, true, || {
        show_transcript_review_in_scrollback(&text, mouse_capture)
    })
    .await;
    ctx.handle.force_redraw();
    if let Err(err) = result {
        ctx.renderer.line(
            MessageStyle::Error,
            &format!("Failed to open transcript in native scrollback: {err}"),
        )?;
    }
    Ok(())
}

pub(super) fn fallback_args_preview(args: &Value) -> String {
    let serialized =
        serde_json::to_string(args).unwrap_or_else(|_| "{\"action\":\"list\"}".to_string());
    let mut chars = serialized.chars();
    let mut preview = String::with_capacity(FALLBACK_ARGS_PREVIEW_LIMIT + 3);
    for _ in 0..FALLBACK_ARGS_PREVIEW_LIMIT {
        if let Some(ch) = chars.next() {
            preview.push(ch);
        } else {
            return serialized;
        }
    }
    if chars.next().is_some() {
        preview.push_str("...");
    }
    preview
}

pub(super) fn stalled_follow_up_recovery_prompt(
    stall_reason: &str,
    has_fallback_hint: bool,
) -> String {
    if has_fallback_hint {
        format!(
            "Continue autonomously from the last stalled turn. Stall reason: {}. Use the recovered fallback hint as the first adjusted strategy, then continue until you can provide a concrete conclusion and final review.",
            stall_reason
        )
    } else {
        format!(
            "Continue autonomously from the last stalled turn. Stall reason: {}. Keep working until you can provide a concrete conclusion and final review.",
            stall_reason
        )
    }
}

fn append_file_reference_metadata(
    content: uni::MessageContent,
    input: &str,
    workspace: &Path,
) -> uni::MessageContent {
    let Some(metadata) = build_file_reference_metadata(input, workspace) else {
        return content;
    };

    match content {
        uni::MessageContent::Text(text) => uni::MessageContent::text(format!("{text}{metadata}")),
        uni::MessageContent::Parts(mut parts) => {
            parts.push(uni::ContentPart::text(metadata));
            uni::MessageContent::parts(parts)
        }
    }
}

fn append_agent_reference_metadata(
    content: uni::MessageContent,
    selected_agents: &[String],
) -> uni::MessageContent {
    if selected_agents.is_empty() {
        return content;
    }

    let mut metadata = String::from("\n\n[agent_reference_metadata]\n");
    for mention in selected_agents {
        metadata.push_str(&format!("selected=@agent-{mention}\n"));
    }

    match content {
        uni::MessageContent::Text(text) => uni::MessageContent::text(format!("{text}{metadata}")),
        uni::MessageContent::Parts(mut parts) => {
            parts.push(uni::ContentPart::text(metadata));
            uni::MessageContent::parts(parts)
        }
    }
}

fn supports_native_openai_file_inputs(
    provider_name: &str,
    model_supports_responses_compaction: bool,
) -> bool {
    provider_name.eq_ignore_ascii_case("openai") && model_supports_responses_compaction
}

async fn handle_inline_prompt_suggestion_request(
    ctx: &mut InteractionLoopContext<'_>,
    state: &mut InteractionState<'_>,
    draft: &str,
) -> Result<()> {
    let Some(suggestion) = generate_inline_prompt_suggestion(
        ctx.provider_client.as_ref(),
        ctx.config,
        ctx.vt_cfg.as_ref(),
        &ctx.config.workspace,
        ctx.conversation_history,
        ctx.session_stats,
        ctx.tool_registry,
        draft,
    )
    .await
    else {
        ctx.handle.clear_inline_prompt_suggestion();
        ctx.renderer.line(
            MessageStyle::Info,
            "No inline prompt suggestion is available for the current draft.",
        )?;
        return Ok(());
    };

    if suggestion.source == PromptSuggestionSource::Llm
        && ctx
            .vt_cfg
            .as_ref()
            .map(|cfg| cfg.agent.prompt_suggestions.show_cost_notice)
            .unwrap_or(true)
        && !*state.inline_prompt_cost_notice_shown
    {
        ctx.renderer.line(
            MessageStyle::Info,
            "Inline prompt suggestions may use tokens when VT Code calls your configured LLM provider.",
        )?;
        *state.inline_prompt_cost_notice_shown = true;
    }

    ctx.handle.set_inline_prompt_suggestion(
        suggestion.prompt,
        suggestion.source == PromptSuggestionSource::Llm,
    );
    Ok(())
}

fn build_file_reference_metadata(input: &str, workspace: &Path) -> Option<String> {
    let mut alias_to_full_path = BTreeMap::new();
    for at_match in vtcode_commons::at_pattern::find_at_patterns(input) {
        let alias = at_match.path.trim();
        if alias.is_empty() {
            continue;
        }
        if let Some(full_path) = resolve_full_path_for_alias(alias, workspace) {
            alias_to_full_path.insert(format!("@{alias}"), full_path);
        }
    }

    if alias_to_full_path.is_empty() {
        return None;
    }

    let mut metadata = String::from("\n\n[file_reference_metadata]\n");
    for (alias, full_path) in alias_to_full_path {
        metadata.push_str(&format!("{}={}\n", alias, full_path));
    }

    Some(metadata)
}

fn resolve_full_path_for_alias(alias: &str, workspace: &Path) -> Option<String> {
    let trimmed = alias.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("data:")
        || !vtcode_commons::paths::is_safe_relative_path(trimmed)
    {
        return None;
    }

    let resolved =
        vtcode_commons::paths::resolve_workspace_path(workspace, Path::new(trimmed)).ok()?;
    Some(resolved.to_string_lossy().to_string())
}

pub(super) async fn build_user_message_content(
    ctx: &mut InteractionLoopContext<'_>,
    input: &str,
) -> uni::MessageContent {
    let allow_structured_non_image_file_inputs = supports_native_openai_file_inputs(
        &ctx.config.provider,
        ctx.provider_client
            .supports_responses_compaction(&ctx.config.model),
    );
    let processed_content = match vtcode_core::utils::at_pattern::parse_at_patterns_with_options(
        input,
        &ctx.config.workspace,
        vtcode_core::utils::at_pattern::AtPatternOptions {
            allow_local_non_image_file_inputs: allow_structured_non_image_file_inputs,
            allow_remote_non_image_file_inputs: allow_structured_non_image_file_inputs,
        },
    )
    .await
    {
        Ok(content) => content,
        Err(err) => {
            tracing::warn!("Failed to parse @ patterns: {}", err);
            uni::MessageContent::text(input.to_string())
        }
    };

    let refined_content = match &processed_content {
        uni::MessageContent::Text(text) => {
            let refined_text =
                refine_and_enrich_prompt(text, ctx.config, ctx.vt_cfg.as_ref()).await;
            uni::MessageContent::text(refined_text)
        }
        uni::MessageContent::Parts(parts) => {
            let mut refined_parts = Vec::new();
            for part in parts {
                match part {
                    uni::ContentPart::Text { text } => {
                        let refined_text =
                            refine_and_enrich_prompt(text, ctx.config, ctx.vt_cfg.as_ref()).await;
                        refined_parts.push(uni::ContentPart::text(refined_text));
                    }
                    _ => refined_parts.push(part.clone()),
                }
            }
            uni::MessageContent::parts(refined_parts)
        }
    };
    let selected_agents: Vec<String> =
        if let Some(controller) = ctx.tool_registry.subagent_controller() {
            controller.set_turn_delegation_hints_from_input(input).await
        } else {
            Vec::new()
        };
    let refined_content =
        append_file_reference_metadata(refined_content, input, &ctx.config.workspace);
    append_agent_reference_metadata(refined_content, selected_agents.as_slice())
}

pub(super) fn refresh_ide_context_before_user_turn(
    ctx: &mut InteractionLoopContext<'_>,
    input_status_state: &mut InputStatusState,
) {
    let latest_editor_snapshot: Option<vtcode_core::EditorContextSnapshot> =
        if let Some(bridge) = ctx.ide_context_bridge.as_mut() {
            match bridge.refresh() {
                Ok((snapshot, _)) => snapshot,
                Err(err) => {
                    tracing::warn!(error = %err, "Failed to refresh IDE context before user turn");
                    bridge.snapshot().cloned()
                }
            }
        } else {
            None
        };
    apply_ide_context_snapshot(
        ctx.context_manager,
        ctx.header_context,
        ctx.handle,
        ctx.config.workspace.as_path(),
        ctx.vt_cfg.as_ref(),
        latest_editor_snapshot,
    );
    crate::agent::runloop::unified::status_line::update_ide_context_source(
        input_status_state,
        ide_context_status_label_from_bridge(
            ctx.context_manager,
            ctx.config.workspace.as_path(),
            ctx.vt_cfg.as_ref(),
            ctx.ide_context_bridge.as_ref(),
        ),
    );
}

pub(super) fn apply_live_theme_and_appearance(
    handle: &vtcode_tui::app::InlineHandle,
    cfg: &VTCodeConfig,
) {
    let color_config = theme::ColorAccessibilityConfig {
        minimum_contrast: cfg.ui.minimum_contrast,
        bold_is_bright: cfg.ui.bold_is_bright,
        safe_colors_only: cfg.ui.safe_colors_only,
    };
    theme::set_color_accessibility_config(color_config);

    let selected = cfg.agent.theme.trim();
    let selected = if selected.is_empty() {
        theme::DEFAULT_THEME_ID
    } else {
        selected
    };
    if let Err(err) = theme::set_active_theme(selected) {
        tracing::warn!(
            theme = selected,
            error = %err,
            "Failed to activate configured theme; falling back to default"
        );
        let _ = theme::set_active_theme(theme::DEFAULT_THEME_ID);
    }

    let styles = theme::active_styles();
    handle.set_theme(inline_theme_from_core_styles(&styles));
    handle.set_appearance(to_tui_appearance(cfg));
    crate::agent::runloop::unified::palettes::apply_prompt_style(handle);
    handle.force_redraw();
}

fn sync_mcp_approval_policy(
    async_mcp_manager: Option<&AsyncMcpManager>,
    vt_cfg: Option<&VTCodeConfig>,
) {
    let (Some(mcp_manager), Some(cfg)) = (async_mcp_manager, vt_cfg) else {
        return;
    };
    let desired_policy = approval_policy_from_human_in_the_loop(cfg.security.human_in_the_loop);
    if mcp_manager.approval_policy() != desired_policy {
        mcp_manager.set_approval_policy(desired_policy);
    }
}

pub(super) fn sync_mcp_approval_policy_for_context(ctx: &InteractionLoopContext<'_>) {
    sync_mcp_approval_policy(ctx.async_mcp_manager.as_deref(), ctx.vt_cfg.as_ref());
}

pub(super) fn scheduler_enabled(ctx: &InteractionLoopContext<'_>) -> bool {
    let enabled = ctx
        .vt_cfg
        .as_ref()
        .map(|cfg| cfg.automation.scheduled_tasks.enabled)
        .unwrap_or(false);
    vtcode_core::scheduler::scheduled_tasks_enabled(enabled)
}

pub(super) fn build_durable_scheduler_daemon() -> Result<SchedulerDaemon> {
    let store = DurableTaskStore::new_default()?;
    let executable = std::env::current_exe()?;
    Ok(SchedulerDaemon::new(store, executable))
}

pub(super) fn refresh_live_ide_context_update(
    ide_context_bridge: &mut Option<
        crate::agent::runloop::unified::session_setup::IdeContextBridge,
    >,
) -> LiveIdeContextUpdate {
    let Some(bridge) = ide_context_bridge.as_mut() else {
        return LiveIdeContextUpdate::default();
    };

    match bridge.refresh() {
        Ok((snapshot, refresh_state)) => LiveIdeContextUpdate {
            snapshot,
            changed: refresh_state.changed,
        },
        Err(err) => {
            tracing::warn!(
                error = %err,
                "Failed to refresh IDE context during live UI update"
            );
            LiveIdeContextUpdate {
                snapshot: bridge.snapshot().cloned(),
                changed: false,
            }
        }
    }
}

async fn try_resume_archived_session(
    renderer: &mut vtcode_core::utils::ansi::AnsiRenderer,
    session_id: &str,
    intent: ArchivedSessionIntent,
    loading_message: &str,
    success_message: &str,
) -> Result<Option<InteractionOutcome>> {
    renderer.line(
        MessageStyle::Info,
        &format!("{loading_message}: {session_id}"),
    )?;

    match crate::agent::agents::load_resume_session(session_id, intent).await {
        Ok(Some(resume)) => {
            renderer.line(
                MessageStyle::Info,
                &format!("{success_message}: {session_id}"),
            )?;
            Ok(Some(InteractionOutcome::Resume {
                resume_session: Box::new(resume),
            }))
        }
        Ok(None) => {
            renderer.line(
                MessageStyle::Error,
                &format!("Session not found: {}", session_id),
            )?;
            Ok(None)
        }
        Err(err) => {
            renderer.line(
                MessageStyle::Error,
                &format!("Failed to load session: {}", err),
            )?;
            Ok(None)
        }
    }
}

pub(super) async fn resolve_inline_loop_action(
    ctx: &mut InteractionLoopContext<'_>,
    state: &mut InteractionState<'_>,
    inline_action: InlineLoopAction,
) -> Result<InlineLoopActionResolution> {
    let resolution = match inline_action {
        InlineLoopAction::Continue => InlineLoopActionResolution::ContinueLoop,
        InlineLoopAction::Submit(text) => InlineLoopActionResolution::Submit(text),
        InlineLoopAction::RequestInlinePromptSuggestion(draft) => {
            handle_inline_prompt_suggestion_request(ctx, state, &draft).await?;
            InlineLoopActionResolution::ContinueLoop
        }
        InlineLoopAction::OpenTranscriptReviewInEditor(text) => {
            open_transcript_review_in_editor(ctx, text).await?;
            InlineLoopActionResolution::ContinueLoop
        }
        InlineLoopAction::OpenTranscriptReviewScrollback(text) => {
            open_transcript_review_scrollback(ctx, text).await?;
            InlineLoopActionResolution::ContinueLoop
        }
        InlineLoopAction::Exit(reason) => {
            InlineLoopActionResolution::Outcome(InteractionOutcome::Exit { reason })
        }
        InlineLoopAction::PlanApproved { auto_accept } => {
            let mode = if auto_accept {
                "auto-accept edits"
            } else {
                "manual edit approvals"
            };
            let message =
                format!("Plan approved. Switching to Edit Mode and starting execution ({mode}).");
            ctx.renderer.line(MessageStyle::Info, &message)?;
            InlineLoopActionResolution::Outcome(InteractionOutcome::PlanApproved { auto_accept })
        }
        InlineLoopAction::PlanEditRequested => {
            ctx.renderer.line(
                MessageStyle::Info,
                "Staying in Plan Mode. Continue refining the plan.",
            )?;
            InlineLoopActionResolution::ContinueLoop
        }
        InlineLoopAction::ResumeSession(session_id) => {
            if let Some(outcome) = try_resume_archived_session(
                ctx.renderer,
                &session_id,
                ArchivedSessionIntent::ResumeInPlace,
                "Loading session",
                "Restarting with session",
            )
            .await?
            {
                InlineLoopActionResolution::Outcome(outcome)
            } else {
                InlineLoopActionResolution::ContinueLoop
            }
        }
        InlineLoopAction::ForkSession {
            session_id,
            summarize,
        } => {
            if let Some(outcome) = try_resume_archived_session(
                ctx.renderer,
                &session_id,
                ArchivedSessionIntent::ForkNewArchive {
                    custom_suffix: None,
                    summarize,
                },
                "Loading session for fork",
                "Restarting from fork source",
            )
            .await?
            {
                InlineLoopActionResolution::Outcome(outcome)
            } else {
                InlineLoopActionResolution::ContinueLoop
            }
        }
        InlineLoopAction::DiffApproved | InlineLoopAction::DiffRejected => {
            InlineLoopActionResolution::ContinueLoop
        }
    };
    Ok(resolution)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::runloop::unified::context_manager::ContextManager;
    use crate::agent::runloop::unified::session_setup::{
        IdeContextBridge, ide_context_status_label_from_bridge,
    };
    use hashbrown::HashMap;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::sync::RwLock;

    #[test]
    fn extract_recent_follow_up_hint_reads_latest_recoverable_tool_payload() {
        let history = vec![
            uni::Message::tool_response(
                "call_1".to_string(),
                serde_json::json!({
                    "error": "older",
                    "is_recoverable": true,
                    "fallback_tool": "read_file",
                    "fallback_tool_args": {"path":"a.rs","offset":1,"limit":20}
                })
                .to_string(),
            ),
            uni::Message::tool_response(
                "call_2".to_string(),
                serde_json::json!({
                    "error": "newer",
                    "is_recoverable": true,
                    "fallback_tool": "task_tracker",
                    "fallback_tool_args": {"action":"list"}
                })
                .to_string(),
            ),
        ];

        let hint = extract_recent_follow_up_hint(&history);
        assert_eq!(
            hint,
            Some((
                "task_tracker".to_string(),
                serde_json::json!({"action":"list"})
            ))
        );
    }

    #[test]
    fn extract_recent_follow_up_hint_skips_non_recoverable_payloads() {
        let history = vec![uni::Message::tool_response(
            "call_1".to_string(),
            serde_json::json!({
                "error": "blocked",
                "is_recoverable": false,
                "fallback_tool": "read_file",
                "fallback_tool_args": {"path":"a.rs"}
            })
            .to_string(),
        )];

        assert!(extract_recent_follow_up_hint(&history).is_none());
    }

    #[test]
    fn supports_native_openai_file_inputs_requires_openai_provider_and_responses_support() {
        assert!(supports_native_openai_file_inputs("openai", true));
        assert!(!supports_native_openai_file_inputs("openai", false));
        assert!(!supports_native_openai_file_inputs("anthropic", true));
        assert!(!supports_native_openai_file_inputs("mycorp", true));
    }

    #[test]
    fn extract_recent_follow_up_hint_prefers_latest_hint() {
        let history = vec![
            uni::Message::tool_response(
                "call_1".to_string(),
                serde_json::json!({
                    "next_continue_args": {
                        "session_id": "run-42"
                    }
                })
                .to_string(),
            ),
            uni::Message::tool_response(
                "call_2".to_string(),
                serde_json::json!({
                    "fallback_tool": "task_tracker",
                    "fallback_tool_args": {"action":"list"},
                    "is_recoverable": true
                })
                .to_string(),
            ),
        ];
        let hint = extract_recent_follow_up_hint(&history);
        assert_eq!(
            hint,
            Some((
                "task_tracker".to_string(),
                serde_json::json!({"action":"list"})
            ))
        );
    }

    #[test]
    fn extract_recent_follow_up_hint_skips_stale_tool_hints_after_assistant_reply() {
        let history = vec![
            uni::Message::tool_response(
                "call_1".to_string(),
                serde_json::json!({
                    "next_continue_args": {
                        "session_id": "run-123"
                    }
                })
                .to_string(),
            ),
            uni::Message::assistant("All done.".to_string()),
        ];

        assert!(extract_recent_follow_up_hint(&history).is_none());
    }

    #[test]
    fn extract_recent_follow_up_hint_keeps_hint_when_system_note_follows_tool() {
        let history = vec![
            uni::Message::tool_response(
                "call_1".to_string(),
                serde_json::json!({
                    "fallback_tool": "task_tracker",
                    "fallback_tool_args": {"action":"list"},
                    "is_recoverable": true
                })
                .to_string(),
            ),
            uni::Message::system("Tool blocked; try fallback.".to_string()),
        ];

        let hint = extract_recent_follow_up_hint(&history);
        assert_eq!(
            hint,
            Some((
                "task_tracker".to_string(),
                serde_json::json!({"action":"list"})
            ))
        );
    }

    #[test]
    fn extract_recent_follow_up_hint_ignores_next_action_when_fallback_exists() {
        let history = vec![uni::Message::tool_response(
            "call_1".to_string(),
            serde_json::json!({
                "error": "Tool preflight validation failed: x",
                "is_recoverable": true,
                "fallback_tool": "task_tracker",
                "fallback_tool_args": {"action":"list"},
                "next_action": "Retry with fallback_tool_args."
            })
            .to_string(),
        )];

        let hint = extract_recent_follow_up_hint(&history);
        assert_eq!(
            hint,
            Some((
                "task_tracker".to_string(),
                serde_json::json!({"action":"list"})
            ))
        );
    }

    #[test]
    fn extract_recent_follow_up_hint_does_not_promote_next_action_only_payload() {
        let history = vec![uni::Message::tool_response(
            "call_1".to_string(),
            serde_json::json!({
                "error": "boom",
                "is_recoverable": true,
                "next_action": "Try an alternative tool or narrower scope."
            })
            .to_string(),
        )];

        assert!(extract_recent_follow_up_hint(&history).is_none());
    }

    #[test]
    fn extract_recent_follow_up_hint_reads_next_continue_args() {
        let history = vec![uni::Message::tool_response(
            "call_1".to_string(),
            serde_json::json!({
                "next_continue_args": {
                    "session_id": "run-123"
                }
            })
            .to_string(),
        )];

        let hint = extract_recent_follow_up_hint(&history);
        assert_eq!(
            hint,
            Some((
                tool_names::UNIFIED_EXEC.to_string(),
                serde_json::json!({
                    "action": "continue",
                    "session_id": "run-123"
                })
            ))
        );
    }

    #[test]
    fn extract_recent_follow_up_hint_reads_next_read_args() {
        let history = vec![uni::Message::tool_response(
            "call_1".to_string(),
            serde_json::json!({
                "next_read_args": {
                    "path": ".vtcode/context/tool_outputs/out.txt",
                    "offset": 41,
                    "limit": 40
                }
            })
            .to_string(),
        )];

        let hint = extract_recent_follow_up_hint(&history);
        assert_eq!(
            hint,
            Some((
                tool_names::UNIFIED_FILE.to_string(),
                serde_json::json!({
                    "action": "read",
                    "path": ".vtcode/context/tool_outputs/out.txt",
                    "offset": 41,
                    "limit": 40
                })
            ))
        );
    }

    #[test]
    fn fallback_args_preview_truncates_long_payloads() {
        let args = serde_json::json!({
            "action": "search",
            "query": "x".repeat(600)
        });
        let preview = fallback_args_preview(&args);
        assert!(preview.ends_with("..."));
        assert!(preview.len() <= 243);
    }

    #[test]
    fn stalled_follow_up_recovery_prompt_mentions_fallback_without_replacing_user_input() {
        let prompt = stalled_follow_up_recovery_prompt("turn blocked", true);

        assert!(prompt.contains("Stall reason: turn blocked"));
        assert!(prompt.contains("Use the recovered fallback hint"));
    }

    #[test]
    fn build_file_reference_metadata_maps_aliases_to_full_paths() {
        let temp_dir = TempDir::new().expect("temp dir");
        let file_path = temp_dir.path().join("src").join("main.rs");
        fs::create_dir_all(file_path.parent().expect("parent")).expect("mkdir");
        fs::write(&file_path, "fn main() {}\n").expect("write file");

        let metadata =
            build_file_reference_metadata("check @src/main.rs and continue", temp_dir.path())
                .expect("metadata");

        assert!(metadata.contains("@src/main.rs="));
        assert!(metadata.contains("src/main.rs"));
    }

    #[test]
    fn append_file_reference_metadata_keeps_ui_alias_but_adds_full_path_context() {
        let temp_dir = TempDir::new().expect("temp dir");
        let file_path = temp_dir.path().join("README.md");
        fs::write(&file_path, "# test\n").expect("write file");

        let content = uni::MessageContent::text("check @README.md".to_string());
        let augmented =
            append_file_reference_metadata(content, "check @README.md", temp_dir.path());

        match augmented {
            uni::MessageContent::Text(text) => {
                assert!(text.contains("check @README.md"));
                assert!(text.contains("[file_reference_metadata]"));
                assert!(text.contains("@README.md="));
                assert!(text.contains("README.md"));
            }
            uni::MessageContent::Parts(_) => panic!("expected text content"),
        }
    }

    #[test]
    fn append_agent_reference_metadata_adds_selected_agent_hint() {
        let content = uni::MessageContent::text("use rust-engineer agent".to_string());
        let augmented = append_agent_reference_metadata(content, &[String::from("rust-engineer")]);

        match augmented {
            uni::MessageContent::Text(text) => {
                assert!(text.contains("use rust-engineer agent"));
                assert!(text.contains("[agent_reference_metadata]"));
                assert!(text.contains("selected=@agent-rust-engineer"));
            }
            uni::MessageContent::Parts(_) => panic!("expected text content"),
        }
    }

    #[test]
    fn build_file_reference_metadata_ignores_non_file_aliases() {
        let temp_dir = TempDir::new().expect("temp dir");
        let metadata = build_file_reference_metadata("npm i @types/node", temp_dir.path());
        assert!(metadata.is_none());
    }

    #[test]
    fn build_file_reference_metadata_ignores_absolute_paths() {
        let temp_dir = TempDir::new().expect("temp dir");
        let metadata = build_file_reference_metadata("check @/tmp/example.rs", temp_dir.path());
        assert!(metadata.is_none());
    }

    #[test]
    fn refresh_live_ide_context_update_tracks_active_file_changes() {
        let temp_dir = TempDir::new().expect("temp dir");
        let workspace = temp_dir.path();
        fs::create_dir_all(workspace.join(".vtcode")).expect("create ide context dir");
        let context_manager = ContextManager::new(
            "sys".into(),
            (),
            Arc::new(RwLock::new(HashMap::new())),
            None,
        );

        let snapshot_path = workspace.join(".vtcode/ide-context.json");
        let mut bridge = Some(IdeContextBridge::new(workspace));

        fs::write(
            &snapshot_path,
            serde_json::json!({
                "version": 1,
                "provider_family": "vscode_compatible",
                "editor_name": "VS Code",
                "workspace_root": workspace,
                "active_file": {
                    "path": workspace.join("src/alpha.rs"),
                    "language_id": "rust",
                    "line_range": { "start": 1, "end": 8 },
                    "dirty": false,
                    "truncated": false
                }
            })
            .to_string(),
        )
        .expect("write alpha snapshot");

        let first = refresh_live_ide_context_update(&mut bridge);
        assert!(first.changed);
        assert_eq!(
            ide_context_status_label_from_bridge(
                &context_manager,
                workspace,
                None,
                bridge.as_ref()
            )
            .as_deref(),
            Some("IDE Context (VS Code): src/alpha.rs")
        );

        fs::write(
            &snapshot_path,
            serde_json::json!({
                "version": 1,
                "provider_family": "vscode_compatible",
                "editor_name": "VS Code",
                "workspace_root": workspace,
                "active_file": {
                    "path": workspace.join("src/beta.rs"),
                    "language_id": "rust",
                    "line_range": { "start": 3, "end": 12 },
                    "dirty": false,
                    "truncated": false
                }
            })
            .to_string(),
        )
        .expect("write beta snapshot");

        let second = refresh_live_ide_context_update(&mut bridge);
        assert!(second.changed);
        assert_eq!(
            ide_context_status_label_from_bridge(
                &context_manager,
                workspace,
                None,
                bridge.as_ref()
            )
            .as_deref(),
            Some("IDE Context (VS Code): src/beta.rs")
        );
    }
}
