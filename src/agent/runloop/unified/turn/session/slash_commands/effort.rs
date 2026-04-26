use std::str::FromStr;

use anyhow::Result;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::models::Provider;
use vtcode_core::config::types::ReasoningEffortLevel;
use vtcode_core::models_manager::{ModelPreset, builtin_model_presets};
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_tui::app::{InlineListItem, InlineListSelection};

use crate::agent::runloop::ui::build_inline_header_context;

use super::{SlashCommandContext, SlashCommandControl};

pub(crate) async fn handle_set_effort(
    mut ctx: SlashCommandContext<'_>,
    level: Option<ReasoningEffortLevel>,
    persist: bool,
) -> Result<SlashCommandControl> {
    let supported = supported_effort_levels(&ctx);
    if supported.is_empty() {
        ctx.renderer.line(
            MessageStyle::Info,
            &format!(
                "The current model '{}' does not support configurable effort.",
                ctx.config.model
            ),
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    let chosen = if let Some(level) = level {
        if !supported.contains(&level) {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!(
                    "Effort level '{}' is not supported by '{}'.",
                    level.as_str(),
                    ctx.config.model
                ),
            )?;
            ctx.renderer.line(
                MessageStyle::Info,
                &format!(
                    "Supported levels: {}.",
                    supported
                        .iter()
                        .map(|value| value.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )?;
            return Ok(SlashCommandControl::Continue);
        }
        level
    } else {
        if !ctx.renderer.supports_inline_ui() {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!(
                    "Current effort level: {}. Supported levels for '{}': {}.",
                    ctx.config.reasoning_effort.as_str(),
                    ctx.config.model,
                    supported
                        .iter()
                        .map(|value| value.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )?;
            ctx.renderer.line(
                MessageStyle::Info,
                "Usage: /effort [--persist] [none|minimal|low|medium|high|xhigh|max]",
            )?;
            return Ok(SlashCommandControl::Continue);
        }

        if !super::ui::ensure_selection_ui_available(&mut ctx, "choosing an effort level")? {
            return Ok(SlashCommandControl::Continue);
        }

        ctx.handle.show_list_modal(
            "Effort level".to_string(),
            vec![
                format!("Set effort for '{}' in this conversation.", ctx.config.model),
                if persist {
                    "The selected level will also be written to vtcode.toml.".to_string()
                } else {
                    "The selected level only affects the active conversation unless you use --persist.".to_string()
                },
            ],
            effort_items(&supported, ctx.config.reasoning_effort, ctx.config.model.as_str()),
            Some(InlineListSelection::ConfigAction(format!(
                "effort:{}",
                ctx.config.reasoning_effort.as_str()
            ))),
            None,
        );

        let Some(selection) = super::ui::wait_for_list_modal_selection(&mut ctx).await else {
            ctx.renderer
                .line(MessageStyle::Info, "Effort selection cancelled.")?;
            return Ok(SlashCommandControl::Continue);
        };

        let InlineListSelection::ConfigAction(action) = selection else {
            ctx.renderer.line(
                MessageStyle::Error,
                "Unsupported effort selection received from inline UI.",
            )?;
            return Ok(SlashCommandControl::Continue);
        };

        let Some(value) = action.strip_prefix("effort:") else {
            ctx.renderer.line(
                MessageStyle::Error,
                "Unsupported effort selection received from inline UI.",
            )?;
            return Ok(SlashCommandControl::Continue);
        };

        let Some(chosen) = ReasoningEffortLevel::parse(value) else {
            ctx.renderer.line(
                MessageStyle::Error,
                "Invalid effort level returned by inline UI.",
            )?;
            return Ok(SlashCommandControl::Continue);
        };

        chosen
    };

    apply_effort_change(&mut ctx, chosen, persist).await?;
    Ok(SlashCommandControl::Continue)
}

async fn apply_effort_change(
    ctx: &mut SlashCommandContext<'_>,
    effort: ReasoningEffortLevel,
    persist: bool,
) -> Result<()> {
    let changed = ctx.config.reasoning_effort != effort;
    ctx.config.reasoning_effort = effort;

    if let Some(cfg) = ctx.vt_cfg.as_mut() {
        cfg.agent.reasoning_effort = effort;
    }

    sync_thread_reasoning_effort(ctx, effort);

    if persist {
        persist_effort_preference(ctx.config.workspace.as_path(), ctx.vt_cfg, effort)?;
    }

    refresh_header_reasoning(ctx).await?;

    let description = effort_description(effort, ctx.config.model.as_str());
    let verb = if changed { "Set" } else { "Kept" };
    let scope = if persist {
        " Updated workspace default for future sessions as well."
    } else {
        ""
    };
    ctx.renderer.line(
        MessageStyle::Info,
        &format!(
            "{} effort level to {}: {}{}",
            verb,
            effort.as_str(),
            description,
            scope
        ),
    )?;

    Ok(())
}

fn effort_items(
    supported: &[ReasoningEffortLevel],
    current: ReasoningEffortLevel,
    model: &str,
) -> Vec<InlineListItem> {
    supported
        .iter()
        .map(|level| InlineListItem {
            title: level.as_str().to_string(),
            subtitle: Some(effort_description(*level, model).to_string()),
            badge: (*level == current).then_some("Current".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "effort:{}",
                level.as_str()
            ))),
            search_value: Some(format!(
                "{} {}",
                level.as_str(),
                effort_description(*level, model)
            )),
        })
        .collect()
}

fn supported_effort_levels(ctx: &SlashCommandContext<'_>) -> Vec<ReasoningEffortLevel> {
    let Some(provider) = Provider::from_str(ctx.config.provider.as_str()).ok() else {
        return provider_supports_effort(ctx)
            .then(all_effort_levels)
            .unwrap_or_default();
    };

    if let Some(preset) = resolve_model_preset(provider, ctx.config.model.as_str()) {
        return preset
            .supported_reasoning_efforts
            .iter()
            .map(|preset| preset.effort)
            .collect();
    }

    provider_supports_effort(ctx)
        .then(all_effort_levels)
        .unwrap_or_default()
}

fn resolve_model_preset(provider: Provider, model: &str) -> Option<ModelPreset> {
    builtin_model_presets()
        .into_iter()
        .find(|preset| preset.provider == provider && preset.model == model)
}

fn provider_supports_effort(ctx: &SlashCommandContext<'_>) -> bool {
    ctx.provider_client
        .supports_reasoning_effort(&ctx.config.model)
}

fn all_effort_levels() -> Vec<ReasoningEffortLevel> {
    ReasoningEffortLevel::allowed_values()
        .iter()
        .filter_map(|value| ReasoningEffortLevel::parse(value))
        .collect()
}

fn sync_thread_reasoning_effort(ctx: &mut SlashCommandContext<'_>, effort: ReasoningEffortLevel) {
    if let Some(mut metadata) = ctx.thread_handle.metadata() {
        metadata.reasoning_effort = effort.as_str().to_string();
        ctx.thread_handle.replace_metadata(Some(metadata));
    }
}

fn persist_effort_preference(
    workspace: &std::path::Path,
    vt_cfg: &mut Option<VTCodeConfig>,
    effort: ReasoningEffortLevel,
) -> Result<()> {
    let mut manager = crate::main_helpers::load_workspace_config(workspace)?;
    let mut config = manager.config().clone();
    config.agent.reasoning_effort = effort;
    manager.save_config(&config)?;
    *vt_cfg = Some(config);
    Ok(())
}

async fn refresh_header_reasoning(ctx: &mut SlashCommandContext<'_>) -> Result<()> {
    let mode_label = match (ctx.config.ui_surface, ctx.full_auto) {
        (vtcode_core::config::types::UiSurfacePreference::Inline, true) => "auto".to_string(),
        (vtcode_core::config::types::UiSurfacePreference::Inline, false) => "inline".to_string(),
        (vtcode_core::config::types::UiSurfacePreference::Alternate, _) => "alt".to_string(),
        (vtcode_core::config::types::UiSurfacePreference::Auto, true) => "auto".to_string(),
        (vtcode_core::config::types::UiSurfacePreference::Auto, false) => "std".to_string(),
    };
    let next_header_context = build_inline_header_context(
        ctx.config,
        ctx.vt_cfg.as_ref(),
        ctx.session_bootstrap,
        provider_label_for_header(ctx),
        ctx.config.model.clone(),
        ctx.provider_client
            .effective_context_size(&ctx.config.model),
        mode_label,
        ctx.config.reasoning_effort.as_str().to_string(),
    )
    .await?;
    ctx.header_context.clone_from(&next_header_context);
    ctx.handle.set_header_context(next_header_context);
    Ok(())
}

fn provider_label_for_header(ctx: &SlashCommandContext<'_>) -> String {
    if ctx.config.provider.eq_ignore_ascii_case("openai")
        && ctx.config.openai_chatgpt_auth.is_some()
    {
        return "OpenAI (ChatGPT)".to_string();
    }

    let key = ctx.config.provider.trim();
    if key.is_empty() {
        return ctx.provider_client.name().to_string();
    }

    ctx.vt_cfg
        .as_ref()
        .map(|cfg| cfg.provider_display_name(key))
        .filter(|label| !label.trim().is_empty())
        .unwrap_or_else(|| key.to_string())
}

fn effort_description(level: ReasoningEffortLevel, model: &str) -> &'static str {
    match level {
        ReasoningEffortLevel::None => "No additional reasoning overhead for the fastest responses",
        ReasoningEffortLevel::Minimal => "Very light reasoning with minimal extra latency",
        ReasoningEffortLevel::Low => "Quick, straightforward implementation with minimal overhead",
        ReasoningEffortLevel::Medium => {
            "Balanced approach with standard implementation and testing"
        }
        ReasoningEffortLevel::High => {
            "Comprehensive implementation with extensive testing and documentation"
        }
        ReasoningEffortLevel::XHigh if model == "claude-opus-4-7" => {
            "Deeper reasoning than high, just below maximum (Opus 4.7 only)"
        }
        ReasoningEffortLevel::XHigh => "Deeper reasoning for harder, longer-running problems",
        ReasoningEffortLevel::Max => {
            "Maximum adaptive effort for the most demanding tasks on supported Anthropic models"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{effort_description, resolve_model_preset};
    use vtcode_core::config::models::Provider;
    use vtcode_core::config::types::ReasoningEffortLevel;

    #[test]
    fn opus_preset_exposes_xhigh_and_max() {
        let preset = resolve_model_preset(Provider::Anthropic, "claude-opus-4-7")
            .expect("preset should exist");
        let levels = preset
            .supported_reasoning_efforts
            .iter()
            .map(|value| value.effort)
            .collect::<Vec<_>>();
        assert_eq!(
            levels,
            vec![
                ReasoningEffortLevel::Low,
                ReasoningEffortLevel::Medium,
                ReasoningEffortLevel::High,
                ReasoningEffortLevel::XHigh,
                ReasoningEffortLevel::Max,
            ]
        );
    }

    #[test]
    fn xhigh_description_matches_requested_opus_copy() {
        assert_eq!(
            effort_description(ReasoningEffortLevel::XHigh, "claude-opus-4-7"),
            "Deeper reasoning than high, just below maximum (Opus 4.7 only)"
        );
    }
}
