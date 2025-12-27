use crate::skills::loader::{SkillLoaderConfig, load_skills};
use crate::skills::model::SkillLoadOutcome;
use crate::skills::system::install_system_skills;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

pub struct SkillsManager {
    codex_home: PathBuf,
    cache_by_cwd: RwLock<HashMap<PathBuf, SkillLoadOutcome>>,
}

impl SkillsManager {
    pub fn new(codex_home: PathBuf) -> Self {
        // Try to install system skills, log error if fails but don't panic
        if let Err(err) = install_system_skills(&codex_home) {
            tracing::error!("failed to install system skills: {err}");
        }

        Self {
            codex_home,
            cache_by_cwd: RwLock::new(HashMap::new()),
        }
    }

    pub fn skills_for_cwd(&self, cwd: &Path) -> SkillLoadOutcome {
        self.skills_for_cwd_with_options(cwd, false)
    }

    pub fn skills_for_cwd_with_options(&self, cwd: &Path, force_reload: bool) -> SkillLoadOutcome {
        if !force_reload {
            let cache = self.cache_by_cwd.read().unwrap();
            if let Some(outcome) = cache.get(cwd) {
                return outcome.clone();
            }
        }

        let project_root = find_git_root(cwd);

        let config = SkillLoaderConfig {
            codex_home: self.codex_home.clone(),
            cwd: cwd.to_path_buf(),
            project_root,
        };

        let outcome = load_skills(&config);

        let mut cache = self.cache_by_cwd.write().unwrap();
        cache.insert(cwd.to_path_buf(), outcome.clone());

        outcome
    }

    /// Clear the entire cache
    pub fn clear_cache(&self) {
        let mut cache = self.cache_by_cwd.write().unwrap();
        cache.clear();
    }
}

fn find_git_root(path: &Path) -> Option<PathBuf> {
    let mut current = path;
    loop {
        if current.join(".git").exists() {
            return Some(current.to_path_buf());
        }
        if let Some(parent) = current.parent() {
            current = parent;
        } else {
            return None;
        }
    }
}
