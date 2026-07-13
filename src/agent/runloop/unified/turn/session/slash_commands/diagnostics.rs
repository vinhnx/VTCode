use anyhow::{Context, Result};
use vtcode_config::loader::ConfigManager;
use vtcode_core::config::ToolPolicy;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_ui::tui::app::{InlineListItem, InlineListSelection};

use crate::agent::runloop::unified::diagnostics::{
    CheckupOptions, count_configured_hooks, run_checkup_diagnostics,
};
use crate::agent::runloop::unified::ui_interaction::display_session_status;

use super::{SlashCommandContext, SlashCommandControl};

#[path = "diagnostics/memory.rs"]
pub(super) mod memory;

const CHECKUP_ACTION_PREFIX: &str = "checkup.action.";
const CHECKUP_ACTION_BACK: &str = "checkup.action.back";
const CHECKUP_ACTION_OPTIMIZE_PREFIX: &str = "checkup.optimize.";

/// Identifies an applicable `/checkup` optimization the user can apply.
///
/// These are all reversible config mutations; `/checkup` confirms with the user
/// (via the selection modal) before mutating anything.
#[derive(Debug)]
struct CheckupRemediation {
    id: &'static str,
    title: String,
    subtitle: String,
    search_value: String,
}

/// Compute the optimizations that are currently applicable for the active config.
fn compute_checkup_remediations(vt_cfg: &Option<VTCodeConfig>) -> Vec<CheckupRemediation> {
    let Some(cfg) = vt_cfg else {
        return Vec::new();
    };
    let mut items = Vec::new();

    if !cfg.automation.full_auto.enabled {
        items.push(CheckupRemediation {
            id: "enable_auto_mode",
            title: "Enable auto mode".to_string(),
            subtitle: "Let VT Code run unattended ([automation.full_auto]).".to_string(),
            search_value: "optimize auto mode unattended full_auto".to_string(),
        });
    }

    if count_configured_hooks(&cfg.hooks.lifecycle) > 0 {
        items.push(CheckupRemediation {
            id: "disable_slow_hooks",
            title: "Disable lifecycle hooks".to_string(),
            subtitle: "Turn off configured hooks that can slow down runs.".to_string(),
            search_value: "optimize hooks slow disable".to_string(),
        });
    }

    if cfg.tools.default_policy != ToolPolicy::Allow {
        items.push(CheckupRemediation {
            id: "preapprove_readonly",
            title: "Pre-approve tool use".to_string(),
            subtitle: "Set tool policy to 'allow' so read-only commands stop prompting."
                .to_string(),
            search_value: "optimize tool policy allow preapprove prompt".to_string(),
        });
    }

    items
}

/// Apply a single optimization to the given config in place.
///
/// Pure and reversible: it only mutates the provided `VTCodeConfig` and returns a
/// human-readable description of what changed. Persistence is the caller's job.
fn apply_checkup_remediation_to_config(id: &str, cfg: &mut VTCodeConfig) -> Result<String> {
    match id {
        "enable_auto_mode" => {
            if cfg.automation.full_auto.enabled {
                return Ok("Auto mode is already enabled.".to_string());
            }
            cfg.automation.full_auto.enabled = true;
            Ok("Enabled auto mode ([automation.full_auto]).".to_string())
        }
        "disable_slow_hooks" => {
            let removed = count_configured_hooks(&cfg.hooks.lifecycle);
            if removed == 0 {
                return Ok("No lifecycle hooks are configured.".to_string());
            }
            let lifecycle = &mut cfg.hooks.lifecycle;
            lifecycle.session_start.clear();
            lifecycle.session_end.clear();
            lifecycle.subagent_start.clear();
            lifecycle.subagent_stop.clear();
            lifecycle.user_prompt_submit.clear();
            lifecycle.pre_tool_use.clear();
            lifecycle.post_tool_use.clear();
            lifecycle.permission_request.clear();
            lifecycle.pre_compact.clear();
            lifecycle.stop.clear();
            lifecycle.task_completion.clear();
            lifecycle.task_completed.clear();
            lifecycle.notification.clear();
            Ok(format!("Disabled {removed} lifecycle hook group(s)."))
        }
        "preapprove_readonly" => {
            if cfg.tools.default_policy == ToolPolicy::Allow {
                return Ok("Tool policy is already 'allow'.".to_string());
            }
            cfg.tools.default_policy = ToolPolicy::Allow;
            Ok("Set tool policy to 'allow' (read-only commands are pre-approved).".to_string())
        }
        other => anyhow::bail!("Unknown checkup optimization: {other}"),
    }
}

pub(crate) async fn handle_show_status(
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    let tool_count = ctx.tools.read().await.len();
    let active_instruction_directory = ctx
        .context_manager
        .active_instruction_directory_snapshot()
        .unwrap_or_else(|| ctx.config.workspace.clone());
    let instruction_context_paths = ctx.context_manager.instruction_context_paths_snapshot();
    display_session_status(
        ctx.renderer,
        crate::agent::runloop::unified::ui_interaction::SessionStatusContext {
            config: ctx.config,
            vt_cfg: ctx.vt_cfg.as_ref(),
            active_instruction_directory: &active_instruction_directory,
            instruction_context_paths: &instruction_context_paths,
            message_count: ctx.conversation_history.len(),
            stats: ctx.session_stats,
            available_tools: tool_count,
            async_mcp_manager: ctx.async_mcp_manager.map(|manager| manager.as_ref()),
            loaded_skills: ctx.loaded_skills,
        },
    )
    .await?;
    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_show_memory(
    mut ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    if !ctx.renderer.supports_inline_ui() {
        memory::render_memory_status_lines(&mut ctx, false).await?;
        ctx.renderer.line(
            MessageStyle::Info,
            "Next actions: `/memory` in inline UI, `/config memory`, or `/edit <target>`.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    if !super::ui::ensure_selection_ui_available(&mut ctx, "opening memory controls")? {
        return Ok(SlashCommandControl::Continue);
    }

    memory::run_memory_modal(&mut ctx, false).await
}

pub(crate) async fn handle_show_memory_config(
    mut ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    if !ctx.renderer.supports_inline_ui() {
        memory::render_memory_config_lines(&mut ctx).await?;
        ctx.renderer.line(
            MessageStyle::Info,
            "Use `/memory` in inline UI for quick actions or `/config agent.persistent_memory` for the raw section.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    if !super::ui::ensure_selection_ui_available(&mut ctx, "opening memory settings")? {
        return Ok(SlashCommandControl::Continue);
    }

    memory::run_memory_modal(&mut ctx, true).await
}

pub(crate) async fn handle_run_checkup(
    mut ctx: SlashCommandContext<'_>,
    quick: bool,
) -> Result<SlashCommandControl> {
    run_checkup(&mut ctx, quick).await?;
    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_start_checkup_interactive(
    mut ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    if !ctx.renderer.supports_inline_ui() {
        run_checkup(&mut ctx, false).await?;
        return Ok(SlashCommandControl::Continue);
    }

    if !super::ui::ensure_selection_ui_available(&mut ctx, "opening checkup")? {
        return Ok(SlashCommandControl::Continue);
    }

    show_checkup_actions_modal(&mut ctx);
    let Some(selection) = super::ui::wait_for_list_modal_selection(&mut ctx).await else {
        return Ok(SlashCommandControl::Continue);
    };

    let InlineListSelection::ConfigAction(action) = selection else {
        return Ok(SlashCommandControl::Continue);
    };

    if action == CHECKUP_ACTION_BACK {
        return Ok(SlashCommandControl::Continue);
    }

    if let Some(opt_id) = action.strip_prefix(CHECKUP_ACTION_OPTIMIZE_PREFIX) {
        return handle_apply_checkup_optimization(ctx, opt_id.to_string()).await;
    }

    let Some(action_key) = action.strip_prefix(CHECKUP_ACTION_PREFIX) else {
        return Ok(SlashCommandControl::Continue);
    };
    match action_key {
        "quick" => run_checkup(&mut ctx, true).await?,
        "full" => run_checkup(&mut ctx, false).await?,
        _ => {}
    }

    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_apply_checkup_optimization(
    mut ctx: SlashCommandContext<'_>,
    id: String,
) -> Result<SlashCommandControl> {
    let mut manager = ConfigManager::load_from_workspace(&ctx.config.workspace)
        .context("Failed to load the workspace config manager")?;
    let mut cfg = ctx
        .vt_cfg
        .clone()
        .context("No active VT Code configuration to optimize")?;

    let description = apply_checkup_remediation_to_config(&id, &mut cfg)?;
    manager.save_config(&cfg)?;
    *ctx.vt_cfg = Some(cfg);

    ctx.renderer.line(
        MessageStyle::Status,
        &format!("[OK] {description} Configuration saved."),
    )?;
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;

    // Re-run the checkup so the updated state is visible immediately.
    run_checkup(&mut ctx, false).await?;
    Ok(SlashCommandControl::Continue)
}

async fn run_checkup(ctx: &mut SlashCommandContext<'_>, quick: bool) -> Result<()> {
    let provider_runtime = ctx.provider_client.name().to_string();
    run_checkup_diagnostics(
        ctx.renderer,
        ctx.config,
        ctx.vt_cfg.as_ref(),
        &provider_runtime,
        ctx.async_mcp_manager.map(|m| m.as_ref()),
        ctx.linked_directories,
        Some(ctx.loaded_skills),
        CheckupOptions { quick },
    )
    .await?;
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(())
}

pub(crate) async fn handle_start_terminal_setup(
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    let vt_cfg = ctx
        .vt_cfg
        .as_ref()
        .context("VT Code configuration not available")?;
    vtcode_core::terminal_setup::run_terminal_setup_wizard(ctx.renderer, vt_cfg).await?;
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

fn show_checkup_actions_modal(ctx: &mut SlashCommandContext<'_>) {
    let mut items = vec![
        InlineListItem {
            title: "Run full checkup".to_string(),
            subtitle: Some(
                "Run all checks: config, provider key, dependencies, MCP, links, and skills"
                    .to_string(),
            ),
            badge: Some("Recommended".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{CHECKUP_ACTION_PREFIX}full"
            ))),
            search_value: Some("checkup full all checks mcp dependencies".to_string()),
        },
        InlineListItem {
            title: "Run quick checkup".to_string(),
            subtitle: Some(
                "Run core checks only (skips dependencies, MCP, links, and skills)".to_string(),
            ),
            badge: Some("Fast".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{CHECKUP_ACTION_PREFIX}quick"
            ))),
            search_value: Some("checkup quick fast checks".to_string()),
        },
        InlineListItem {
            title: "Back".to_string(),
            subtitle: Some("Close without running the checkup".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(
                CHECKUP_ACTION_BACK.to_string(),
            )),
            search_value: Some("back close cancel".to_string()),
        },
    ];

    for remediation in compute_checkup_remediations(ctx.vt_cfg) {
        items.push(InlineListItem {
            title: remediation.title,
            subtitle: Some(remediation.subtitle),
            badge: Some("Optimization".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{CHECKUP_ACTION_OPTIMIZE_PREFIX}{}",
                remediation.id
            ))),
            search_value: Some(remediation.search_value),
        });
    }

    ctx.renderer.show_list_modal(
        "Checkup",
        vec![
            "Choose how to run the VT Code checkup.".to_string(),
            "Use Enter to run an action, Esc to close.".to_string(),
        ],
        items,
        Some(InlineListSelection::ConfigAction(format!(
            "{CHECKUP_ACTION_PREFIX}full"
        ))),
        None,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with_hooks() -> VTCodeConfig {
        let mut cfg = VTCodeConfig::default();
        cfg.hooks.lifecycle.pre_tool_use.push(Default::default());
        cfg
    }

    #[test]
    fn no_remediations_when_config_is_already_optimized() {
        let mut cfg = VTCodeConfig::default();
        cfg.automation.full_auto.enabled = true;
        cfg.tools.default_policy = ToolPolicy::Allow;
        // lifecycle defaults to empty
        let items = compute_checkup_remediations(&Some(cfg));
        assert!(items.is_empty(), "unexpected: {items:?}");
    }

    #[test]
    fn none_config_yields_no_remediations() {
        assert!(compute_checkup_remediations(&None).is_empty());
    }

    #[test]
    fn reports_all_three_when_unoptimized() {
        let cfg = cfg_with_hooks();
        let ids: Vec<&str> = compute_checkup_remediations(&Some(cfg))
            .iter()
            .map(|r| r.id)
            .collect();
        assert!(ids.contains(&"enable_auto_mode"));
        assert!(ids.contains(&"disable_slow_hooks"));
        assert!(ids.contains(&"preapprove_readonly"));
    }

    #[test]
    fn enable_auto_mode_mutates_config() {
        let mut cfg = VTCodeConfig::default();
        assert!(!cfg.automation.full_auto.enabled);
        let msg = apply_checkup_remediation_to_config("enable_auto_mode", &mut cfg).unwrap();
        assert!(cfg.automation.full_auto.enabled);
        assert!(msg.contains("Enabled"));
    }

    #[test]
    fn disable_slow_hooks_clears_all_vectors() {
        let mut cfg = cfg_with_hooks();
        let msg = apply_checkup_remediation_to_config("disable_slow_hooks", &mut cfg).unwrap();
        assert_eq!(count_configured_hooks(&cfg.hooks.lifecycle), 0);
        assert!(msg.contains("Disabled"));
    }

    #[test]
    fn preapprove_readonly_sets_allow_policy() {
        let mut cfg = VTCodeConfig::default();
        cfg.tools.default_policy = ToolPolicy::Prompt;
        let msg = apply_checkup_remediation_to_config("preapprove_readonly", &mut cfg).unwrap();
        assert_eq!(cfg.tools.default_policy, ToolPolicy::Allow);
        assert!(msg.contains("allow"));
    }

    #[test]
    fn unknown_remediation_id_errors() {
        let mut cfg = VTCodeConfig::default();
        assert!(apply_checkup_remediation_to_config("bogus", &mut cfg).is_err());
    }
}
