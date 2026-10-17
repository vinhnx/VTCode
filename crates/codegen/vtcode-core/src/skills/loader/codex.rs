use std::fs;
use std::path::{Path, PathBuf};

use hashbrown::HashSet;
use serde::Deserialize;

use crate::skills::model::SkillLoadOutcome;

#[derive(Debug, Default, Deserialize)]
struct CodexConfig {
    #[serde(default)]
    skills: CodexSkillsConfig,
}

#[derive(Debug, Default, Deserialize)]
struct CodexSkillsConfig {
    #[serde(default)]
    config: Vec<CodexSkillToggle>,
}

#[derive(Debug, Deserialize)]
struct CodexSkillToggle {
    #[serde(default)]
    path: Option<PathBuf>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default = "default_skill_toggle_enabled")]
    enabled: bool,
}

#[derive(Debug, Default)]
struct DisabledSkillSelectors {
    paths: HashSet<PathBuf>,
    names: HashSet<String>,
}

fn default_skill_toggle_enabled() -> bool {
    true
}

pub(crate) fn filter_disabled_skills(outcome: &mut SkillLoadOutcome, home_dir: Option<&Path>) {
    let disabled = disabled_skill_selectors(home_dir);
    if disabled.paths.is_empty() && disabled.names.is_empty() {
        return;
    }

    outcome.skills.retain(|skill| {
        let canonical = dunce::canonicalize(&skill.path).unwrap_or_else(|_| skill.path.clone());
        !disabled.paths.contains(&canonical) && !disabled.names.contains(&skill.name)
    });
}

fn disabled_skill_selectors(home_dir: Option<&Path>) -> DisabledSkillSelectors {
    let Some(home_dir) = home_dir else {
        return DisabledSkillSelectors::default();
    };

    let config_path = home_dir.join(".codex").join("config.toml");
    let Ok(content) = fs::read_to_string(&config_path) else {
        return DisabledSkillSelectors::default();
    };
    let Ok(config) = toml::from_str::<CodexConfig>(&content) else {
        return DisabledSkillSelectors::default();
    };

    let mut selectors = DisabledSkillSelectors::default();
    for entry in config.skills.config.into_iter().filter(|entry| !entry.enabled) {
        if let Some(path) = entry.path {
            selectors.paths.insert(dunce::canonicalize(&path).unwrap_or(path));
        }
        if let Some(name) = entry.name.map(|name| name.trim().to_string())
            && !name.is_empty()
        {
            selectors.names.insert(name);
        }
    }

    selectors
}
