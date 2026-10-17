//! Shared native fast-mode eligibility, effective display, and session discovery.

use anyhow::Result;
use vtcode_config::models::{Provider, ProviderModelSupport};
use vtcode_config::{AnthropicSpeed, OpenAIServiceTier, VTCodeConfig};
use vtcode_core::config::types::AgentConfig;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::state::SessionStats;
use crate::agent::runloop::model_picker::configured_openai_base_url;

pub(crate) fn supports_fast_mode(config: &AgentConfig, cfg: &VTCodeConfig) -> bool {
    cfg.anthropic_speed_supported(&config.provider, &config.model)
        || (config.provider.eq_ignore_ascii_case(Provider::OpenAI.as_ref())
            && config.openai_chatgpt_auth.is_none()
            && cfg.custom_provider(&config.provider).is_none()
            && Provider::OpenAI.supports_service_tier_value(&config.model, OpenAIServiceTier::Ultrafast)
            && configured_openai_base_url(Some(cfg))
                .is_none_or(|url| vtcode_core::llm::provider::is_standard_decisions_endpoint(&url)))
}

/// Saved preferences are provider-local; only a supported active route gets a badge.
pub(crate) fn effective_speed_label(config: &AgentConfig, cfg: &VTCodeConfig) -> Option<&'static str> {
    if cfg.anthropic_speed_supported(&config.provider, &config.model) {
        return cfg
            .provider
            .anthropic
            .speed
            .filter(|speed| *speed == AnthropicSpeed::Fast)
            .map(AnthropicSpeed::as_str);
    }
    if let Ok(provider) = config.provider.parse::<Provider>()
        && cfg.custom_provider(&config.provider).is_none()
    {
        return cfg
            .provider
            .openai
            .service_tier
            .filter(|tier| provider.supports_service_tier_value(&config.model, *tier))
            .filter(|tier| *tier != OpenAIServiceTier::Ultrafast || supports_fast_mode(config, cfg))
            .map(OpenAIServiceTier::as_str);
    }
    None
}

pub(crate) fn fast_mode_selected(config: &AgentConfig, cfg: &VTCodeConfig) -> bool {
    if Provider::Anthropic.name_matches(&config.provider) {
        cfg.provider.anthropic.speed == Some(AnthropicSpeed::Fast)
    } else if Provider::OpenAI.name_matches(&config.provider) {
        cfg.provider.openai.service_tier == Some(OpenAIServiceTier::Ultrafast)
    } else {
        false
    }
}

/// Reevaluate at idle so startup and model/auth changes share one session tip.
pub(crate) fn maybe_render_fast_mode_suggestion(
    renderer: &mut AnsiRenderer,
    config: &AgentConfig,
    cfg: Option<&VTCodeConfig>,
    stats: &mut SessionStats,
) -> Result<()> {
    if cfg.is_some_and(|cfg| fast_mode_selected(config, cfg)) {
        stats.dismiss_fast_mode_suggestion();
        return Ok(());
    }
    let applicable = renderer.supports_inline_ui()
        && !config.api_key.trim().is_empty()
        && cfg.is_some_and(|cfg| supports_fast_mode(config, cfg));
    if stats.take_fast_mode_suggestion(applicable) {
        renderer.line(
            MessageStyle::Info,
            &format!(
                "{} supports fast processing with your API key. Use /fast on for faster responses at higher cost (premium pricing); the choice is saved for future sessions. Switching Claude speeds affects prompt-cache reuse.",
                config.model
            ),
        )?;
    }
    Ok(())
}
