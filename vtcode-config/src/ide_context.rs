use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum IdeContextProviderFamily {
    #[default]
    VscodeCompatible,
    Zed,
    Generic,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum IdeContextProviderMode {
    #[default]
    Auto,
    VscodeCompatible,
    Zed,
    Generic,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IdeContextProviderConfig {
    #[serde(default = "default_provider_enabled")]
    pub enabled: bool,
}

impl Default for IdeContextProviderConfig {
    fn default() -> Self {
        Self {
            enabled: default_provider_enabled(),
        }
    }
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct IdeContextProvidersConfig {
    #[serde(default)]
    pub vscode_compatible: IdeContextProviderConfig,
    #[serde(default)]
    pub zed: IdeContextProviderConfig,
    #[serde(default)]
    pub generic: IdeContextProviderConfig,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IdeContextConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Inject active editor context into request-time model input.
    #[serde(default = "default_inject_into_prompt")]
    pub inject_into_prompt: bool,
    #[serde(default = "default_show_in_tui")]
    pub show_in_tui: bool,
    #[serde(default = "default_include_selection_text")]
    pub include_selection_text: bool,
    #[serde(default)]
    pub provider_mode: IdeContextProviderMode,
    #[serde(default)]
    pub providers: IdeContextProvidersConfig,
}

impl IdeContextConfig {
    pub fn provider_enabled(&self, family: IdeContextProviderFamily) -> bool {
        match family {
            IdeContextProviderFamily::VscodeCompatible => self.providers.vscode_compatible.enabled,
            IdeContextProviderFamily::Zed => self.providers.zed.enabled,
            IdeContextProviderFamily::Generic => self.providers.generic.enabled,
        }
    }

    pub fn allows_provider_family(&self, family: IdeContextProviderFamily) -> bool {
        if !self.enabled || !self.provider_enabled(family) {
            return false;
        }

        match self.provider_mode {
            IdeContextProviderMode::Auto => true,
            IdeContextProviderMode::VscodeCompatible => {
                family == IdeContextProviderFamily::VscodeCompatible
            }
            IdeContextProviderMode::Zed => family == IdeContextProviderFamily::Zed,
            IdeContextProviderMode::Generic => family == IdeContextProviderFamily::Generic,
        }
    }
}

impl Default for IdeContextConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            inject_into_prompt: default_inject_into_prompt(),
            show_in_tui: default_show_in_tui(),
            include_selection_text: default_include_selection_text(),
            provider_mode: IdeContextProviderMode::default(),
            providers: IdeContextProvidersConfig::default(),
        }
    }
}

const fn default_enabled() -> bool {
    true
}

const fn default_provider_enabled() -> bool {
    true
}

const fn default_inject_into_prompt() -> bool {
    true
}

const fn default_show_in_tui() -> bool {
    true
}

const fn default_include_selection_text() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::{
        IdeContextConfig, IdeContextProviderFamily, IdeContextProviderMode,
        IdeContextProvidersConfig,
    };

    #[test]
    fn default_config_allows_enabled_providers_in_auto_mode() {
        let config = IdeContextConfig::default();
        assert!(config.allows_provider_family(IdeContextProviderFamily::VscodeCompatible));
        assert!(config.allows_provider_family(IdeContextProviderFamily::Zed));
        assert!(config.allows_provider_family(IdeContextProviderFamily::Generic));
    }

    #[test]
    fn provider_mode_filters_other_families() {
        let config = IdeContextConfig {
            provider_mode: IdeContextProviderMode::Zed,
            ..IdeContextConfig::default()
        };
        assert!(!config.allows_provider_family(IdeContextProviderFamily::VscodeCompatible));
        assert!(config.allows_provider_family(IdeContextProviderFamily::Zed));
    }

    #[test]
    fn disabled_provider_family_is_rejected() {
        let config = IdeContextConfig {
            providers: IdeContextProvidersConfig {
                generic: super::IdeContextProviderConfig { enabled: false },
                ..IdeContextProvidersConfig::default()
            },
            ..IdeContextConfig::default()
        };
        assert!(!config.allows_provider_family(IdeContextProviderFamily::Generic));
    }
}
