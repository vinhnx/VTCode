//! Transactional service-tier updates for `/fast`.

use anyhow::{Context, Result, bail};
use vtcode_config::{AnthropicSpeed, ConfigManager, OpenAIServiceTier};
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig;
use vtcode_core::llm::provider::{ContextWindowProvider, LLMProvider};
use vtcode_core::utils::ansi::MessageStyle;

use super::{SlashCommandContext, SlashCommandControl};
use crate::agent::runloop::unified::fast_mode::{effective_speed_label, fast_mode_selected, supports_fast_mode};
use crate::agent::runloop::unified::session_setup::create_provider_client;

pub(crate) async fn handle_toggle_fast_mode(
    ctx: SlashCommandContext<'_>,
    enable: Option<bool>,
) -> Result<SlashCommandControl> {
    let result = crate::main_helpers::load_workspace_config(&ctx.config.workspace)
        .and_then(|mut manager| apply_fast_mode(ctx.config, ctx.vt_cfg, ctx.provider_client, &mut manager, enable));
    match result {
        Ok(tier) => {
            ctx.session_stats.dismiss_fast_mode_suggestion();
            ctx.header_context.service_tier = ctx
                .vt_cfg
                .as_ref()
                .and_then(|cfg| effective_speed_label(ctx.config, cfg))
                .map(|label| format!("{}{}", vtcode_config::constants::ui::HEADER_SERVICE_TIER_PREFIX, label));
            ctx.handle.set_header_context(ctx.header_context.clone());
            let message = if let Some(label) = tier {
                format!(
                    "{label} enabled for {} at higher cost (premium pricing). Saved for subsequent requests and sessions; the server reports the speed actually served. Switching Claude speeds affects prompt-cache reuse.",
                    ctx.config.model
                )
            } else {
                "Fast mode off: provider default speed. Saved for subsequent requests and sessions. Switching Claude speeds affects prompt-cache reuse.".to_string()
            };
            ctx.renderer.line(MessageStyle::Info, &message)?;
        }
        Err(err) => ctx
            .renderer
            .line(MessageStyle::Error, &format!("Could not change fast mode: {err:#}"))?,
    }
    Ok(SlashCommandControl::Continue)
}

fn apply_fast_mode(
    config: &AgentConfig,
    vt_cfg: &mut Option<VTCodeConfig>,
    provider_client: &mut Box<dyn LLMProvider>,
    manager: &mut ConfigManager,
    enable: Option<bool>,
) -> Result<Option<&'static str>> {
    let mut runtime_cfg = vt_cfg.as_ref().unwrap_or_else(|| manager.config()).clone();
    let is_openai = vtcode_commons::provider::Provider::OpenAI.name_matches(&config.provider);
    let is_anthropic = vtcode_commons::provider::Provider::Anthropic.name_matches(&config.provider);
    if runtime_cfg.custom_provider(&config.provider).is_some() || !(is_openai || is_anthropic) {
        bail!(
            "/fast supports native OpenAI and Anthropic only. Select an eligible route with /model; saved preferences are unchanged."
        );
    }
    let anthropic = is_anthropic;
    let enabled = enable.unwrap_or(!fast_mode_selected(config, &runtime_cfg));
    if enabled && !supports_fast_mode(config, &runtime_cfg) {
        bail!(
            "/fast on requires native OpenAI API-key authentication with gpt-6-astra or gpt-6.1-sol, or native Anthropic with claude-opus-5-5, claude-opus-5, or claude-opus-4-8. Select one with /model, or use /fast off to clear this provider override."
        );
    }
    let tier = enabled.then_some(OpenAIServiceTier::Ultrafast);
    let speed = enabled.then_some(AnthropicSpeed::Fast);
    if anthropic {
        runtime_cfg.provider.anthropic.speed = speed;
    } else {
        runtime_cfg.provider.openai.service_tier = tier;
    }
    // Prepare before saving and install only after saving succeeds. Rebuilding
    // also clears a tier captured by the old client at session startup.
    let prepared = create_provider_client(config, Some(&runtime_cfg))?;
    let prepared = ContextWindowProvider::wrap(
        prepared,
        &config.model,
        Some(provider_client.effective_context_size(&config.model)),
    );
    let mut persisted_cfg = manager.config().clone();
    if anthropic {
        persisted_cfg.provider.anthropic.speed = speed;
    } else {
        persisted_cfg.provider.openai.service_tier = tier;
    }
    manager.save_config(&persisted_cfg).context("Failed to save service tier")?;
    *provider_client = prepared;
    *vt_cfg = Some(runtime_cfg);
    Ok(if anthropic {
        speed.map(AnthropicSpeed::as_str)
    } else {
        tier.map(OpenAIServiceTier::as_str)
    })
}

#[cfg(test)]
#[path = "fast/tests.rs"]
mod tests;
