use crate::agent::runloop::ui::build_inline_header_context;
use crate::agent::runloop::unified::session_setup::ide_context::{
    IdeContextBridge, status_line_editor_label, tui_header_summary,
};
use crate::agent::runloop::unified::{context_manager, palettes};
use crate::agent::runloop::welcome::SessionBootstrap;
use anyhow::Result;
use tracing::warn;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_tui::app::{InlineHandle, InlineHeaderContext};

use super::persistent_memory::{
    apply_persistent_memory_header_guide, load_persistent_memory_status,
};

pub(super) struct HeaderContextInit<'a> {
    pub(super) config: &'a CoreAgentConfig,
    pub(super) vt_cfg: Option<&'a VTCodeConfig>,
    pub(super) session_bootstrap: &'a SessionBootstrap,
    pub(super) provider_client: &'a dyn uni::LLMProvider,
    pub(super) header_provider_label: String,
    pub(super) full_auto: bool,
}

pub(super) async fn initialize_header_context(
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    context_manager: &mut context_manager::ContextManager,
    ide_context_bridge: &mut Option<IdeContextBridge>,
    init: HeaderContextInit<'_>,
) -> Result<InlineHeaderContext> {
    let HeaderContextInit {
        config,
        vt_cfg,
        session_bootstrap,
        provider_client,
        header_provider_label,
        full_auto,
    } = init;

    let persistent_memory_status = load_persistent_memory_status(config, vt_cfg);
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

    handle.set_theme(vtcode_core::ui::inline_theme_from_core_styles(
        &vtcode_core::ui::theme::active_styles(),
    ));
    palettes::apply_prompt_style(handle);

    let reasoning_label = vt_cfg
        .as_ref()
        .map(|cfg| cfg.agent.reasoning_effort.as_str().to_string())
        .unwrap_or_else(|| config.reasoning_effort.as_str().to_string());

    let mode_label = match (config.ui_surface, full_auto) {
        (vtcode_core::config::types::UiSurfacePreference::Inline, true) => "auto".to_string(),
        (vtcode_core::config::types::UiSurfacePreference::Inline, false) => "inline".to_string(),
        (vtcode_core::config::types::UiSurfacePreference::Alternate, _) => "alt".to_string(),
        (vtcode_core::config::types::UiSurfacePreference::Auto, true) => "auto".to_string(),
        (vtcode_core::config::types::UiSurfacePreference::Auto, false) => "std".to_string(),
    };

    let mut header_context = build_inline_header_context(
        config,
        vt_cfg,
        session_bootstrap,
        header_provider_label,
        config.model.clone(),
        provider_client.effective_context_size(&config.model),
        mode_label,
        reasoning_label,
    )
    .await?;
    if let Some(memory_status) = persistent_memory_status.as_ref() {
        apply_persistent_memory_header_guide(&mut header_context, memory_status);
    }

    let initial_editor_snapshot = if let Some(bridge) = ide_context_bridge.as_mut() {
        match bridge.refresh() {
            Ok((snapshot, _)) => snapshot,
            Err(err) => {
                warn!("Failed to refresh IDE context snapshot: {}", err);
                None
            }
        }
    } else {
        None
    };
    apply_ide_context_snapshot(
        context_manager,
        &mut header_context,
        handle,
        config.workspace.as_path(),
        vt_cfg,
        initial_editor_snapshot,
    );

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
    let storage_mode = vt_cfg
        .map(|cfg| cfg.agent.credential_storage_mode)
        .unwrap_or_default();
    let api_key = vtcode_core::config::api_keys::get_api_key(
        "openai",
        &vtcode_core::config::api_keys::ApiKeySources::default(),
    )
    .ok();
    let overview =
        vtcode_config::auth::summarize_openai_credentials(auth_cfg, storage_mode, api_key)?;
    let Some(notice) = overview.notice.as_deref() else {
        return Ok(());
    };

    renderer.line(MessageStyle::Info, notice)?;
    if let Some(recommendation) = overview.recommendation.as_deref() {
        renderer.line(MessageStyle::Output, recommendation)?;
    }
    Ok(())
}

pub(crate) fn apply_ide_context_snapshot(
    context_manager: &mut context_manager::ContextManager,
    header_context: &mut InlineHeaderContext,
    handle: &InlineHandle,
    workspace: &std::path::Path,
    vt_cfg: Option<&VTCodeConfig>,
    snapshot: Option<vtcode_core::EditorContextSnapshot>,
) {
    let ide_context_config = vt_cfg.map(|cfg| &cfg.ide_context);
    context_manager.set_editor_context_snapshot(snapshot.clone(), ide_context_config);
    let effective_ide_context_config =
        context_manager.effective_ide_context_config_with_base(ide_context_config);
    header_context.editor_context = tui_header_summary(
        workspace,
        Some(&effective_ide_context_config),
        snapshot.as_ref(),
    );
    handle.set_header_context(header_context.clone());
}

pub(crate) fn ide_context_status_label(
    context_manager: &context_manager::ContextManager,
    workspace: &std::path::Path,
    vt_cfg: Option<&VTCodeConfig>,
    snapshot: Option<&vtcode_core::EditorContextSnapshot>,
    source: Option<&std::path::Path>,
) -> Option<String> {
    let effective_ide_context_config =
        context_manager.effective_ide_context_config_with_base(vt_cfg.map(|cfg| &cfg.ide_context));
    status_line_editor_label(
        workspace,
        Some(&effective_ide_context_config),
        snapshot,
        source,
    )
}

pub(crate) fn ide_context_status_label_from_bridge(
    context_manager: &context_manager::ContextManager,
    workspace: &std::path::Path,
    vt_cfg: Option<&VTCodeConfig>,
    ide_context_bridge: Option<&IdeContextBridge>,
) -> Option<String> {
    ide_context_bridge.and_then(|bridge| {
        ide_context_status_label(
            context_manager,
            workspace,
            vt_cfg,
            bridge.snapshot(),
            bridge.snapshot_source(),
        )
    })
}
