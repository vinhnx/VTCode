use crate::ui::tui::types::TrustMode;
use std::collections::{HashMap, HashSet};

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct TrustSetting {
    pub mode: TrustMode,
    pub updated_at: String,
}

#[allow(dead_code)]
pub struct TrustManager {
    settings: HashMap<String, TrustSetting>,
    session_cache: HashSet<String>,
}

#[allow(dead_code)]
impl TrustManager {
    pub fn new() -> Self {
        Self {
            settings: HashMap::new(),
            session_cache: HashSet::new(),
        }
    }

    pub fn check_auto_approve(&self, file_path: &str) -> bool {
        if let Some(setting) = self.settings.get(file_path)
            && matches!(setting.mode, TrustMode::AutoTrust)
        {
            return true;
        }
        self.session_cache.contains(file_path)
    }

    pub fn update_trust(&mut self, file_path: String, mode: TrustMode) {
        let timestamp = chrono::Utc::now().to_rfc3339();
        let setting = TrustSetting {
            mode,
            updated_at: timestamp,
        };

        match mode {
            TrustMode::AutoTrust | TrustMode::Always => {
                self.settings.insert(file_path.clone(), setting);
            }
            TrustMode::Session => {
                self.session_cache.insert(file_path);
            }
            TrustMode::Once => {}
        }
    }

    pub fn get_trust_mode(&self, file_path: &str) -> Option<TrustMode> {
        self.settings.get(file_path).map(|s| s.mode)
    }
}

impl Default for TrustManager {
    fn default() -> Self {
        Self::new()
    }
}
