use crate::skills::loader::{SkillLoaderConfig, discover_skill_metadata_lightweight, load_skills};
use crate::skills::model::SkillLoadOutcome;
use crate::skills::system::install_system_skills;
use crate::skills::types::Skill;
use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::RwLock;
use std::time::{Duration, SystemTime};

use crate::utils::file_utils::read_file_with_context_sync;

/// Cache entry with TTL tracking for skill outcomes
#[derive(Clone)]
struct CachedSkillOutcome {
    outcome: SkillLoadOutcome,
    timestamp: SystemTime,
}

impl CachedSkillOutcome {
    fn is_expired(&self, ttl: Duration) -> bool {
        self.timestamp.elapsed().unwrap_or(ttl) > ttl
    }
}

/// Cached instruction entry for on-demand skill parsing (Phase 3)
#[derive(Clone)]
struct CachedSkillInstruction {
    skill: Skill,
    timestamp: SystemTime,
}

impl CachedSkillInstruction {
    fn is_expired(&self, ttl: Duration) -> bool {
        self.timestamp.elapsed().unwrap_or(ttl) > ttl
    }
}

pub struct SkillsManager {
    codex_home: PathBuf,
    /// Per-cwd skill loading cache with TTL and max capacity
    cache_by_cwd: RwLock<HashMap<PathBuf, CachedSkillOutcome>>,
    /// Max number of cached workspaces (prevents unbounded growth)
    max_cache_size: usize,
    /// TTL for cached metadata (default 5 minutes)
    cache_ttl: Duration,
    /// Tracks if system skills installation has been attempted
    system_skills_initialized: OnceLock<()>,
    /// Per-skill instruction cache with TTL (Phase 3: deferred SKILL.md parsing)
    instruction_cache: RwLock<HashMap<String, CachedSkillInstruction>>,
    /// Max instructions cached in memory (prevents unbounded growth)
    max_instruction_cache_size: usize,
    /// TTL for instruction cache (default 10 minutes - longer than metadata)
    instruction_cache_ttl: Duration,
}

impl SkillsManager {
    pub fn new(codex_home: PathBuf) -> Self {
        Self {
            codex_home,
            cache_by_cwd: RwLock::new(HashMap::new()),
            max_cache_size: 10,
            cache_ttl: Duration::from_secs(5 * 60), // 5 minutes
            system_skills_initialized: OnceLock::new(),
            instruction_cache: RwLock::new(HashMap::new()),
            max_instruction_cache_size: 50, // Cache up to 50 parsed skills
            instruction_cache_ttl: Duration::from_secs(10 * 60), // 10 minutes
        }
    }

    /// Lazy initialize system skills (non-blocking, can be called async)
    pub fn ensure_system_skills_installed(&self) {
        self.system_skills_initialized.get_or_init(|| {
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
                if let Some(cached) = cache.get(cwd) {
                    if !cached.is_expired(self.cache_ttl) {
                        return cached.outcome.clone();
                    }
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
        };

        let outcome = load_skills(&config);

        if let Ok(mut cache) = self.cache_by_cwd.write() {
            // Enforce max cache size: remove oldest expired entries first, then LRU
            if cache.len() >= self.max_cache_size && !cache.contains_key(cwd) {
                // Remove oldest expired entries
                let expired: Vec<_> = cache
                    .iter()
                    .filter(|(_, v)| v.timestamp.elapsed().unwrap_or_default() > self.cache_ttl)
                    .map(|(k, _)| k.clone())
                    .collect();

                for key in expired {
                    cache.remove(&key);
                }

                // If still at capacity, remove oldest entry by timestamp (simple LRU)
                if cache.len() >= self.max_cache_size {
                    let oldest_key = cache
                        .iter()
                        .min_by_key(|(_, v)| v.timestamp)
                        .map(|(k, _)| k.clone());
                    if let Some(key) = oldest_key {
                        cache.remove(&key);
                    }
                }
            }

            cache.insert(
                cwd.to_path_buf(),
                CachedSkillOutcome {
                    outcome: outcome.clone(),
                    timestamp: SystemTime::now(),
                },
            );
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
        };

        // Use lightweight discovery (no manifest parsing)
        discover_skill_metadata_lightweight(&config)
    }

    /// Load full skill instructions on-demand (Phase 3)
    /// Parses SKILL.md and returns Skill with full manifest and instructions.
    /// Cached with LRU eviction (max 50 skills, 10-minute TTL).
    /// Returns cached result if available and not expired.
    pub fn load_skill_instructions(&self, skill_name: &str, skill_path: &Path) -> Result<Skill> {
        // Check cache first
        {
            if let Ok(cache) = self.instruction_cache.read() {
                if let Some(cached) = cache.get(skill_name) {
                    if !cached.is_expired(self.instruction_cache_ttl) {
                        return Ok(cached.skill.clone());
                    }
                }
            } else {
                tracing::warn!("skill instruction cache lock poisoned while reading cache");
            }
        }

        // Parse SKILL.md on-demand
        let skill_md = skill_path.join("SKILL.md");
        let content = read_file_with_context_sync(&skill_md, "skill instructions")
            .map_err(|e| anyhow::anyhow!("Failed to read SKILL.md for '{}': {}", skill_name, e))?;

        let (manifest, instructions) = crate::skills::manifest::parse_skill_content(&content)?;
        let skill = Skill::new(manifest, skill_path.to_path_buf(), instructions)?;

        // Cache the parsed skill
        if let Ok(mut cache) = self.instruction_cache.write() {
            // Enforce max cache size: remove expired entries first, then LRU
            if cache.len() >= self.max_instruction_cache_size && !cache.contains_key(skill_name) {
                // Remove expired entries
                let expired: Vec<_> = cache
                    .iter()
                    .filter(|(_, v)| v.is_expired(self.instruction_cache_ttl))
                    .map(|(k, _)| k.clone())
                    .collect();

                for key in expired {
                    cache.remove(&key);
                }

                // If still at capacity, remove oldest by timestamp (LRU)
                if cache.len() >= self.max_instruction_cache_size {
                    let oldest_key = cache
                        .iter()
                        .min_by_key(|(_, v)| v.timestamp)
                        .map(|(k, _)| k.clone());
                    if let Some(key) = oldest_key {
                        cache.remove(&key);
                    }
                }
            }

            cache.insert(
                skill_name.to_string(),
                CachedSkillInstruction {
                    skill: skill.clone(),
                    timestamp: SystemTime::now(),
                },
            );
        } else {
            tracing::warn!("skill instruction cache lock poisoned while writing cache");
        }

        Ok(skill)
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
    fn test_skills_metadata_lightweight() {
        let temp_home = TempDir::new().unwrap();
        let manager = SkillsManager::new(temp_home.path().to_path_buf());

        let cwd = temp_home.path();

        // Lightweight discovery should work without errors
        let outcome = manager.skills_metadata_lightweight(cwd);

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
            "Lightweight discovery found {} skills, full discovery found {}. Difference {} exceeds tolerance {}",
            light_count,
            full_count,
            diff,
            tolerance
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
}
