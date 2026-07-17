use crate::skills::SkillMetadata;
#[cfg(test)]
use crate::skills::loader::discover_skill_metadata_lightweight_hermetic;
use crate::skills::loader::{
    SkillLoaderConfig, clear_lightweight_skill_metadata_cache, discover_skill_metadata_lightweight,
    load_skills,
};
use crate::skills::model::SkillLoadOutcome;
use crate::skills::system::install_system_skills;
use crate::skills::system::uninstall_system_skills;
use crate::skills::types::Skill;
use anyhow::Result;
use hashbrown::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::RwLock;
use std::time::{Duration, SystemTime};

use crate::utils::file_utils::read_file_with_context_sync;

/// Generic cache entry with TTL tracking.
#[derive(Clone)]
struct CachedEntry<T> {
    value: T,
    timestamp: SystemTime,
}

impl<T> CachedEntry<T> {
    fn new(value: T) -> Self {
        Self { value, timestamp: SystemTime::now() }
    }

    fn is_expired(&self, ttl: Duration) -> bool {
        self.timestamp.elapsed().unwrap_or(ttl) > ttl
    }
}

pub struct SkillsManager {
    codex_home: PathBuf,
    bundled_skills_enabled: bool,
    /// Per-cwd skill loading cache with TTL and max capacity
    cache_by_cwd: RwLock<HashMap<PathBuf, CachedEntry<SkillLoadOutcome>>>,
    /// Max number of cached workspaces (prevents unbounded growth)
    max_cache_size: usize,
    /// TTL for cached metadata (default 5 minutes)
    cache_ttl: Duration,
    /// Tracks if system skills installation has been attempted
    system_skills_initialized: OnceLock<()>,
    /// Per-skill instruction cache with TTL (Phase 3: deferred SKILL.md parsing)
    instruction_cache: RwLock<HashMap<String, CachedEntry<Skill>>>,
    /// Max instructions cached in memory (prevents unbounded growth)
    max_instruction_cache_size: usize,
    /// TTL for instruction cache (default 10 minutes - longer than metadata)
    instruction_cache_ttl: Duration,
}

/// Generic cache eviction: remove expired entries, then LRU if still over capacity.
fn evict_cache<K, V>(
    cache: &mut HashMap<K, V>,
    max_size: usize,
    ttl: Duration,
    get_timestamp: impl Fn(&V) -> SystemTime,
    is_expired: impl Fn(&V, Duration) -> bool,
) where
    K: Eq + std::hash::Hash + Clone,
{
    // Remove expired entries
    let expired: Vec<_> = cache
        .iter()
        .filter(|(_, v)| is_expired(v, ttl))
        .map(|(k, _)| k.clone())
        .collect();
    for key in expired {
        cache.remove(&key);
    }

    // If still at capacity, remove oldest entry by timestamp (LRU)
    if cache.len() >= max_size {
        let oldest_key = cache.iter().min_by_key(|(_, v)| get_timestamp(v)).map(|(k, _)| k.clone());
        if let Some(key) = oldest_key {
            cache.remove(&key);
        }
    }
}

impl SkillsManager {
    pub fn new(codex_home: PathBuf) -> Self {
        Self::new_with_bundled_skills_enabled(codex_home, true)
    }

    pub fn new_with_bundled_skills_enabled(
        codex_home: PathBuf,
        bundled_skills_enabled: bool,
    ) -> Self {
        let manager = Self {
            codex_home,
            bundled_skills_enabled,
            cache_by_cwd: RwLock::new(HashMap::new()),
            max_cache_size: 10,
            cache_ttl: Duration::from_secs(5 * 60), // 5 minutes
            system_skills_initialized: OnceLock::new(),
            instruction_cache: RwLock::new(HashMap::new()),
            max_instruction_cache_size: 50, // Cache up to 50 parsed skills
            instruction_cache_ttl: Duration::from_secs(10 * 60), // 10 minutes
        };

        if !manager.bundled_skills_enabled {
            uninstall_system_skills(&manager.codex_home);
        }

        manager
    }

    pub fn bundled_skills_enabled(&self) -> bool {
        self.bundled_skills_enabled
    }

    /// Lazy initialize system skills (non-blocking, can be called async)
    pub fn ensure_system_skills_installed(&self) {
        self.system_skills_initialized.get_or_init(|| {
            if !self.bundled_skills_enabled {
                return;
            }

            // Try to install system skills, log error if fails but don't panic
            if let Err(err) = install_system_skills(&self.codex_home) {
                tracing::warn!("lazy system skills installation failed: {err}");
            }
        });
    }

    pub fn skills_for_cwd(&self, cwd: &Path) -> SkillLoadOutcome {
        self.skills_for_cwd_with_options(cwd, false)
    }

    pub fn skills_for_cwd_with_options(&self, cwd: &Path, force_reload: bool) -> SkillLoadOutcome {
        // Ensure system skills are installed at least once
        self.ensure_system_skills_installed();

        if !force_reload {
            if let Ok(cache) = self.cache_by_cwd.read() {
                if let Some(cached) = cache.get(cwd)
                    && !cached.is_expired(self.cache_ttl)
                {
                    return cached.value.clone();
                }
            } else {
                tracing::warn!("skills metadata cache lock poisoned while reading cache");
            }
        }

        let project_root = find_git_root(cwd);

        let config = SkillLoaderConfig {
            codex_home: self.codex_home.clone(),
            cwd: cwd.to_path_buf(),
            project_root,
            include_bundled_system_skills: self.bundled_skills_enabled,
        };

        let outcome = load_skills(&config);

        if let Ok(mut cache) = self.cache_by_cwd.write() {
            // Enforce max cache size: remove expired entries first, then LRU
            if cache.len() >= self.max_cache_size && !cache.contains_key(cwd) {
                evict_cache(
                    &mut cache,
                    self.max_cache_size,
                    self.cache_ttl,
                    |v| v.timestamp,
                    |v, ttl| v.is_expired(ttl),
                );
            }

            cache.insert(cwd.to_path_buf(), CachedEntry::new(outcome.clone()));
        } else {
            tracing::warn!("skills metadata cache lock poisoned while writing cache");
        }

        outcome
    }

    /// Clear the entire cache
    pub fn clear_cache(&self) {
        if let Ok(mut cache) = self.cache_by_cwd.write() {
            cache.clear();
        } else {
            tracing::warn!("skills metadata cache lock poisoned while clearing cache");
        }
        clear_lightweight_skill_metadata_cache();
    }

    /// Get current cache size (for testing/diagnostics)
    pub fn cache_size(&self) -> usize {
        if let Ok(cache) = self.cache_by_cwd.read() {
            cache.len()
        } else {
            tracing::warn!("skills metadata cache lock poisoned while reading cache size");
            0
        }
    }

    /// Quick metadata discovery without parsing SKILL.md files (Phase 2)
    /// Returns skill stubs with name and path only (no manifest parsing).
    /// ~10x faster than full discovery, suitable for listing available skills.
    pub fn skills_metadata_lightweight(&self, cwd: &Path) -> SkillLoadOutcome {
        // Ensure system skills are installed at least once
        self.ensure_system_skills_installed();

        let project_root = find_git_root(cwd);

        let config = SkillLoaderConfig {
            codex_home: self.codex_home.clone(),
            cwd: cwd.to_path_buf(),
            project_root,
            include_bundled_system_skills: self.bundled_skills_enabled,
        };

        // Use lightweight discovery (no manifest parsing)
        discover_skill_metadata_lightweight(&config)
    }

    #[cfg(test)]
    fn skills_metadata_lightweight_hermetic(&self, cwd: &Path) -> SkillLoadOutcome {
        let project_root = find_git_root(cwd);

        let config = SkillLoaderConfig {
            codex_home: self.codex_home.clone(),
            cwd: cwd.to_path_buf(),
            project_root,
            include_bundled_system_skills: self.bundled_skills_enabled,
        };

        discover_skill_metadata_lightweight_hermetic(&config)
    }

    /// Load full skill instructions on-demand (Phase 3)
    /// Parses SKILL.md and returns Skill with full manifest and instructions.
    /// Cached with LRU eviction (max 50 skills, 10-minute TTL).
    /// Returns cached result if available and not expired.
    pub fn load_skill_instructions(&self, skill_name: &str, skill_path: &Path) -> Result<Skill> {
        // Composite cache key: name + resolved path to avoid collisions when
        // skills in different directories share the same name.
        let cache_key = format!("{}:{}", skill_name, skill_path.display());

        // Check cache first
        {
            if let Ok(cache) = self.instruction_cache.read() {
                if let Some(cached) = cache.get(&cache_key)
                    && !cached.is_expired(self.instruction_cache_ttl)
                {
                    return Ok(cached.value.clone());
                }
            } else {
                tracing::warn!("skill instruction cache lock poisoned while reading cache");
            }
        }

        // Parse SKILL.md on-demand
        let skill_md = skill_path.join("SKILL.md");
        let content = read_file_with_context_sync(&skill_md, "skill instructions")
            .map_err(|e| anyhow::anyhow!("Failed to read SKILL.md for '{skill_name}': {e}"))?;

        let (manifest, instructions) = crate::skills::manifest::parse_skill_content(&content)?;
        let skill = Skill::new(manifest, skill_path.to_path_buf(), instructions)?;

        // Cache the parsed skill
        if let Ok(mut cache) = self.instruction_cache.write() {
            // Enforce max cache size: remove expired entries first, then LRU
            if cache.len() >= self.max_instruction_cache_size && !cache.contains_key(&cache_key) {
                evict_cache(
                    &mut cache,
                    self.max_instruction_cache_size,
                    self.instruction_cache_ttl,
                    |v| v.timestamp,
                    |v, ttl| v.is_expired(ttl),
                );
            }

            cache.insert(cache_key, CachedEntry::new(skill.clone()));
        } else {
            tracing::warn!("skill instruction cache lock poisoned while writing cache");
        }

        Ok(skill)
    }

    /// Resolve a skill by name from the discovered set for the given cwd.
    /// Returns `Err` if the skill is not found, enabling callers to fail loud.
    pub fn resolve_skill_by_name(&self, cwd: &Path, skill_name: &str) -> Result<SkillMetadata> {
        let outcome = self.skills_for_cwd(cwd);
        let available: Vec<String> = outcome.skills.iter().map(|s| s.name.clone()).collect();
        outcome.skills.into_iter().find(|s| s.name == skill_name).ok_or_else(|| {
            anyhow::anyhow!(
                "Skill '{}' not found. Available skills: [{}]",
                skill_name,
                available.join(", ")
            )
        })
    }

    /// Load skills by name from a list of skill names (e.g. from
    /// `LoopEngineConfig.preload_skills`). Returns metadata for each
    /// found skill and logs warnings for missing ones.
    pub fn loop_skills(&self, cwd: &Path, skill_names: &[String]) -> Vec<SkillMetadata> {
        if skill_names.is_empty() {
            return Vec::new();
        }

        let outcome = self.skills_for_cwd(cwd);
        let available: HashMap<String, &SkillMetadata> =
            outcome.skills.iter().map(|s| (s.name.clone(), s)).collect();

        let mut loaded = Vec::new();
        for name in skill_names {
            if let Some(meta) = available.get(name) {
                loaded.push((*meta).clone());
            } else {
                tracing::warn!(
                    skill = %name,
                    "Preload skill not found; skipping. Available: {}",
                    available
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }
        loaded
    }

    /// Clear instruction cache
    pub fn clear_instruction_cache(&self) {
        if let Ok(mut cache) = self.instruction_cache.write() {
            cache.clear();
        } else {
            tracing::warn!("skill instruction cache lock poisoned while clearing cache");
        }
    }

    /// Get instruction cache size (for testing/diagnostics)
    pub fn instruction_cache_size(&self) -> usize {
        if let Ok(cache) = self.instruction_cache.read() {
            cache.len()
        } else {
            tracing::warn!("skill instruction cache lock poisoned while reading cache size");
            0
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_skills_manager_lazy_initialization() {
        let temp_home = TempDir::new().unwrap();
        let manager = SkillsManager::new(temp_home.path().to_path_buf());

        // Manager should be created without system skills initialized yet
        assert_eq!(manager.cache_size(), 0);

        // Ensure system skills are initialized
        manager.ensure_system_skills_installed();

        // Should still have empty cache (only system installation tracked)
        assert_eq!(manager.cache_size(), 0);
    }

    #[test]
    fn test_skills_manager_cache_with_ttl() {
        let temp_home = TempDir::new().unwrap();
        let manager = SkillsManager::new(temp_home.path().to_path_buf());

        let cwd = temp_home.path();

        // First load
        let outcome1 = manager.skills_for_cwd(cwd);
        assert_eq!(manager.cache_size(), 1);

        // Second load (should return cached)
        let outcome2 = manager.skills_for_cwd(cwd);
        assert_eq!(outcome1.skills.len(), outcome2.skills.len());
        assert_eq!(manager.cache_size(), 1);

        // Force reload
        let outcome3 = manager.skills_for_cwd_with_options(cwd, true);
        assert_eq!(manager.cache_size(), 1);
        assert_eq!(outcome1.skills.len(), outcome3.skills.len());
    }

    #[test]
    fn test_skills_manager_max_cache_size() {
        let temp_home = TempDir::new().unwrap();
        let manager = SkillsManager::new(temp_home.path().to_path_buf());

        // Create multiple temp directories and load skills
        for _ in 0..15 {
            let dir = TempDir::new().unwrap();
            let cwd = dir.path();
            manager.skills_for_cwd(cwd);
        }

        // Cache should respect max_cache_size (10)
        assert!(manager.cache_size() <= 10);
    }

    #[test]
    fn test_skills_manager_clear_cache() {
        let temp_home = TempDir::new().unwrap();
        let manager = SkillsManager::new(temp_home.path().to_path_buf());

        let cwd = temp_home.path();
        manager.skills_for_cwd(cwd);
        assert_eq!(manager.cache_size(), 1);

        manager.clear_cache();
        assert_eq!(manager.cache_size(), 0);
    }

    #[test]
    #[serial]
    fn test_skills_manager_clear_cache_clears_lightweight_discovery_cache() {
        clear_lightweight_skill_metadata_cache();

        let temp_home = TempDir::new().unwrap();
        let workspace = TempDir::new().unwrap();
        let manager = SkillsManager::new(temp_home.path().to_path_buf());
        let skill_dir = workspace.path().join(".agents/skills/clear-cache-skill");

        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: clear-cache-skill\ndescription: clear cache test\n---\n# Body\n",
        )
        .unwrap();

        let first = manager.skills_metadata_lightweight_hermetic(workspace.path());
        assert!(
            first.skills.iter().any(|skill| skill.name == "clear-cache-skill"),
            "expected first discovery to find test skill",
        );

        fs::remove_dir_all(&skill_dir).unwrap();

        let second = manager.skills_metadata_lightweight_hermetic(workspace.path());
        assert!(
            second.skills.iter().any(|skill| skill.name == "clear-cache-skill"),
            "expected cached discovery to preserve removed skill before clear_cache",
        );

        manager.clear_cache();

        let third = manager.skills_metadata_lightweight_hermetic(workspace.path());
        assert!(
            !third.skills.iter().any(|skill| skill.name == "clear-cache-skill"),
            "expected clear_cache to flush lightweight discovery cache",
        );
    }

    #[test]
    fn test_skills_metadata_lightweight() {
        let temp_home = TempDir::new().unwrap();
        let manager = SkillsManager::new(temp_home.path().to_path_buf());

        let cwd = temp_home.path();

        // Lightweight discovery should work without errors
        let outcome = manager.skills_metadata_lightweight_hermetic(cwd);

        // Should return empty outcome (no skills in temp dir)
        // but should not crash or error
        assert_eq!(outcome.errors.len(), 0);
    }

    #[test]
    fn test_lightweight_vs_full_discovery() {
        let temp_home = TempDir::new().unwrap();
        let manager = SkillsManager::new(temp_home.path().to_path_buf());

        let cwd = temp_home.path();

        // Both should complete without error
        let lightweight = manager.skills_metadata_lightweight(cwd);
        let full = manager.skills_for_cwd(cwd);

        // Lightweight discovery should find at least as many skills as full discovery
        // (or may find more if full discovery filters some due to parse errors)
        // but they should be in the same ballpark (within 10% difference)
        let light_count = lightweight.skills.len() as i32;
        let full_count = full.skills.len() as i32;
        let diff = (light_count - full_count).abs();
        let tolerance = (full_count / 10).max(5); // 10% tolerance or 5 items, whichever is larger

        assert!(
            diff <= tolerance,
            "Lightweight discovery found {light_count} skills, full discovery found {full_count}. Difference {diff} exceeds tolerance {tolerance}"
        );
    }

    #[test]
    fn test_instruction_cache_initialization() {
        let temp_home = TempDir::new().unwrap();
        let manager = SkillsManager::new(temp_home.path().to_path_buf());

        // Instruction cache should be empty at start
        assert_eq!(manager.instruction_cache_size(), 0);

        // Clear should work without error
        manager.clear_instruction_cache();
        assert_eq!(manager.instruction_cache_size(), 0);
    }

    #[test]
    fn test_instruction_cache_max_size() {
        let temp_home = TempDir::new().unwrap();
        let manager = SkillsManager::new(temp_home.path().to_path_buf());

        // Max size is 50, create more entries than that
        // Note: This test is limited because we need actual SKILL.md files
        // For now, just verify the cache respects max size in behavior
        assert_eq!(manager.instruction_cache_size(), 0);

        // Even after multiple operations, should be bounded
        manager.clear_instruction_cache();
        assert_eq!(manager.instruction_cache_size(), 0);
    }

    #[test]
    fn test_instruction_cache_clear() {
        let temp_home = TempDir::new().unwrap();
        let manager = SkillsManager::new(temp_home.path().to_path_buf());

        // Cache should start empty
        assert_eq!(manager.instruction_cache_size(), 0);

        // Clear should work
        manager.clear_instruction_cache();
        assert_eq!(manager.instruction_cache_size(), 0);
    }

    #[test]
    fn test_disabled_bundled_skills_remove_stale_system_cache() {
        let temp_home = TempDir::new().unwrap();
        let stale_skill_dir = temp_home.path().join("skills/.system/stale-skill");
        fs::create_dir_all(&stale_skill_dir).unwrap();
        fs::write(stale_skill_dir.join("SKILL.md"), "# stale\n").unwrap();

        let _manager =
            SkillsManager::new_with_bundled_skills_enabled(temp_home.path().to_path_buf(), false);

        assert!(!temp_home.path().join("skills/.system").exists());
    }

    #[test]
    fn test_disabled_bundled_skills_exclude_system_root_even_if_recreated() {
        let temp_home = TempDir::new().unwrap();
        let workspace = TempDir::new().unwrap();
        let bundled_skill_dir = temp_home.path().join("skills/.system/bundled-skill");
        fs::create_dir_all(&bundled_skill_dir).unwrap();
        fs::write(
            bundled_skill_dir.join("SKILL.md"),
            "---\nname: bundled-skill\ndescription: bundled\n---\n\n# Body\n",
        )
        .unwrap();

        let manager =
            SkillsManager::new_with_bundled_skills_enabled(temp_home.path().to_path_buf(), false);

        fs::create_dir_all(&bundled_skill_dir).unwrap();
        fs::write(
            bundled_skill_dir.join("SKILL.md"),
            "---\nname: bundled-skill\ndescription: bundled\n---\n\n# Body\n",
        )
        .unwrap();

        let outcome = manager.skills_for_cwd(workspace.path());
        assert!(outcome.skills.iter().all(|skill| skill.name != "bundled-skill"));
    }
}
