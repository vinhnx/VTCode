use crate::skills::cli_bridge::discover_cli_tools;
use crate::skills::model::{SkillLoadOutcome, SkillScope};
use hashbrown::HashSet;
use std::path::{Path, PathBuf};

// Config for loader
#[derive(Debug, Clone)]
pub struct SkillLoaderConfig {
    pub codex_home: PathBuf,
    pub cwd: PathBuf,
    pub project_root: Option<PathBuf>,
    pub include_bundled_system_skills: bool,
}

pub struct SkillRoot {
    pub path: PathBuf,
    pub scope: SkillScope,
    pub is_tool_root: bool,
    pub is_plugin_root: bool,
}

mod cache;
mod codex;
mod discovery;
mod enhanced;
mod mentions;
mod resources;

pub use self::enhanced::{EnhancedSkill, EnhancedSkillLoader};

#[cfg(test)]
pub(crate) use self::discovery::find_git_root;
use self::discovery::tool_config_to_metadata;
use self::discovery::{discover_metadata_under_root, discover_skills_under_root, skill_roots_with_home_dir};
#[cfg(test)]
pub(crate) use self::enhanced::{discovery_config_for_codex_home, select_traditional_skill_ctx};

pub use self::resources::load_skill_resources;

use self::codex::filter_disabled_skills;

pub use self::mentions::{SkillMentionDetectionOptions, detect_skill_mentions, detect_skill_mentions_with_options};

pub(crate) use self::cache::clear_lightweight_skill_metadata_cache;
use self::cache::{LightweightSkillCacheKey, cache_lightweight_skill_outcome, get_cached_lightweight_skill_outcome};

pub fn load_skills(config: &SkillLoaderConfig) -> SkillLoadOutcome {
    let home_dir = dirs::home_dir();
    load_skills_with_home_dir(config, home_dir.as_deref())
}

/// Lightweight metadata discovery that avoids parsing SKILL.md files.
/// Returns skill stubs with only name, description, and path (no manifest parsing).
/// This is much faster for listing available skills.
pub fn discover_skill_metadata_lightweight(config: &SkillLoaderConfig) -> SkillLoadOutcome {
    let home_dir = dirs::home_dir();
    discover_skill_metadata_lightweight_with_home_dir(config, home_dir.as_deref())
}

/// Internal helper that allows specifying an explicit home directory.
/// This is useful for testing to avoid picking up real user skills from ~/.agents/skills.
pub(crate) fn load_skills_with_home_dir(config: &SkillLoaderConfig, home_dir: Option<&Path>) -> SkillLoadOutcome {
    let mut outcome = SkillLoadOutcome::default();
    let roots = skill_roots_with_home_dir(config, home_dir);

    for root in roots {
        discover_skills_under_root(&root, &mut outcome);
    }

    add_system_cli_tools(&mut outcome);
    dedup_and_sort(&mut outcome);
    filter_disabled_skills(&mut outcome, home_dir);
    outcome
}

/// Internal helper for lightweight discovery with explicit home directory.
/// Useful for hermetic tests.
fn discover_skill_metadata_lightweight_with_home_dir(
    config: &SkillLoaderConfig,
    home_dir: Option<&Path>,
) -> SkillLoadOutcome {
    let cache_key = LightweightSkillCacheKey::new(config, home_dir);
    if let Some(cached) = get_cached_lightweight_skill_outcome(&cache_key) {
        return cached;
    }

    let outcome = discover_skill_metadata_lightweight_uncached(config, home_dir);
    cache_lightweight_skill_outcome(cache_key, &outcome);
    outcome
}

fn discover_skill_metadata_lightweight_uncached(
    config: &SkillLoaderConfig,
    home_dir: Option<&Path>,
) -> SkillLoadOutcome {
    let mut outcome = SkillLoadOutcome::default();
    let roots = skill_roots_with_home_dir(config, home_dir);

    for root in roots {
        discover_metadata_under_root(&root, &mut outcome);
    }

    add_system_cli_tools(&mut outcome);
    dedup_and_sort(&mut outcome);
    filter_disabled_skills(&mut outcome, home_dir);
    outcome
}

fn add_system_cli_tools(outcome: &mut SkillLoadOutcome) {
    if let Ok(system_tools) = discover_cli_tools() {
        for tool in system_tools {
            if let Ok(skill) = tool_config_to_metadata(&tool, SkillScope::System) {
                outcome.skills.push(skill);
            }
        }
    }
}

fn dedup_and_sort(outcome: &mut SkillLoadOutcome) {
    let mut seen: HashSet<String> = HashSet::new();
    let mut seen_paths: HashSet<PathBuf> = HashSet::new();
    let mut shadowed: Vec<String> = Vec::new();
    outcome.skills.retain(|skill| {
        // Same physical file discovered via overlapping roots (symlinks,
        // canonicalization aliases) is not a conflict; drop silently.
        let canonical = dunce::canonicalize(&skill.path).unwrap_or_else(|_| skill.path.clone());
        if !seen_paths.insert(canonical) {
            return false;
        }
        if seen.insert(skill.name.clone()) {
            return true;
        }
        // Agent Skills interop: same-name skills must not silently replace one
        // another. First discovery wins (project roots precede user roots).
        // Shadowing is expected (e.g. repo `.agents/skills` over an installed
        // user plugin with the same name), so this stays at debug level.
        shadowed.push(format!("{} ({})", skill.name, skill.path.display()));
        false
    });
    if !shadowed.is_empty() {
        tracing::debug!(
            shadowed = ?shadowed,
            "duplicate skill names shadowed by earlier discovery; only the first occurrence is loaded"
        );
    }
    outcome.skills.sort_by(|a, b| a.name.cmp(&b.name));
}

/// Test helper for hermetic skill loading that does not pick up user skills.
/// Use this in tests to avoid failures when ~/.agents/skills contains skills.
#[cfg(test)]
pub fn load_skills_hermetic(config: &SkillLoaderConfig) -> SkillLoadOutcome {
    load_skills_with_home_dir(config, None)
}

/// Test helper for hermetic lightweight skill discovery.
#[cfg(test)]
pub fn discover_skill_metadata_lightweight_hermetic(config: &SkillLoaderConfig) -> SkillLoadOutcome {
    discover_skill_metadata_lightweight_with_home_dir(config, None)
}

#[cfg(test)]
mod tests;
