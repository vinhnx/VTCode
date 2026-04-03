use serde::{Deserialize, Serialize};

pub const DEFAULT_TERMINAL_TITLE_ITEMS: &[&str] = &["spinner", "project"];

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
pub struct TerminalTitleConfig {
    #[serde(default)]
    pub items: Option<Vec<String>>,
}

impl TerminalTitleConfig {
    #[must_use]
    pub fn effective_items(&self) -> Vec<String> {
        self.items.clone().unwrap_or_else(|| {
            DEFAULT_TERMINAL_TITLE_ITEMS
                .iter()
                .map(|item| (*item).to_string())
                .collect()
        })
    }

    #[must_use]
    pub fn manages_title(&self) -> bool {
        self.items.as_ref().is_none_or(|items| !items.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_TERMINAL_TITLE_ITEMS, TerminalTitleConfig};

    #[test]
    fn serde_defaults_when_terminal_title_table_is_missing() {
        let config: TerminalTitleConfig = toml::from_str("").expect("config should parse");

        assert_eq!(config.items, None);
    }

    #[test]
    fn serde_preserves_explicit_empty_items() {
        let config: TerminalTitleConfig =
            toml::from_str("items = []").expect("config should parse");

        assert_eq!(config.items, Some(Vec::new()));
    }

    #[test]
    fn serde_preserves_valid_item_list() {
        let config: TerminalTitleConfig =
            toml::from_str("items = [\"spinner\", \"project\", \"model\"]")
                .expect("config should parse");

        assert_eq!(
            config.items,
            Some(vec![
                "spinner".to_string(),
                "project".to_string(),
                "model".to_string()
            ])
        );
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
        let config = TerminalTitleConfig {
            items: Some(Vec::new()),
        };

        assert!(config.effective_items().is_empty());
        assert!(!config.manages_title());
    }

    #[test]
    fn explicit_items_are_preserved() {
        let config = TerminalTitleConfig {
            items: Some(vec!["project".to_string(), "model".to_string()]),
        };

        assert_eq!(
            config.effective_items(),
            vec!["project".to_string(), "model".to_string()]
        );
        assert!(config.manages_title());
    }
}
