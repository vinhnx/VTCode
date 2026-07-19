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
        return Provider::from_str(configured).unwrap_or(hardcoded_default);
    }

    // Prefer the first discovered non-local, non-managed provider (a real
    // API-key/OAuth credential). Local providers are always "discovered" but
    // are rarely what a user wants as the default on first run.
    if let Some(first_ready) = discovered
        .iter()
        .find(|d| !matches!(d.source, CredentialSource::Local | CredentialSource::ManagedAuth))
    {
        return first_ready.provider;
    }

    hardcoded_default
}

pub(crate) fn prompt_provider(
    renderer: &mut AnsiRenderer,
    default: Provider,
    discovered: &[DiscoveredProvider],
) -> Result<Provider> {
    renderer.line(MessageStyle::Status, "Choose your default provider:")?;
    let providers = Provider::all_providers();

    match select_provider_with_ratatui(&providers, default, discovered) {
        Ok(provider) => Ok(provider),
        Err(error) => {
            renderer.line(MessageStyle::Info, &format!("Falling back to manual input ({error})."))?;
            prompt_provider_text(renderer, &providers, default, discovered)
        }
    }
}

fn provider_entries(providers: &[Provider], discovered: &[DiscoveredProvider]) -> Vec<SelectionEntry> {
    providers
        .iter()
        .map(|provider| {
            let subtitle = find_discovered(discovered, *provider).map(|entry| {
                let mark = match entry.source {
                    CredentialSource::Env => "✓",
                    CredentialSource::SecureStorage => "✓",
                    CredentialSource::OAuth => "✓",
                    CredentialSource::ManagedAuth => "•",
                    CredentialSource::Local => "•",
                };
                format!("{mark} {}", entry.source.describe(*provider))
            });
            SelectionEntry::new(provider.label(), subtitle)
        })
        .collect()
}

fn prompt_provider_text(
    renderer: &mut AnsiRenderer,
    providers: &[Provider],
    default: Provider,
    discovered: &[DiscoveredProvider],
) -> Result<Provider> {
    for (index, provider) in providers.iter().enumerate() {
        let marker = if find_discovered(discovered, *provider).is_some() {
            " ✓"
        } else {
            ""
        };
        renderer.line(MessageStyle::Info, &format!("  {}) {}{marker}", index + 1, provider.label()))?;
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
    // user can just press Enter when their key is already exported.
    let default_index = discovered
        .iter()
        .find(|d| matches!(d.source, CredentialSource::Env | CredentialSource::SecureStorage | CredentialSource::OAuth))
        .and_then(|d| providers.iter().position(|p| *p == d.provider))
        .unwrap_or_else(|| providers.iter().position(|provider| *provider == default).unwrap_or(0));

    let instructions = format!(
        "Default: {}. Use ↑/↓ or j/k to choose, Enter to confirm, Esc to keep the default. ✓ = ready to use.",
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
            },
            DiscoveredProvider {
                provider: Provider::Ollama,
                source: CredentialSource::Local,
            },
        ];
        let entries = provider_entries(&[Provider::OpenAI, Provider::OpenRouter, Provider::Ollama], &discovered);

        assert_eq!(entries[0].title, "OpenAI");
        assert!(entries[0].description.is_none(), "OpenAI has no credential, description must be None");

        assert_eq!(entries[1].title, "OpenRouter");
        let or_desc = entries[1].description.as_ref().expect("OpenRouter should be marked ready");
        assert!(or_desc.starts_with("✓"), "OpenRouter description should start with ✓: {or_desc}");

        assert_eq!(entries[2].title, "Ollama");
        let ollama_desc = entries[2].description.as_ref().expect("Ollama should be marked");
        assert!(ollama_desc.starts_with("•"), "Ollama (local) description should start with •: {ollama_desc}");
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
            },
            DiscoveredProvider {
                provider: Provider::Copilot,
                source: CredentialSource::ManagedAuth,
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
            },
            DiscoveredProvider {
                provider: Provider::OpenRouter,
                source: CredentialSource::Env,
            },
        ];

        assert_eq!(resolve_initial_provider(&config, &discovered), Provider::OpenRouter);
    }
}
