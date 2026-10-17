use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fmt::Write;
use std::fs;
use std::io::{self, Write as _};
use std::path::Path;

use vtcode_config::models::{ModelId, Provider};
use vtcode_core::config::EditorToolConfig;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::threads::ArchivedSessionIntent;
use vtcode_core::llm::ModelResolver;
use vtcode_core::llm::provider as uni;
use vtcode_core::scheduler::{DurableTaskStore, SchedulerDaemon};
use vtcode_core::tools::continuation::{PtyContinuationArgs, ReadChunkContinuationArgs};
use vtcode_core::tools::terminal_app::{EditorLaunchConfig, TerminalAppLauncher};
use vtcode_core::tools::tool_intent::{VERIFIER_SHELL_FORM_NOTE, verifier_reference};
use vtcode_core::ui::theme;
use vtcode_core::ui::{inline_theme_from_core_styles, to_tui_appearance, to_tui_fullscreen};
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_ui::tui::app::{ContentPart as UiContentPart, SubmittedInput};

use crate::agent::runloop::prompt::refine_and_enrich_prompt;
use crate::agent::runloop::unified::async_mcp_manager::{AsyncMcpManager, approval_policy_from_human_in_the_loop};
use crate::agent::runloop::unified::external_editor::run_blocking_with_event_loop_suspended;
use crate::agent::runloop::unified::inline_events::InlineLoopAction;
use crate::agent::runloop::unified::interactive_features::{PromptSuggestionSource, generate_inline_prompt_suggestion};
use crate::agent::runloop::unified::turn::primary_agent_runtime::{
    PrimaryAgentRuntimeSyncContext, load_primary_agent_specs as load_primary_agent_specs_for_runtime,
    sync_primary_agent_runtime as sync_primary_agent_runtime_context,
};
use crate::startup::{auto_grant_tui_full_auto_workspace_trust, ensure_full_auto_workspace_trust};

use crate::agent::runloop::unified::planning_workflow::PlanningFinishReason;
use crate::agent::runloop::unified::planning_workflow_state::transition_to_planning_workflow;
use crate::agent::runloop::welcome::SessionBootstrap;
use vtcode_core::core::interfaces::session::PlanningEntrySource;

use super::super::interaction_loop::{InteractionLoopContext, InteractionOutcome, InteractionState};

const FALLBACK_ARGS_PREVIEW_LIMIT: usize = 240;
const TOOL_OUTPUT_SCROLLBACK_EXIT_HINT: &str = "[Native tool-output view. Press Esc or q to return to fullscreen.]";

#[derive(Debug, Deserialize)]
struct ToolErrorPayloadHint {
    #[serde(default)]
    fallback_tool: Option<String>,
    #[serde(default)]
    fallback_tool_args: Option<Value>,
    #[serde(default)]
    is_recoverable: Option<bool>,
}

pub(super) enum InlineLoopActionResolution {
    ContinueLoop,
    Submit(SubmittedInput),
    SubmitPrompt(SubmittedInput),
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

        if let Some(next_continue) = obj.get("next_continue_args").and_then(PtyContinuationArgs::from_value) {
            return Some((
                tool_names::WRITE_STDIN.to_string(),
                json!({
                    "session_id": next_continue.session_id,
                    "chars": ""
                }),
            ));
        }

        if let Some(next_read) = obj.get("next_read_args").and_then(ReadChunkContinuationArgs::from_value) {
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
        if content_ref.contains("\"fallback_tool\"") && content_ref.contains("\"fallback_tool_args\"") {
            let Some(parsed) = serde_json::from_str::<ToolErrorPayloadHint>(content_ref).ok() else {
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

async fn open_tool_output_in_editor(ctx: &mut InteractionLoopContext<'_>, text: String) -> Result<()> {
    let editor_config = ctx
        .vt_cfg
        .as_ref()
        .map(|config| config.tools.editor.clone())
        .unwrap_or_default();
    if !editor_config.enabled {
        ctx.renderer
            .line(MessageStyle::Warning, "External editor is disabled (`tools.editor.enabled = false`).")?;
        return Ok(());
    }

    let mut temp = tempfile::NamedTempFile::new()?;
    temp.write_all(text.as_bytes())?;
    temp.flush()?;
    let (_, path) = temp.keep()?;
    let path_for_cleanup = path.clone();
    let launcher = TerminalAppLauncher::new(ctx.config.workspace.clone());
    let launch_config = review_editor_launch_config(&editor_config);
    let result = run_blocking_with_event_loop_suspended(ctx.handle, editor_config.suspend_tui, move || {
        launcher.launch_editor_with_config(Some(path.clone()), launch_config)
    })
    .await;
    let cleanup = fs::remove_file(&path_for_cleanup);
    ctx.handle.force_redraw();

    match result {
        Ok(_) => {
            ctx.renderer.line(MessageStyle::Info, "Tool output opened in editor.")?;
        }
        Err(err) => {
            ctx.renderer
                .line(MessageStyle::Error, &format!("Failed to open tool output in editor: {err}"))?;
        }
    }

    if let Err(err) = cleanup {
        tracing::debug!(%err, path = %path_for_cleanup.display(), "failed to remove tool output temp file");
    }

    Ok(())
}

async fn launch_input_editor_with_draft(ctx: &mut InteractionLoopContext<'_>, draft: &str) -> Result<()> {
    let editor_config = ctx
        .vt_cfg
        .as_ref()
        .map(|config| config.tools.editor.clone())
        .unwrap_or_default();
    if !editor_config.enabled {
        ctx.renderer
            .line(MessageStyle::Warning, "External editor is disabled (`tools.editor.enabled = false`).")?;
        return Ok(());
    }

    let mut temp = tempfile::NamedTempFile::new()?;
    temp.write_all(draft.as_bytes())?;
    temp.flush()?;
    let (_, path) = temp.keep()?;
    let path_for_cleanup = path.clone();
    let launcher = TerminalAppLauncher::new(ctx.config.workspace.clone());
    let launch_config = review_editor_launch_config(&editor_config);
    let result = run_blocking_with_event_loop_suspended(ctx.handle, editor_config.suspend_tui, move || {
        launcher.launch_editor_with_config(Some(path.clone()), launch_config)
    })
    .await;

    let (message_style, message) = match result {
        Ok(_) => {
            let content = fs::read_to_string(&path_for_cleanup)
                .with_context(|| format!("failed to read edited content from {}", path_for_cleanup.display()))?;
            ctx.handle.set_input(content);
            (MessageStyle::Info, "Editor closed. Input updated with edited content.".to_owned())
        }
        Err(err) => (MessageStyle::Error, format!("Failed to launch editor: {err}")),
    };

    if let Err(err) = fs::remove_file(&path_for_cleanup) {
        tracing::debug!(%err, path = %path_for_cleanup.display(), "failed to remove input editor temp file");
    }

    ctx.handle.force_redraw();
    ctx.renderer.line(message_style, &message)?;
    Ok(())
}

fn show_tool_output_in_scrollback(text: &str, mouse_capture: bool) -> Result<()> {
    use ratatui::crossterm::{
        event::{
            self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
            EnableFocusChange, EnableMouseCapture, Event, KeyCode, KeyEventKind,
        },
        execute,
        terminal::{Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
    };

    let mut stderr = io::stderr();
    // Purge the alternate viewport before leaving so the fullscreen TUI frame
    // is never revealed in the main scrollback (mirrors the canonical
    // `panic_hook::restore_tui` ordering).
    // Best-effort: a failed clear must not block the native scrollback view.
    let _ = execute!(stderr, Clear(ClearType::All));
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
    writeln!(stderr, "{TOOL_OUTPUT_SCROLLBACK_EXIT_HINT}")?;
    stderr.flush()?;

    loop {
        match event::read()? {
            Event::Key(key)
                if matches!(key.kind, KeyEventKind::Press)
                    && matches!(key.code, KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q')) =>
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

async fn open_tool_output_scrollback(ctx: &mut InteractionLoopContext<'_>, text: String) -> Result<()> {
    let mouse_capture = ctx
        .vt_cfg
        .as_ref()
        .map(|config| config.ui.fullscreen.mouse_capture)
        .unwrap_or(true);
    let result = run_blocking_with_event_loop_suspended(ctx.handle, true, move || {
        show_tool_output_in_scrollback(&text, mouse_capture)
    })
    .await;
    ctx.handle.force_redraw();
    if let Err(err) = result {
        ctx.renderer
            .line(MessageStyle::Error, &format!("Failed to open tool output in native scrollback: {err}"))?;
    }
    Ok(())
}

pub(super) fn fallback_args_preview(args: &Value) -> String {
    let serialized = serde_json::to_string(args).unwrap_or_else(|_| "{\"action\":\"list\"}".to_string());
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

pub(super) fn stalled_follow_up_recovery_prompt(stall_reason: &str, has_fallback_hint: bool) -> String {
    let clean_stall_reason = stall_reason
        .replace("Tools remain disabled while the recovery response is finalized.", "")
        .replace("Tools remain disabled while the recovery response is finalized", "");
    let clean_stall_reason = clean_stall_reason.trim();
    if has_fallback_hint {
        format!(
            "Continue from the last stalled turn. Stall reason: {clean_stall_reason}. Tools are fully enabled for this turn. Use the recovered fallback hint as the first adjusted strategy, review previous outputs, adjust your approach to avoid the prior blocker, and continue toward the objective."
        )
    } else {
        format!(
            "Continue from the last stalled turn. Stall reason: {clean_stall_reason}. Tools are fully enabled for this turn. Review previous outputs, adjust your approach to avoid the prior blocker, and continue toward the objective."
        )
    }
}

/// Verifier-first resume directive for a stall with the verification gate
/// still pending. Unlike the generic stalled-follow-up directive (which pushes
/// toward a conclusion), this keeps the original task alive: verify the
/// pending edits with the detected project command first, then resume.
/// `default_verifier` should come from `resolve_harness_verifier_command`;
/// `None` names the generic build/test/lint description rather than a
/// command that may not exist in this workspace.
pub(super) fn stalled_verification_resume_directive(default_verifier: Option<&str>) -> String {
    let verifier = verifier_reference(default_verifier);
    format!(
        "Previous turn stalled with edits still awaiting verification. Run {verifier} with `exec_command`, \
        standalone or as a pure `&&` chain of verifiers, and once it exits 0 resume the original request from where \
        it stalled. {VERIFIER_SHELL_FORM_NOTE} The turn cannot complete while the gate is pending, so a completion \
        claim before a verifier passes ends the turn blocked; a failed verifier grants a bounded number of fix-up \
        edits before the next verification."
    )
}

fn append_trailing_text_part(content: &mut uni::MessageContent, trailing_text: String) {
    match content {
        uni::MessageContent::Text(text) => text.push_str(&trailing_text),
        uni::MessageContent::Parts(parts) => parts.push(uni::ContentPart::text(trailing_text)),
    }
}

fn append_file_reference_metadata(content: &mut uni::MessageContent, input: &str, workspace: &Path) {
    let Some(metadata) = build_file_reference_metadata(input, workspace) else {
        return;
    };

    append_trailing_text_part(content, metadata);
}

fn append_agent_reference_metadata(content: &mut uni::MessageContent, selected_agents: &[String]) {
    if selected_agents.is_empty() {
        return;
    }

    let mut metadata = String::from("\n\n[agent_reference_metadata]\n");
    for mention in selected_agents {
        let _ = writeln!(metadata, "selected=@agent-{mention}");
    }

    append_trailing_text_part(content, metadata);
}

fn supports_native_openai_file_inputs(provider_name: &str, model_supports_responses_compaction: bool) -> bool {
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
        ctx.renderer
            .line(MessageStyle::Info, "No inline prompt suggestion is available for the current draft.")?;
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

    ctx.handle
        .set_inline_prompt_suggestion(suggestion.prompt, suggestion.source == PromptSuggestionSource::Llm);
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
    for (alias, full_path) in &alias_to_full_path {
        let _ = writeln!(metadata, "{alias}={full_path}");
    }
    metadata.push_str(
        "Hint: Read each referenced file once using the resolved path above. Do not re-read unless truncated.\n",
    );

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

    let resolved = vtcode_commons::paths::resolve_workspace_path(workspace, Path::new(trimmed)).ok()?;
    Some(resolved.to_string_lossy().to_string())
}

pub(super) async fn build_user_message_content(
    ctx: &mut InteractionLoopContext<'_>,
    input: &SubmittedInput,
) -> uni::MessageContent {
    let text = input.text.as_str();
    let allow_structured_non_image_file_inputs = supports_native_openai_file_inputs(
        &ctx.config.provider,
        ctx.provider_client.supports_responses_compaction(&ctx.config.model),
    );
    let processed_content = match vtcode_core::utils::at_pattern::parse_at_patterns_with_options(
        text,
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
            uni::MessageContent::text(text.to_string())
        }
    };

    let refined_content = match &processed_content {
        uni::MessageContent::Text(text) => {
            let refined_text = refine_and_enrich_prompt(text, ctx.config, ctx.vt_cfg.as_ref()).await;
            uni::MessageContent::text(refined_text)
        }
        uni::MessageContent::Parts(parts) => {
            let mut refined_parts = Vec::new();
            for part in parts {
                match part {
                    uni::ContentPart::Text { text } => {
                        let refined_text = refine_and_enrich_prompt(text, ctx.config, ctx.vt_cfg.as_ref()).await;
                        refined_parts.push(uni::ContentPart::text(refined_text));
                    }
                    _ => refined_parts.push(part.clone()),
                }
            }
            uni::MessageContent::parts(refined_parts)
        }
    };
    let selected_agents: Vec<String> = if let Some(controller) = ctx.tool_registry.subagent_controller() {
        controller.set_turn_delegation_hints_from_input(text).await
    } else {
        Vec::new()
    };
    let mut refined_content = refined_content;
    append_submitted_attachments(&mut refined_content, input.attachments.as_slice());
    append_file_reference_metadata(&mut refined_content, text, &ctx.config.workspace);
    append_agent_reference_metadata(&mut refined_content, selected_agents.as_slice());
    refined_content
}

fn append_submitted_attachments(content: &mut uni::MessageContent, attachments: &[UiContentPart]) {
    if attachments.is_empty() {
        return;
    }

    let mut parts = match std::mem::take(content) {
        uni::MessageContent::Text(text) => {
            if text.is_empty() {
                Vec::new()
            } else {
                vec![uni::ContentPart::text(text)]
            }
        }
        uni::MessageContent::Parts(parts) => parts,
    };

    parts.extend(attachments.iter().map(|attachment| match attachment {
        UiContentPart::Text { text } => uni::ContentPart::text(text.clone()),
        UiContentPart::Image { data, media_type } => uni::ContentPart::image(data.to_string(), media_type.clone()),
    }));

    *content = uni::MessageContent::parts(parts);
}

pub(super) fn submitted_images_are_unsupported(
    input: &SubmittedInput,
    model_supports_vision: bool,
    workspace: &Path,
) -> bool {
    !model_supports_vision
        && (input.attachments.iter().any(vtcode_ui::tui::app::ContentPart::is_image)
            || vtcode_core::utils::at_pattern::input_may_parse_image_parts(&input.text, workspace))
}

pub(super) fn selected_model_supports_image_input(
    provider_key: &str,
    model: &str,
    provider_supports_vision: bool,
) -> bool {
    let Some(provider) = parse_image_capability_provider(provider_key) else {
        return provider_supports_vision;
    };

    if let Some(input_modalities) = model_id_input_modalities_for_provider(provider, model) {
        return input_modalities.iter().any(|modality| modality.eq_ignore_ascii_case("image"));
    }

    let Some(resolved) = ModelResolver::resolve(Some(provider.as_ref()), model, &[], None) else {
        return provider_supports_vision;
    };
    let input_modalities = resolved.input_modalities();
    if input_modalities.is_empty() {
        return provider_supports_vision;
    }

    input_modalities.iter().any(|modality| modality.eq_ignore_ascii_case("image"))
}

fn parse_image_capability_provider(provider_key: &str) -> Option<Provider> {
    let trimmed = provider_key.trim();
    if let Ok(provider) = trimmed.parse::<Provider>() {
        return Some(provider);
    }

    // Runtime header labels may include the auth mode. Capability checks should
    // still use the selected provider's model metadata.
    trimmed.starts_with("OpenAI").then_some(Provider::OpenAI)
}

fn model_id_input_modalities_for_provider(provider: Provider, model: &str) -> Option<&'static [&'static str]> {
    for candidate in model_id_candidates(model) {
        let Ok(model_id) = candidate.parse::<ModelId>() else {
            continue;
        };
        if model_id.provider() != provider {
            continue;
        }
        let input_modalities = model_id.input_modalities();
        if !input_modalities.is_empty() {
            return Some(input_modalities);
        }
    }

    None
}

fn model_id_candidates(model: &str) -> Vec<String> {
    let trimmed = model.trim();
    let mut candidates = Vec::new();
    push_unique_candidate(&mut candidates, trimmed);

    let lower = trimmed.to_ascii_lowercase();
    push_unique_candidate(&mut candidates, &lower);

    if let Some((prefix, _)) = trimmed.split_once(" (") {
        let prefix = prefix.trim();
        push_unique_candidate(&mut candidates, prefix);
        let lower_prefix = prefix.to_ascii_lowercase();
        push_unique_candidate(&mut candidates, &lower_prefix);
    }

    candidates
}

fn push_unique_candidate(candidates: &mut Vec<String>, value: &str) {
    if value.is_empty() {
        return;
    }
    if candidates.iter().any(|candidate| candidate == value) {
        return;
    }
    candidates.push(value.to_string());
}

pub(super) fn replace_submitted_input_text(input: &mut SubmittedInput, text: String) {
    input.text = text;
}

pub(super) fn apply_live_theme_and_appearance(
    handle: &vtcode_ui::tui::app::InlineHandle,
    cfg: &VTCodeConfig,
    session_bootstrap: &SessionBootstrap,
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
    handle.program_status(vtcode_commons::program_status::ProgramStatusUpdate::Configure {
        enabled: cfg.ui.program_status.enabled,
    });
    handle.set_fullscreen_interaction(to_tui_fullscreen(cfg));
    handle.set_key_bindings(session_bootstrap.effective_key_bindings(cfg));
    crate::agent::runloop::unified::palettes::apply_prompt_style(handle);
    handle.force_redraw();
}

fn sync_mcp_approval_policy(async_mcp_manager: Option<&AsyncMcpManager>, vt_cfg: Option<&VTCodeConfig>) {
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

async fn try_resume_archived_session(
    renderer: &mut vtcode_core::utils::ansi::AnsiRenderer,
    session_id: &str,
    intent: ArchivedSessionIntent,
    loading_message: &str,
    success_message: &str,
) -> Result<Option<InteractionOutcome>> {
    renderer.line(MessageStyle::Info, &format!("{loading_message}: {session_id}"))?;

    match crate::agent::agents::load_resume_session(session_id, intent).await {
        Ok(Some(resume)) => {
            renderer.line(MessageStyle::Info, &format!("{success_message}: {session_id}"))?;
            Ok(Some(InteractionOutcome::Resume { resume_session: Box::new(resume) }))
        }
        Ok(None) => {
            renderer.line(MessageStyle::Error, &format!("Session not found: {session_id}"))?;
            Ok(None)
        }
        Err(err) => {
            renderer.line(MessageStyle::Error, &format!("Failed to load session: {err}"))?;
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
        InlineLoopAction::SubmitPrompt(text) => InlineLoopActionResolution::SubmitPrompt(text),
        InlineLoopAction::SubmitQueued(queued) => {
            if let Some(primary_agent) = queued.primary_agent {
                handle_select_primary_agent(ctx, state, Some(primary_agent)).await?;
            }
            InlineLoopActionResolution::Submit(queued.input)
        }
        InlineLoopAction::CyclePrimaryAgent => {
            handle_cycle_primary_agent(ctx, state).await?;
            InlineLoopActionResolution::ContinueLoop
        }
        InlineLoopAction::CyclePrimaryAgentPrevious => {
            handle_cycle_primary_agent_previous(ctx, state).await?;
            InlineLoopActionResolution::ContinueLoop
        }
        InlineLoopAction::SelectPrimaryAgent { name } => {
            handle_select_primary_agent(ctx, state, name).await?;
            InlineLoopActionResolution::ContinueLoop
        }
        InlineLoopAction::RequestInlinePromptSuggestion(draft) => {
            handle_inline_prompt_suggestion_request(ctx, state, &draft).await?;
            InlineLoopActionResolution::ContinueLoop
        }
        InlineLoopAction::OpenToolOutputInEditor(text) => {
            open_tool_output_in_editor(ctx, text).await?;
            InlineLoopActionResolution::ContinueLoop
        }
        InlineLoopAction::OpenToolOutputScrollback(text) => {
            open_tool_output_scrollback(ctx, text).await?;
            InlineLoopActionResolution::ContinueLoop
        }
        InlineLoopAction::Exit(reason) => InlineLoopActionResolution::Outcome(InteractionOutcome::Exit { reason }),
        InlineLoopAction::PlanApproved { target } => {
            let message = match target.execution_context {
                crate::agent::runloop::unified::planning_workflow::PlanExecutionContext::Current => {
                    "Plan approved. Starting execution in the current context."
                }
                crate::agent::runloop::unified::planning_workflow::PlanExecutionContext::Fresh => {
                    "Plan approved. Preparing a fresh execution thread."
                }
            };
            ctx.renderer.line(MessageStyle::Info, message)?;
            InlineLoopActionResolution::Outcome(InteractionOutcome::PlanApproved { target })
        }
        InlineLoopAction::PlanEditRequested => {
            ctx.renderer
                .line(MessageStyle::Info, "Continuing the planning workflow. Refine the plan before execution.")?;
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
        InlineLoopAction::ForkSession { session_id, summarize } => {
            if let Some(outcome) = try_resume_archived_session(
                ctx.renderer,
                &session_id,
                ArchivedSessionIntent::ForkNewArchive { custom_suffix: None, summarize },
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
        InlineLoopAction::LaunchEditorWithDraft { draft } => {
            launch_input_editor_with_draft(ctx, &draft).await?;
            InlineLoopActionResolution::ContinueLoop
        }
        InlineLoopAction::DiffApproved | InlineLoopAction::DiffRejected => InlineLoopActionResolution::ContinueLoop,
    };
    Ok(resolution)
}

async fn handle_cycle_primary_agent(
    ctx: &mut InteractionLoopContext<'_>,
    state: &mut InteractionState<'_>,
) -> Result<()> {
    let Some(specs) = load_primary_agent_specs_or_report(ctx).await? else {
        return Ok(());
    };
    match next_primary_agent_name(ctx.active_primary_agent.active(), &specs) {
        Some(name) => handle_select_primary_agent(ctx, state, Some(name)).await,
        None => {
            ctx.renderer.line(MessageStyle::Error, "No primary agents are available.")?;
            Ok(())
        }
    }
}

async fn handle_cycle_primary_agent_previous(
    ctx: &mut InteractionLoopContext<'_>,
    state: &mut InteractionState<'_>,
) -> Result<()> {
    let Some(specs) = load_primary_agent_specs_or_report(ctx).await? else {
        return Ok(());
    };
    match previous_primary_agent_name(ctx.active_primary_agent.active(), &specs) {
        Some(name) => handle_select_primary_agent(ctx, state, Some(name)).await,
        None => {
            ctx.renderer.line(MessageStyle::Error, "No primary agents are available.")?;
            Ok(())
        }
    }
}

pub(crate) async fn handle_select_primary_agent(
    ctx: &mut InteractionLoopContext<'_>,
    state: &mut InteractionState<'_>,
    name: Option<String>,
) -> Result<()> {
    let Some(name) = name else {
        let Some(specs) = load_primary_agent_specs_or_report(ctx).await? else {
            return Ok(());
        };
        let display_name = ctx
            .active_primary_agent
            .reset_to_default_from_specs(&specs)
            .display_name
            .clone();
        let is_plan_agent = ctx.active_primary_agent.active().name().eq_ignore_ascii_case("plan");
        if !is_plan_agent {
            leave_planning_for_execution(ctx).await?;
        }
        sync_primary_agent_runtime(ctx, state).await?;
        set_primary_agent_display(ctx, display_name);
        return Ok(());
    };

    let Some(specs) = load_primary_agent_specs_or_report(ctx).await? else {
        return Ok(());
    };

    // Check workspace trust BEFORE switching to avoid the switch-then-revert
    // flicker that corrupts the TUI display.  Auto-permission agents need
    // full-auto trust; if it is not already granted we block the switch
    // cleanly instead of mutating state and rolling back.
    if agent_needs_trust(&specs, &name) {
        auto_grant_tui_full_auto_workspace_trust(&ctx.config.workspace).await?;

        if !ensure_full_auto_workspace_trust(&ctx.config.workspace).await? {
            ctx.renderer
                .line(MessageStyle::Warning, "Workspace trust required for auto agent. Keeping previous agent.")?;
            return Ok(());
        }
    }

    let previous_primary_agent = ctx.active_primary_agent.active().name().to_string();
    match ctx.active_primary_agent.select_from_specs(&specs, &name) {
        Ok(active) => {
            let display_name = active.display_name.clone();
            let is_plan_agent = active.identity.name.eq_ignore_ascii_case("plan");
            let policy_overrides = active.tool_policy_overrides.clone();
            // Resolve and close the planning gate before any runtime
            // reconfiguration can fail, so switching away cannot leave a
            // dangling approval request.
            if !is_plan_agent {
                leave_planning_for_execution(ctx).await?;
            }
            // Apply per-agent tool policy overrides before refreshing the tool snapshot
            for (tool_name, policy) in &policy_overrides {
                if let Err(err) = ctx.tool_registry.set_tool_policy(tool_name, policy.clone()).await {
                    tracing::warn!("Failed to apply tool policy override for '{}' on agent switch: {}", tool_name, err);
                }
            }
            sync_primary_agent_runtime(ctx, state).await?;
            set_primary_agent_display(ctx, display_name);
            // Activating the plan agent also enters the planning workflow so
            // both "plan" concepts stay unified. No researching indicator
            // here: no request exists yet; the turn start renders it.
            if is_plan_agent && !ctx.tool_registry.is_planning_active() {
                transition_to_planning_workflow(
                    ctx.tool_registry,
                    ctx.session_stats,
                    ctx.plan_session,
                    ctx.handle,
                    PlanningEntrySource::AgentSelection,
                    Some(previous_primary_agent),
                    ctx.vt_cfg.as_ref().map(|cfg| cfg.default_primary_agent.clone()),
                    true,
                    true,
                )
                .await;
            }
        }
        Err(vtcode_core::primary_agent::PrimaryAgentResolutionError::UnknownAgent { requested }) => {
            ctx.renderer
                .line(MessageStyle::Error, &format!("Unknown primary agent '{requested}'."))?;
        }
    }

    Ok(())
}

async fn leave_planning_for_execution(ctx: &mut InteractionLoopContext<'_>) -> Result<()> {
    let removed = crate::agent::runloop::unified::planning_workflow::clear_stale_recovery_directives_for_execution(
        ctx.conversation_history,
    );
    if removed > 0 {
        tracing::info!(removed, "Cleared stale recovery directives during execution-agent switch");
    }

    if !ctx.tool_registry.is_planning_active() {
        return Ok(());
    }

    crate::agent::runloop::unified::planning_workflow::resolve_plan_approval(
        ctx.plan_session,
        ctx.harness_emitter,
        ctx.thread_id,
        ctx.thread_id,
        vtcode_core::exec::events::PlanApprovalDecision::Cancel,
        false,
    );
    crate::agent::runloop::unified::planning_workflow::finish_planning_workflow(
        ctx.tool_registry,
        ctx.plan_session,
        ctx.handle,
        PlanningFinishReason::Cancelled,
    )
    .await?;
    Ok(())
}

async fn sync_primary_agent_runtime(
    ctx: &mut InteractionLoopContext<'_>,
    state: &mut InteractionState<'_>,
) -> Result<()> {
    let mut runtime = PrimaryAgentRuntimeSyncContext {
        config: ctx.config,
        vt_cfg: ctx.vt_cfg.as_ref(),
        thread_id: ctx.thread_id,
        active_primary_agent: ctx.active_primary_agent.active(),
        lifecycle_hooks: ctx.lifecycle_hooks,
        async_mcp_manager: ctx.async_mcp_manager.as_ref(),
        tool_registry: ctx.tool_registry,
        tools: ctx.tools,
        tool_catalog: ctx.tool_catalog,
        mcp_catalog_initialized: state.mcp_catalog_initialized,
        pending_mcp_refresh: state.pending_mcp_refresh,
        provider_client: &**ctx.provider_client,
    };
    sync_primary_agent_runtime_context(&mut runtime).await
}

async fn load_primary_agent_specs_or_report(
    ctx: &mut InteractionLoopContext<'_>,
) -> Result<Option<Vec<vtcode_config::SubagentSpec>>> {
    match load_primary_agent_specs(ctx).await {
        Ok(specs) => Ok(Some(specs)),
        Err(err) => {
            ctx.renderer
                .line(MessageStyle::Error, &format!("Failed to discover primary agents: {err}"))?;
            Ok(None)
        }
    }
}

async fn load_primary_agent_specs(ctx: &InteractionLoopContext<'_>) -> Result<Vec<vtcode_config::SubagentSpec>> {
    load_primary_agent_specs_for_runtime(ctx.tool_registry, &ctx.config.workspace).await
}

fn set_primary_agent_display(ctx: &mut InteractionLoopContext<'_>, name: String) {
    let color = ctx.active_primary_agent.active().color.clone().filter(|c| !c.trim().is_empty());
    ctx.header_context.primary_agent = Some(name.clone());
    ctx.header_context.primary_agent_color = color.clone();
    ctx.handle.set_primary_agent(Some(name), color);
}

fn next_primary_agent_name(
    active: &vtcode_core::primary_agent::ActivePrimaryAgent,
    specs: &[vtcode_config::SubagentSpec],
) -> Option<String> {
    let names = primary_agent_names(specs);

    if names.is_empty() {
        return None;
    }

    Some(match names.iter().position(|name| name == &active.identity.name) {
        Some(index) if index + 1 < names.len() => names[index + 1].clone(),
        Some(_) | None => names[0].clone(),
    })
}

fn previous_primary_agent_name(
    active: &vtcode_core::primary_agent::ActivePrimaryAgent,
    specs: &[vtcode_config::SubagentSpec],
) -> Option<String> {
    let names = primary_agent_names(specs);

    if names.is_empty() {
        return None;
    }

    Some(match names.iter().position(|name| name == &active.identity.name) {
        Some(0) | None => names[names.len() - 1].clone(),
        Some(index) => names[index - 1].clone(),
    })
}

fn primary_agent_names(specs: &[vtcode_config::SubagentSpec]) -> Vec<String> {
    let mut names = specs
        .iter()
        .filter(|spec| spec.is_primary())
        .map(|spec| spec.name.trim())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    names.sort_by(|left, right| {
        primary_agent_cycle_rank(left)
            .cmp(&primary_agent_cycle_rank(right))
            .then_with(|| left.to_ascii_lowercase().cmp(&right.to_ascii_lowercase()))
    });
    names.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    names
}

fn primary_agent_cycle_rank(name: &str) -> u8 {
    match name.trim().to_ascii_lowercase().as_str() {
        "build" => 0,
        "duck" => 1,
        "plan" => 2,
        "auto" => 3,
        _ => 4,
    }
}

/// Returns `true` when the named agent requires workspace trust (i.e. it has
/// `PermissionDefault::Auto`).
fn agent_needs_trust(specs: &[vtcode_config::SubagentSpec], name: &str) -> bool {
    specs.iter().any(|s| {
        s.name.eq_ignore_ascii_case(name)
            && s.permissions.default == vtcode_config::core::permissions::PermissionDefault::Auto
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use vtcode_config::core::permissions::{AgentPermissionsConfig, PermissionDefault};
    use vtcode_config::{SubagentSource, SubagentSpec};
    use vtcode_core::tools::tool_intent::GENERIC_VERIFIER_DESCRIPTION;

    #[test]
    fn program_status_live_reload_projects_opt_in_and_disable() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let handle = vtcode_ui::tui::app::InlineHandle::new_for_tests(sender);
        let mut cfg = VTCodeConfig::default();
        let bootstrap = SessionBootstrap::default();
        for enabled in [true, false] {
            cfg.ui.program_status.enabled = enabled;
            apply_live_theme_and_appearance(&handle, &cfg, &bootstrap);
            let mut projections = Vec::new();
            while let Ok(command) = receiver.try_recv() {
                if let vtcode_ui::tui::app::InlineCommand::ProgramStatus(update) = command {
                    projections.push(update);
                }
            }
            assert_eq!(projections, vec![vtcode_commons::program_status::ProgramStatusUpdate::Configure { enabled }]);
        }
    }

    #[test]
    fn next_primary_agent_name_starts_with_first_sorted_agent() {
        let specs = vec![test_subagent_spec("beta"), test_subagent_spec("alpha")];

        assert_eq!(next_primary_agent_name(&default_active_primary_agent(), &specs), Some("alpha".to_string()));
    }

    #[test]
    fn next_primary_agent_name_cycles_to_next_sorted_agent() {
        let specs = vec![test_subagent_spec("beta"), test_subagent_spec("alpha")];
        let active = vtcode_core::primary_agent::ActivePrimaryAgent::from_spec(&specs[1]);

        assert_eq!(next_primary_agent_name(&active, &specs), Some("beta".to_string()));
    }

    #[test]
    fn next_primary_agent_name_cycles_last_agent_to_first() {
        let specs = vec![test_subagent_spec("build"), test_subagent_spec("duck")];
        let active = vtcode_core::primary_agent::ActivePrimaryAgent::from_spec(&specs[1]);

        assert_eq!(next_primary_agent_name(&active, &specs), Some("build".to_string()));
    }

    #[test]
    fn primary_agent_names_put_builtin_auto_last_before_custom_agents() {
        let specs = vec![
            test_subagent_spec("zeta"),
            test_subagent_spec("auto"),
            test_subagent_spec("plan"),
            test_subagent_spec("build"),
            test_subagent_spec("duck"),
            test_subagent_spec("alpha"),
        ];

        assert_eq!(
            primary_agent_names(&specs),
            vec![
                "build".to_string(),
                "duck".to_string(),
                "plan".to_string(),
                "auto".to_string(),
                "alpha".to_string(),
                "zeta".to_string(),
            ]
        );
    }

    #[test]
    fn previous_primary_agent_name_cycles_through_explicit_builtin_order() {
        let specs = vec![
            test_subagent_spec("auto"),
            test_subagent_spec("plan"),
            test_subagent_spec("build"),
            test_subagent_spec("duck"),
        ];
        let active = vtcode_core::primary_agent::ActivePrimaryAgent::from_spec(&specs[0]);

        assert_eq!(previous_primary_agent_name(&active, &specs), Some("plan".to_string()));
    }

    #[test]
    fn next_primary_agent_name_skips_non_primary_subagents() {
        let mut worker = test_subagent_spec("worker");
        worker.mode = vtcode_config::AgentMode::Subagent;
        let specs = vec![worker, test_subagent_spec("duck")];

        assert_eq!(next_primary_agent_name(&default_active_primary_agent(), &specs), Some("duck".to_string()));
    }

    #[test]
    fn auto_and_plan_remain_reachable_when_cycling_agents() {
        let mut auto = test_subagent_spec("auto");
        auto.permissions = AgentPermissionsConfig::new(PermissionDefault::Auto);
        let specs = vec![test_subagent_spec("build"), test_subagent_spec("plan"), auto];

        let build = vtcode_core::primary_agent::ActivePrimaryAgent::from_spec(&specs[0]);
        let plan = vtcode_core::primary_agent::ActivePrimaryAgent::from_spec(&specs[1]);

        assert_eq!(next_primary_agent_name(&build, &specs), Some("plan".to_string()));
        assert_eq!(next_primary_agent_name(&plan, &specs), Some("auto".to_string()));
        assert!(agent_needs_trust(&specs, "auto"));
    }

    fn default_active_primary_agent() -> vtcode_core::primary_agent::ActivePrimaryAgent {
        vtcode_core::primary_agent::ActivePrimaryAgentState::default().active().clone()
    }

    fn test_subagent_spec(name: &str) -> SubagentSpec {
        SubagentSpec {
            name: name.to_string(),
            description: String::new(),
            prompt: String::new(),
            tools: None,
            disallowed_tools: Vec::new(),
            model: None,
            color: None,
            reasoning_effort: None,
            permissions: AgentPermissionsConfig::new(PermissionDefault::Ask),
            skills: Vec::new(),
            mcp_servers: Vec::new(),
            hooks: None,
            background: false,
            mode: vtcode_config::AgentMode::Primary,
            max_turns: None,
            nickname_candidates: Vec::new(),
            initial_prompt: None,
            memory: None,
            isolation: None,
            aliases: Vec::new(),
            source: SubagentSource::ProjectVtcode,
            file_path: None,
            warnings: Vec::new(),
            tool_policy_overrides: BTreeMap::new(),
        }
    }

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
        assert_eq!(hint, Some(("task_tracker".to_string(), serde_json::json!({"action":"list"}))));
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
        assert_eq!(hint, Some(("task_tracker".to_string(), serde_json::json!({"action":"list"}))));
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
        assert_eq!(hint, Some(("task_tracker".to_string(), serde_json::json!({"action":"list"}))));
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
        assert_eq!(hint, Some(("task_tracker".to_string(), serde_json::json!({"action":"list"}))));
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

        let (tool_name, args) = extract_recent_follow_up_hint(&history).expect("continuation hint");
        assert_eq!(tool_name, tool_names::WRITE_STDIN, "continuation hints must use the public write_stdin tool");
        assert_ne!(tool_name, tool_names::UNIFIED_EXEC);
        assert_eq!(
            args,
            serde_json::json!({
                "session_id": "run-123",
                "chars": ""
            })
        );
        assert!(args.get("action").is_none());
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
    fn stalled_verification_resume_directive_names_verifier_and_keeps_completion_gated() {
        let directive = stalled_verification_resume_directive(Some("go test ./..."));

        assert!(directive.contains("Run `go test ./...` with `exec_command`"));
        assert!(directive.contains("resume the original request"));
        assert!(directive.contains("The turn cannot complete while the gate is pending"));
        assert!(directive.contains(VERIFIER_SHELL_FORM_NOTE));
    }

    #[test]
    fn stalled_verification_resume_directive_names_generic_verifier_without_project_marker() {
        let directive = stalled_verification_resume_directive(None);

        assert!(directive.contains(&format!("Run {GENERIC_VERIFIER_DESCRIPTION} with `exec_command`")));
        assert!(!directive.contains("Run `cargo check --locked`"), "no single command is presumed: {directive}");
        assert!(directive.contains("resume the original request"));
    }

    #[test]
    fn build_file_reference_metadata_maps_aliases_to_full_paths() {
        let temp_dir = TempDir::new().expect("temp dir");
        let file_path = temp_dir.path().join("src").join("main.rs");
        fs::create_dir_all(file_path.parent().expect("parent")).expect("mkdir");
        fs::write(&file_path, "fn main() {}\n").expect("write file");

        let metadata =
            build_file_reference_metadata("check @src/main.rs and continue", temp_dir.path()).expect("metadata");

        assert!(metadata.contains("@src/main.rs="));
        assert!(metadata.contains("src/main.rs"));
    }

    #[test]
    fn append_file_reference_metadata_keeps_ui_alias_but_adds_full_path_context() {
        let temp_dir = TempDir::new().expect("temp dir");
        let file_path = temp_dir.path().join("README.md");
        fs::write(&file_path, "# test\n").expect("write file");

        let mut augmented = uni::MessageContent::text("check @README.md".to_string());
        append_file_reference_metadata(&mut augmented, "check @README.md", temp_dir.path());

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
        let mut augmented = uni::MessageContent::text("use rust-engineer agent".to_string());
        append_agent_reference_metadata(&mut augmented, &[String::from("rust-engineer")]);

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
    fn append_submitted_attachments_leaves_text_only_content_unchanged() {
        let mut content = uni::MessageContent::text("plain prompt".to_string());

        append_submitted_attachments(&mut content, &[]);

        assert_eq!(content, uni::MessageContent::text("plain prompt".to_string()));
    }

    #[tokio::test]
    async fn parsed_at_image_stays_before_pasted_image() {
        let temp_dir = TempDir::new().expect("temp dir");
        let image_path = temp_dir.path().join("sample.png");
        fs::write(&image_path, tiny_png_bytes()).expect("write image");
        let mut content = vtcode_core::utils::at_pattern::parse_at_patterns_with_options(
            "look @sample.png",
            temp_dir.path(),
            vtcode_core::utils::at_pattern::AtPatternOptions::default(),
        )
        .await
        .expect("parse image");

        append_submitted_attachments(&mut content, &[UiContentPart::image("pasted-image", "image/png")]);

        assert_image_payload_order(&content, &["iVBOR", "pasted-image"]);
    }

    #[tokio::test]
    async fn data_image_stays_before_pasted_image() {
        let mut content = vtcode_core::utils::at_pattern::parse_at_patterns_with_options(
            "look data:image/png;base64,parsedimage",
            Path::new("."),
            vtcode_core::utils::at_pattern::AtPatternOptions::default(),
        )
        .await
        .expect("parse data image");

        append_submitted_attachments(&mut content, &[UiContentPart::image("pasted-image", "image/png")]);

        assert_image_payload_order(&content, &["parsedimage", "pasted-image"]);
    }

    #[test]
    fn clipboard_images_are_appended_after_refined_text_and_parsed_parts() {
        let mut content = uni::MessageContent::parts(vec![
            uni::ContentPart::text("refined prompt".to_string()),
            uni::ContentPart::image("parsed-image".to_string(), "image/png".to_string()),
        ]);

        append_submitted_attachments(&mut content, &[UiContentPart::image("pasted-image", "image/png")]);

        match content {
            uni::MessageContent::Parts(parts) => {
                assert!(matches!(
                    parts.as_slice(),
                    [
                        uni::ContentPart::Text { .. },
                        uni::ContentPart::Image { .. },
                        uni::ContentPart::Image { .. }
                    ]
                ));
                assert_eq!(
                    image_payloads(&uni::MessageContent::parts(parts)),
                    vec!["parsed-image".to_string(), "pasted-image".to_string()]
                );
            }
            uni::MessageContent::Text(_) => panic!("expected parts"),
        }
    }

    #[test]
    fn inline_image_placeholder_text_stays_before_pasted_image_part() {
        let mut content = uni::MessageContent::text("[Image #1] and here?".to_string());

        append_submitted_attachments(&mut content, &[UiContentPart::image("pasted-image", "image/png")]);

        match content {
            uni::MessageContent::Parts(parts) => {
                assert!(matches!(parts.as_slice(), [uni::ContentPart::Text { .. }, uni::ContentPart::Image { .. }]));
                assert_eq!(parts[0].as_text(), Some("[Image #1] and here?"));
                assert_eq!(image_payloads(&uni::MessageContent::parts(parts)), vec!["pasted-image".to_string()]);
            }
            uni::MessageContent::Text(_) => panic!("expected parts"),
        }
    }

    #[test]
    fn metadata_remains_trailing_after_all_image_parts() {
        let temp_dir = TempDir::new().expect("temp dir");
        let file_path = temp_dir.path().join("README.md");
        fs::write(&file_path, "# test\n").expect("write file");
        let mut content = uni::MessageContent::parts(vec![
            uni::ContentPart::text("check @README.md".to_string()),
            uni::ContentPart::image("parsed-image".to_string(), "image/png".to_string()),
        ]);
        append_submitted_attachments(&mut content, &[UiContentPart::image("pasted-image", "image/png")]);

        append_file_reference_metadata(&mut content, "check @README.md", temp_dir.path());
        append_agent_reference_metadata(&mut content, &[String::from("rust-engineer")]);

        match content {
            uni::MessageContent::Parts(parts) => {
                assert!(matches!(parts[1], uni::ContentPart::Image { .. }));
                assert!(matches!(parts[2], uni::ContentPart::Image { .. }));
                assert!(matches!(parts[3], uni::ContentPart::Text { .. }));
                assert!(matches!(parts[4], uni::ContentPart::Text { .. }));
                assert!(parts[3].as_text().unwrap_or("").contains("[file_reference_metadata]"));
                assert!(parts[4].as_text().unwrap_or("").contains("[agent_reference_metadata]"));
            }
            uni::MessageContent::Text(_) => panic!("expected parts"),
        }
    }

    #[test]
    fn unsupported_model_rejects_submitted_image_attachments() {
        let temp_dir = TempDir::new().expect("temp dir");
        let input = SubmittedInput::new("please inspect this", vec![UiContentPart::image("pasted-image", "image/png")]);

        assert!(submitted_images_are_unsupported(&input, false, temp_dir.path()));
        assert!(!submitted_images_are_unsupported(&input, true, temp_dir.path()));
        assert_eq!(input.text, "please inspect this");
        assert_eq!(input.attachments, vec![UiContentPart::image("pasted-image", "image/png")]);
    }

    #[test]
    fn unsupported_model_rejects_text_data_image() {
        let temp_dir = TempDir::new().expect("temp dir");
        let input = SubmittedInput::from("look data:image/png;base64,parsedimage".to_string());

        assert!(submitted_images_are_unsupported(&input, false, temp_dir.path()));
        assert!(!submitted_images_are_unsupported(&input, true, temp_dir.path()));
        assert_eq!(input.text, "look data:image/png;base64,parsedimage");
        assert!(input.attachments.is_empty());
    }

    #[test]
    fn selected_model_image_support_uses_image_modality_metadata() {
        assert!(selected_model_supports_image_input("openai", "gpt-5.6-sol", false));
    }

    #[test]
    fn selected_model_image_support_uses_alias_modality_metadata() {
        // `gpt-5.6` is the documented family alias routing to `gpt-5.6-sol`;
        // modality metadata must come from the aliased model.
        assert!(selected_model_supports_image_input("openai", "gpt-5.6", false));
    }

    #[test]
    fn selected_model_image_support_accepts_chatgpt_provider_label() {
        assert!(selected_model_supports_image_input("OpenAI (ChatGPT)", "gpt-5.6-sol", false));
    }

    #[test]
    fn selected_model_image_support_accepts_display_model_label() {
        // Picker-style labels may carry a context suffix; the stripped prefix
        // must still resolve to the model's modality metadata.
        assert!(selected_model_supports_image_input("OpenAI (ChatGPT)", "gpt-5.6-sol (128K)", false));
    }

    #[test]
    fn selected_model_image_support_rejects_text_only_metadata() {
        assert!(!selected_model_supports_image_input("openai", "gpt-oss-20b", true));
    }

    #[test]
    fn selected_model_image_support_falls_back_without_metadata() {
        assert!(selected_model_supports_image_input("openai", "custom-vision-model", true));
        assert!(!selected_model_supports_image_input("custom-provider", "gpt-5.6-sol", false));
    }

    #[test]
    fn unsupported_model_rejects_text_at_image_path() {
        let temp_dir = TempDir::new().expect("temp dir");
        let image_path = temp_dir.path().join("sample.png");
        fs::write(&image_path, tiny_png_bytes()).expect("write image");
        let input = SubmittedInput::from("look @sample.png".to_string());

        assert!(submitted_images_are_unsupported(&input, false, temp_dir.path()));
        assert!(input.attachments.is_empty());
    }

    #[test]
    fn unsupported_model_rejects_text_raw_image_path() {
        let temp_dir = TempDir::new().expect("temp dir");
        let image_path = temp_dir.path().join("raw.png");
        fs::write(&image_path, tiny_png_bytes()).expect("write image");
        let input = SubmittedInput::from(format!("look {}", image_path.display()));

        assert!(submitted_images_are_unsupported(&input, false, temp_dir.path()));
        assert!(input.attachments.is_empty());
    }

    #[test]
    fn unsupported_model_rejects_text_image_url() {
        let temp_dir = TempDir::new().expect("temp dir");
        let input = SubmittedInput::from("look @https://example.com/sample.png?cache=1".to_string());

        assert!(submitted_images_are_unsupported(&input, false, temp_dir.path()));
        assert!(input.attachments.is_empty());
    }

    #[test]
    fn update_input_text_preserves_attachments_for_submit_time_gating() {
        let temp_dir = TempDir::new().expect("temp dir");
        let image = UiContentPart::image("pasted-image", "image/png");
        let mut input = SubmittedInput::new("/compact please inspect this", vec![image.clone()]);

        replace_submitted_input_text(&mut input, "please inspect this".to_string());

        assert_eq!(input.text, "please inspect this");
        assert_eq!(input.attachments, vec![image]);
        assert!(submitted_images_are_unsupported(&input, false, temp_dir.path()));
    }

    fn tiny_png_bytes() -> &'static [u8] {
        &[
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 31,
            21, 196, 137, 0, 0, 0, 10, 73, 68, 65, 84, 120, 156, 99, 0, 1, 0, 0, 5, 0, 1, 13, 10, 45, 180, 0, 0, 0, 0,
            73, 69, 78, 68, 174, 66, 96, 130,
        ]
    }

    fn image_payloads(content: &uni::MessageContent) -> Vec<String> {
        let uni::MessageContent::Parts(parts) = content else {
            return Vec::new();
        };
        parts
            .iter()
            .filter_map(|part| match part {
                uni::ContentPart::Image { data, .. } => Some(data.clone()),
                _ => None,
            })
            .collect()
    }

    fn assert_image_payload_order(content: &uni::MessageContent, expected_prefixes: &[&str]) {
        let payloads = image_payloads(content);
        assert_eq!(payloads.len(), expected_prefixes.len());
        for (payload, expected_prefix) in payloads.iter().zip(expected_prefixes) {
            assert!(payload.starts_with(expected_prefix), "payload {payload:?} should start with {expected_prefix:?}");
        }
    }

    #[test]
    fn test_stalled_follow_up_recovery_prompt_sanitizes_disabled_tools_message() {
        let reason = "Recovery tool-call limit reached after 4 blocked calls. Tools remain disabled while the recovery response is finalized.";
        let prompt = stalled_follow_up_recovery_prompt(reason, false);
        assert!(!prompt.contains("Tools remain disabled"));
        assert!(prompt.contains("Tools are fully enabled for this turn."));
        assert!(prompt.contains("Recovery tool-call limit reached after 4 blocked calls."));
    }
}
