use std::str::FromStr;

use anyhow::Result;
use vtcode_config::api_keys::{CredentialSource, DiscoveredProvider, find_discovered};
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::models::Provider;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_ui::tui::ui::interactive_list::SelectionEntry;

use super::common::{prompt_with_placeholder, run_selection};

/// Pick the initial provider for the wizard.
///
/// Order of preference:
/// 1. The provider already recorded in `vtcode.toml` (if valid).
/// 2. The first provider with a discoverable credential (env var, OS keyring,
///    OAuth, managed auth, or local) — so a user who already exports, say,
///    `OPENROUTER_API_KEY` is not defaulted into OpenAI (which they may have
///    no key for).
/// 3. The built-in default provider.
pub(crate) fn resolve_initial_provider(config: &VTCodeConfig, discovered: &[DiscoveredProvider]) -> Provider {
    let configured = config.agent.provider.trim();
    let hardcoded_default =
        Provider::from_str(vtcode_core::config::constants::defaults::DEFAULT_PROVIDER).unwrap_or(Provider::OpenAI);

    if !configured.is_empty() {
        let parsed = Provider::from_str(configured).unwrap_or(hardcoded_default);
        if config.providers_whitelist.is_empty()
            || config
                .providers_whitelist
                .iter()
                .any(|w| w.eq_ignore_ascii_case(parsed.as_ref()))
        {
            return parsed;
        }
    }

    if config.providers_whitelist.is_empty() {
        if let Some(first_ready) = discovered
            .iter()
            .find(|d| !matches!(d.source, CredentialSource::Local | CredentialSource::ManagedAuth))
        {
            return first_ready.provider;
        }
        return hardcoded_default;
    }

    let whitelisted: Vec<Provider> = Provider::all_providers()
        .into_iter()
        .filter(|p| config.providers_whitelist.iter().any(|w| w.eq_ignore_ascii_case(p.as_ref())))
        .collect();

    if let Some(first_ready) = discovered.iter().find(|d| {
        !matches!(d.source, CredentialSource::Local | CredentialSource::ManagedAuth)
            && whitelisted.contains(&d.provider)
    }) {
        return first_ready.provider;
    }

    if let Some(first_ready) = discovered.iter().find(|d| whitelisted.contains(&d.provider)) {
        return first_ready.provider;
    }

    whitelisted.into_iter().next().unwrap_or(hardcoded_default)
}

pub(crate) fn prompt_provider(
    renderer: &mut AnsiRenderer,
    default: Provider,
    discovered: &[DiscoveredProvider],
    whitelist: &[String],
) -> Result<Provider> {
    renderer.line(MessageStyle::Status, "Choose your default provider:")?;
    let all = Provider::all_providers();
    let available: Vec<Provider> = if whitelist.is_empty() {
        all
    } else {
        all.into_iter()
            .filter(|p| whitelist.iter().any(|w| w.eq_ignore_ascii_case(p.as_ref())))
            .collect()
    };
    let providers = sort_providers_by_readiness(available, discovered);

    match select_provider_with_ratatui(&providers, default, discovered) {
        Ok(provider) => Ok(provider),
        Err(error) => {
            renderer.line(MessageStyle::Info, &format!("Falling back to manual input ({error})."))?;
            prompt_provider_text(renderer, &providers, default, discovered)
        }
    }
}

/// Reorder providers so ready ones come first, in three tiers:
/// 0. Real credential ready (env var / OS keyring / OAuth) — the ones the
///    user can pick and use immediately.
/// 1. Nominal ready (managed auth / local) — always "ready" but rarely the
///    default a user wants on first run.
/// 2. Not ready (no credential discovered).
///
/// Stable within each tier (preserves the input order, which is
/// `Provider::all_providers()`).
fn sort_providers_by_readiness(providers: Vec<Provider>, discovered: &[DiscoveredProvider]) -> Vec<Provider> {
    let mut tiered: Vec<(u8, usize, Provider)> = providers
        .into_iter()
        .enumerate()
        .map(|(idx, provider)| {
            let tier = match find_discovered(discovered, provider).map(|d| d.source) {
                Some(CredentialSource::Env | CredentialSource::SecureStorage | CredentialSource::OAuth) => 0,
                Some(CredentialSource::ManagedAuth | CredentialSource::Local) => 1,
                None => 2,
            };
            (tier, idx, provider)
        })
        .collect();
    tiered.sort_by_key(|(tier, idx, _)| (*tier, *idx));
    tiered.into_iter().map(|(_, _, p)| p).collect()
}

fn provider_entries(providers: &[Provider], discovered: &[DiscoveredProvider]) -> Vec<SelectionEntry> {
    providers
        .iter()
        .map(|provider| {
            let subtitle = find_discovered(discovered, *provider).map(|entry| ready_subtitle(*provider, entry));
            SelectionEntry::new(provider.label(), subtitle)
        })
        .collect()
}

/// Build the per-provider readiness marker shown under the provider name.
/// Surfaces the *specific* env var that was discovered so the user knows
/// exactly what vtcode read (e.g. `GOOGLE_API_KEY` vs `GEMINI_API_KEY`).
fn ready_subtitle(provider: Provider, entry: &DiscoveredProvider) -> String {
    let mark = match entry.source {
        CredentialSource::Env | CredentialSource::SecureStorage | CredentialSource::OAuth => "✓",
        CredentialSource::ManagedAuth | CredentialSource::Local => "•",
    };
    let detail = match entry.source {
        CredentialSource::Env => match entry.env_var {
            Some(var) => format!("found {var} in environment"),
            None => entry.source.describe(provider).to_string(),
        },
        _ => entry.source.describe(provider).to_string(),
    };
    format!("{mark} {detail}")
}

fn prompt_provider_text(
    renderer: &mut AnsiRenderer,
    providers: &[Provider],
    default: Provider,
    discovered: &[DiscoveredProvider],
) -> Result<Provider> {
    for (index, provider) in providers.iter().enumerate() {
        let marker = match find_discovered(discovered, *provider).map(|d| d.source) {
            Some(CredentialSource::Env | CredentialSource::SecureStorage | CredentialSource::OAuth) => " ✓",
            Some(_) => " •",
            None => "",
        };
        renderer.line(MessageStyle::Info, &format!("  {:>2}. {}{marker}", index + 1, provider.label()))?;
    }

    let default_label = default.to_string();

    loop {
        let input = prompt_with_placeholder(&format!("Provider [{default_label}]"))?;
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(default);
        }

        if let Ok(index) = trimmed.parse::<usize>()
            && let Some(provider) = providers.get(index - 1)
        {
            return Ok(*provider);
        }

        match Provider::from_str(trimmed) {
            Ok(provider) => return Ok(provider),
            Err(err) => {
                renderer.line(MessageStyle::Error, &format!("{err}. Please choose a valid provider."))?;
            }
        }
    }
}

fn select_provider_with_ratatui(
    providers: &[Provider],
    default: Provider,
    discovered: &[DiscoveredProvider],
) -> Result<Provider> {
    let entries = provider_entries(providers, discovered);

    // Default the cursor to the first ready (env/keyring/OAuth) provider so the
    // user can just press Enter when their key is already exported. After
    // `sort_providers_by_readiness` this is typically index 0, but search
    // explicitly in case the reordered list's first entry is tier 1/2.
    let default_index = discovered
        .iter()
        .find(|d| matches!(d.source, CredentialSource::Env | CredentialSource::SecureStorage | CredentialSource::OAuth))
        .and_then(|d| providers.iter().position(|p| *p == d.provider))
        .unwrap_or_else(|| providers.iter().position(|provider| *provider == default).unwrap_or(0));

    let instructions = format!(
        "Default: {}. Use ↑/↓ or j/k to choose, Enter to confirm, Esc to keep the default. ✓ = ready to use, • = ready (local/managed).",
        default.label()
    );
    let selected_index = run_selection("Providers", &instructions, &entries, default_index)?;
    Ok(providers[selected_index])
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtcode_config::api_keys::discover_available_providers;

    fn base_config() -> VTCodeConfig {
        VTCodeConfig::default()
    }

    #[test]
    fn provider_entries_mark_discovered_providers() {
        // In CI/debug, discovery includes at least the local + managed-auth
        // providers. Build a synthetic discovery list to assert the description.
        let discovered = vec![
            DiscoveredProvider {
                provider: Provider::OpenRouter,
                source: CredentialSource::Env,
                env_var: Some("OPENROUTER_API_KEY"),
            },
            DiscoveredProvider {
                provider: Provider::Ollama,
                source: CredentialSource::Local,
                env_var: None,
            },
        ];
        let entries = provider_entries(&[Provider::OpenAI, Provider::OpenRouter, Provider::Ollama], &discovered);

        assert_eq!(entries[0].title, "OpenAI");
        assert!(entries[0].description.is_none(), "OpenAI has no credential, description must be None");

        assert_eq!(entries[1].title, "OpenRouter");
        let or_desc = entries[1].description.as_ref().expect("OpenRouter should be marked ready");
        assert!(or_desc.starts_with("✓"), "OpenRouter description should start with ✓: {or_desc}");
        assert!(or_desc.contains("OPENROUTER_API_KEY"), "OpenRouter description should name the env var: {or_desc}");

        assert_eq!(entries[2].title, "Ollama");
        let ollama_desc = entries[2].description.as_ref().expect("Ollama should be marked");
        assert!(ollama_desc.starts_with("•"), "Ollama (local) description should start with •: {ollama_desc}");
    }

    #[test]
    fn ready_subtitle_surfaces_alternate_env_var_name() {
        // When discovery used the alternate GOOGLE_API_KEY, the subtitle must
        // report *that* name, not the primary GEMINI_API_KEY.
        let entry = DiscoveredProvider {
            provider: Provider::Gemini,
            source: CredentialSource::Env,
            env_var: Some("GOOGLE_API_KEY"),
        };
        let subtitle = ready_subtitle(Provider::Gemini, &entry);
        assert!(subtitle.contains("GOOGLE_API_KEY"), "subtitle should name the alternate env var: {subtitle}");
        assert!(!subtitle.contains("GEMINI_API_KEY"), "subtitle must not name the primary var: {subtitle}");
    }

    #[test]
    fn sort_providers_by_readiness_puts_ready_first() {
        let all = Provider::all_providers();
        let discovered = vec![
            DiscoveredProvider {
                provider: Provider::OpenRouter,
                source: CredentialSource::Env,
                env_var: Some("OPENROUTER_API_KEY"),
            },
            DiscoveredProvider {
                provider: Provider::Ollama,
                source: CredentialSource::Local,
                env_var: None,
            },
        ];
        let sorted = sort_providers_by_readiness(all, &discovered);

        // Tier 0 (OpenRouter) must come before tier 1 (Ollama) and tier 2
        // (everything else, e.g. OpenAI which has no credential here).
        let or_pos = sorted.iter().position(|p| *p == Provider::OpenRouter).unwrap();
        let openai_pos = sorted.iter().position(|p| *p == Provider::OpenAI).unwrap();
        let ollama_pos = sorted.iter().position(|p| *p == Provider::Ollama).unwrap();
        assert!(or_pos < openai_pos, "ready OpenRouter must come before unconfigured OpenAI");
        assert!(or_pos < ollama_pos, "tier-0 OpenRouter must come before tier-1 Ollama");
        assert!(ollama_pos < openai_pos, "tier-1 Ollama must come before tier-2 OpenAI");
    }

    #[test]
    fn sort_providers_by_readiness_is_stable_within_tier() {
        // Two ready providers must keep their `all_providers()` relative order.
        let all = Provider::all_providers();
        let discovered = vec![
            DiscoveredProvider {
                provider: Provider::OpenRouter,
                source: CredentialSource::Env,
                env_var: Some("OPENROUTER_API_KEY"),
            },
            DiscoveredProvider {
                provider: Provider::Anthropic,
                source: CredentialSource::Env,
                env_var: Some("ANTHROPIC_API_KEY"),
            },
        ];
        let sorted = sort_providers_by_readiness(all, &discovered);
        let or_pos = sorted.iter().position(|p| *p == Provider::OpenRouter).unwrap();
        let anthropic_pos = sorted.iter().position(|p| *p == Provider::Anthropic).unwrap();
        // In `all_providers()`, Anthropic (index 1) comes before OpenRouter
        // (index 9), so Anthropic must still come first within tier 0.
        assert!(anthropic_pos < or_pos, "stable sort must preserve all_providers() order within a tier");
    }

    #[test]
    fn resolve_initial_provider_prefers_configured_value() {
        let mut config = base_config();
        config.agent.provider = "anthropic".to_string();

        let discovered = discover_available_providers();

        assert_eq!(resolve_initial_provider(&config, &discovered), Provider::Anthropic);
    }

    #[test]
    fn resolve_initial_provider_falls_back_to_hardcoded_default_when_nothing_ready() {
        // With no discovered remote credential, the hardcoded default provider
        // must be returned — not a local/managed-auth provider. Clear the
        // configured provider so we actually exercise the discovered-list
        // fallback branch (rather than the configured-value short-circuit).
        let mut config = base_config();
        config.agent.provider = String::new();
        let discovered: Vec<DiscoveredProvider> = vec![
            DiscoveredProvider {
                provider: Provider::Ollama,
                source: CredentialSource::Local,
                env_var: None,
            },
            DiscoveredProvider {
                provider: Provider::Copilot,
                source: CredentialSource::ManagedAuth,
                env_var: None,
            },
        ];

        let expected =
            Provider::from_str(vtcode_core::config::constants::defaults::DEFAULT_PROVIDER).unwrap_or(Provider::OpenAI);

        let resolved = resolve_initial_provider(&config, &discovered);
        assert_eq!(
            resolved, expected,
            "should fall back to the hardcoded default when no remote credential is discovered"
        );
    }

    #[test]
    fn resolve_initial_provider_prefers_first_ready_remote_provider() {
        let mut config = base_config();
        config.agent.provider = String::new();
        let discovered = vec![
            DiscoveredProvider {
                provider: Provider::Ollama,
                source: CredentialSource::Local,
                env_var: None,
            },
            DiscoveredProvider {
                provider: Provider::OpenRouter,
                source: CredentialSource::Env,
                env_var: Some("OPENROUTER_API_KEY"),
            },
        ];

        assert_eq!(resolve_initial_provider(&config, &discovered), Provider::OpenRouter);
    }
}
