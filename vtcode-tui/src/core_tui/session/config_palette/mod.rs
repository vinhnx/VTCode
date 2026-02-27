use crate::config::loader::{ConfigManager, VTCodeConfig};
use ratatui::widgets::ListState;

mod actions;
mod items;
mod persistence;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq)]
pub enum ConfigItemKind {
    Bool { value: bool },
    Enum { value: String, options: Vec<String> },
    Number { value: i64, min: i64, max: i64 },
    Display { value: String },
}

#[derive(Debug, Clone)]
pub struct ConfigItem {
    pub key: String,
    pub label: String,
    pub kind: ConfigItemKind,
    pub description: Option<String>,
}

pub struct ConfigPalette {
    pub items: Vec<ConfigItem>,
    pub list_state: ListState,
    pub config_manager: ConfigManager,
    // Keep a local copy to modify before saving
    pub config: VTCodeConfig,
    pub modified: bool,
}

impl ConfigPalette {
    pub fn new(manager: ConfigManager) -> Self {
        let config = manager.config().clone();
        let mut palette = Self {
            items: Vec::new(),
            list_state: ListState::default(),
            config_manager: manager,
            config,
            modified: false,
        };
        palette.reload_items_from_config();

        // select first item by default if available
        if !palette.items.is_empty() {
            palette.list_state.select(Some(0));
        }

        palette
    }

    pub fn selected(&self) -> Option<usize> {
        self.list_state.selected()
    }

    pub fn move_up(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.items.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    pub fn move_down(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i >= self.items.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }
}
