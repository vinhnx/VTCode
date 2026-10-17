use crate::agent::runloop::ui::build_inline_header_context;
use crate::agent::runloop::unified::palettes;
use crate::agent::runloop::welcome::SessionBootstrap;
use anyhow::Result;
use tracing::warn;
use vtcode_core::config::api_keys::{ApiKeySources, get_api_key_with_mode};
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_ui::tui::app::{InlineHandle, InlineHeaderContext};

use super::persistent_memory::{apply_persistent_memory_header_guide, load_persistent_memory_status};

pub(super) struct HeaderContextInit<'a> {
    pub(super) config: &'a CoreAgentConfig,
    pub(super) vt_cfg: Option<&'a VTCodeConfig>,
    pub(super) session_bootstrap: &'a SessionBootstrap,
    pub(super) provider_client: &'a dyn uni::LLMProvider,
    pub(super) header_provider_label: String,
}

pub(super) async fn initialize_header_context(
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    init: HeaderContextInit<'_>,
) -> Result<InlineHeaderContext> {
    let HeaderContextInit {
        config,
        vt_cfg,
        session_bootstrap,
        provider_client,
        header_provider_label,
    } = init;

    let persistent_memory_status = load_persistent_memory_status(config, vt_cfg).await;
    if let Err(err) = persistent_memory_status.as_ref() {
        warn!(
            workspace = %config.workspace.display(),
            error = ?err,
            "Failed to load persistent memory status for TUI guide"
        );
        renderer.line(
            MessageStyle::Warning,
            "Persistent memory is enabled, but VT Code couldn't load the TUI memory guide.",
        )?;
    }
    let persistent_memory_status = persistent_memory_status.ok().flatten();

    if let Some(notice) = session_bootstrap.search_tools_notice.as_ref() {
        notice.render(renderer)?;
    }
    maybe_render_openai_priority_notice(renderer, config, vt_cfg)?;
    // System-prompt budget warning is rendered after session hydration via
    // `apply_post_hydration_ui`, when the composed report is available.

    handle.set_theme(vtcode_core::ui::inline_theme_from_core_styles(&vtcode_core::ui::theme::active_styles()));
    palettes::apply_prompt_style(handle);

    let reasoning_label = vt_cfg
        .as_ref()
        .map(|cfg| cfg.agent.reasoning_effort.as_str().to_string())
        .unwrap_or_else(|| config.reasoning_effort.as_str().to_string());

    let mut header_context = build_inline_header_context(
        config,
        vt_cfg,
        session_bootstrap,
        header_provider_label,
        config.model.clone(),
        provider_client.effective_context_size(&config.model),
        reasoning_label,
    )
    .await?;
    if let Some(memory_status) = persistent_memory_status.as_ref() {
        apply_persistent_memory_header_guide(&mut header_context, memory_status);
    }

    // Push initial context so the compact header shows provider/model on
    // first paint. `SetHeaderContext` preserves the TUI primary agent.
    handle.set_header_context(header_context.clone());

    Ok(header_context)
}

fn maybe_render_openai_priority_notice(
    renderer: &mut AnsiRenderer,
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
) -> Result<()> {
    if !config.provider.eq_ignore_ascii_case("openai") {
        return Ok(());
    }

    let default_auth = vtcode_auth::OpenAIAuthConfig::default();
    let auth_cfg = vt_cfg.map(|cfg| &cfg.auth.openai).unwrap_or(&default_auth);
    let storage_mode = vt_cfg.map(|cfg| cfg.agent.credential_storage_mode).unwrap_or_default();
    let api_key = get_api_key_with_mode("openai", &ApiKeySources::default(), storage_mode).ok();
    let overview = vtcode_config::auth::summarize_openai_credentials(auth_cfg, storage_mode, api_key)?;
    let Some(notice) = overview.notice.as_deref() else {
        return Ok(());
    };

    renderer.line(MessageStyle::Info, notice)?;
    if let Some(recommendation) = overview.recommendation.as_deref() {
        renderer.line(MessageStyle::Output, recommendation)?;
    }
    Ok(())
}

/// Render a one-time session-start warning when the composed system prompt
/// exceeded its configured token budget. Mirrors the headless path's warning
/// (pushed into `runtime.state.warnings` in `task_setup.rs`) so interactive
/// and headless sessions surface the same signal.
pub(crate) fn maybe_render_system_prompt_budget_warning(
    renderer: &mut AnsiRenderer,
    vt_cfg: Option<&VTCodeConfig>,
    session_bootstrap: &SessionBootstrap,
) -> Result<()> {
    let report = &session_bootstrap.system_prompt_report;
    if !report.over_budget {
        return Ok(());
    }

    let warning_enabled = vt_cfg.map(|cfg| cfg.agent.system_prompt_budget_warning).unwrap_or(true);
    if !warning_enabled {
        return Ok(());
    }

    let max_tokens = vt_cfg
        .map(|cfg| cfg.agent.max_system_prompt_tokens)
        .unwrap_or(vtcode_core::config::constants::prompt_budget::DEFAULT_MAX_SYSTEM_PROMPT_TOKENS);

    renderer.line(
        MessageStyle::Warning,
        &format!(
            "Base system prompt is ~{} tokens (budget {}); later appendices (session context, runtime line, subagents roster) add more. Consider a leaner system prompt mode or enable agent.trim_system_prompt.",
            report.token_estimate, max_tokens
        ),
    )
}
