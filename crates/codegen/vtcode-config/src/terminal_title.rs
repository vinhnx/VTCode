use serde::{Deserialize, Serialize};

pub const DEFAULT_TERMINAL_TITLE_ITEMS: &[&str] = &["spinner", "project"];

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
pub struct TerminalTitleConfig {
    /// Ordered title items; `None` resolves to `["spinner", "project"]`
    /// (spinner enabled by default), and an empty array disables VT
    /// Code-managed titles.
    #[serde(default)]
    pub items: Option<Vec<String>>,

    /// Show the rotating loading indicator in the terminal title. Disable
    /// to suppress the tab spinner (footer shimmer is unaffected).
    #[serde(default = "default_global_indicator")]
    pub global_indicator: bool,
}

fn default_global_indicator() -> bool {
    true
}

/// Effective title items with the global loading indicator applied: a
/// disabled indicator drops the spinner entry, keeping every other item.
pub fn effective_title_items(items: Option<&[String]>, global_indicator: bool) -> Vec<String> {
    let resolved = items
        .map(<[String]>::to_vec)
        .unwrap_or_else(|| DEFAULT_TERMINAL_TITLE_ITEMS.iter().map(|item| (*item).to_string()).collect());
    if global_indicator {
        resolved
    } else {
        resolved.into_iter().filter(|item| item != "spinner").collect()
    }
}

impl TerminalTitleConfig {
    #[must_use]
    fn effective_items(&self) -> Vec<String> {
        self.items
            .clone()
            .unwrap_or_else(|| DEFAULT_TERMINAL_TITLE_ITEMS.iter().map(|item| (*item).to_string()).collect())
    }

    #[must_use]
    fn manages_title(&self) -> bool {
        self.items.as_ref().is_none_or(|items| !items.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_TERMINAL_TITLE_ITEMS, TerminalTitleConfig, effective_title_items};

    #[test]
    fn shipped_default_items_lead_with_the_spinner() {
        // The shipped binary must always manage the title with the activity
        // spinner first; single source in `data/default_config.toml` mirrors it.
        assert_eq!(DEFAULT_TERMINAL_TITLE_ITEMS, ["spinner", "project"]);
    }

    #[test]
    fn serde_defaults_when_terminal_title_table_is_missing() {
        let config: TerminalTitleConfig = toml::from_str("").expect("config should parse");

        assert_eq!(config.items, None);
    }

    #[test]
    fn serde_preserves_explicit_empty_items() {
        let config: TerminalTitleConfig = toml::from_str("items = []").expect("config should parse");

        assert_eq!(config.items, Some(Vec::new()));
    }

    #[test]
    fn serde_preserves_valid_item_list() {
        let config: TerminalTitleConfig =
            toml::from_str("items = [\"spinner\", \"project\", \"model\"]").expect("config should parse");

        assert_eq!(config.items, Some(vec!["spinner".to_string(), "project".to_string(), "model".to_string()]));
    }

    #[test]
    fn disabled_global_indicator_drops_only_the_spinner() {
        assert_eq!(
            effective_title_items(None, true),
            DEFAULT_TERMINAL_TITLE_ITEMS
                .iter()
                .map(|item| (*item).to_string())
                .collect::<Vec<_>>()
        );
        // Defaults resolve first, so the spinner drops even when unset.
        assert_eq!(effective_title_items(None, false), vec!["project".to_string()]);
        let configured = Some(vec!["status".to_string(), "spinner".to_string(), "thread".to_string()]);
        assert_eq!(
            effective_title_items(configured.as_deref(), false),
            vec!["status".to_string(), "thread".to_string()]
        );
        // The setting defaults on through serde.
        let config: TerminalTitleConfig = toml::from_str("").expect("config should parse");
        assert!(config.global_indicator);
    }

    #[test]
    fn missing_items_uses_default_terminal_title_items() {
        let config = TerminalTitleConfig::default();

        assert_eq!(
            config.effective_items(),
            DEFAULT_TERMINAL_TITLE_ITEMS
                .iter()
                .map(|item| (*item).to_string())
                .collect::<Vec<_>>()
        );
        assert!(config.manages_title());
    }

    #[test]
    fn explicit_empty_items_disables_terminal_title_management() {
        let config = TerminalTitleConfig { items: Some(Vec::new()), global_indicator: true };

        assert!(config.effective_items().is_empty());
        assert!(!config.manages_title());
    }

    #[test]
    fn explicit_items_are_preserved() {
        let config = TerminalTitleConfig {
            items: Some(vec!["project".to_string(), "model".to_string()]),
            global_indicator: true,
        };

        assert_eq!(config.effective_items(), vec!["project".to_string(), "model".to_string()]);
        assert!(config.manages_title());
    }
}
