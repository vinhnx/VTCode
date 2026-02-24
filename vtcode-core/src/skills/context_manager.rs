//! Progressive Context Management for Skills
//!
//! Manages skill context loading with memory efficiency through:
//! - Progressive disclosure (metadata → instructions → resources)
//! - Context budget tracking and enforcement
//! - LRU eviction for unused skills
//! - Memory usage monitoring
//! - Skill state persistence

use crate::skills::types::{Skill, SkillManifest};
use crate::utils::file_utils::{read_json_file_sync, write_json_file_sync};
use anyhow::{Context, Result, anyhow};
use lru::LruCache;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tracing::{debug, info, warn};

/// Configuration for context management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextConfig {
    /// Maximum total context size in tokens
    pub max_context_tokens: usize,

    /// Maximum number of cached skills
    pub max_cached_skills: usize,

    /// Token cost for skill metadata (name + description)
    pub metadata_token_cost: usize,

    /// Token cost for skill instructions per character
    pub instruction_token_factor: f64,

    /// Token cost for skill resources
    pub resource_token_cost: usize,

    /// Enable memory monitoring
    pub enable_monitoring: bool,

    /// Context eviction policy
    pub eviction_policy: EvictionPolicy,

    /// Enable persistent caching
    pub enable_persistence: bool,

    /// Cache persistence path
    pub cache_path: Option<PathBuf>,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            max_context_tokens: 50_000, // 50k tokens total
            max_cached_skills: 100,
            metadata_token_cost: 50,
            instruction_token_factor: 0.25, // ~4 chars per token
            resource_token_cost: 200,
            enable_monitoring: true,
            eviction_policy: EvictionPolicy::LRU,
            enable_persistence: false,
            cache_path: None,
        }
    }
}

/// Context eviction policies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EvictionPolicy {
    /// Least Recently Used eviction
    LRU,
    /// Least Frequently Used eviction
    LFU,
    /// Token-cost based eviction (evict most expensive)
    TokenCost,
    /// Manual eviction only
    Manual,
}

/// Skill context loading levels
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ContextLevel {
    /// Metadata only (name, description) - ~50 tokens
    Metadata,
    /// Instructions loaded - variable tokens
    Instructions,
    /// Full skill with resources - maximum tokens
    Full,
}

/// Context usage tracking
#[derive(Debug, Clone)]
pub struct ContextUsage {
    /// Number of times skill was accessed
    pub access_count: u64,

    /// Last access timestamp
    pub last_access: std::time::Instant,

    /// Total time loaded in memory
    pub total_loaded_duration: std::time::Duration,

    /// Token cost for this skill
    pub token_cost: usize,
}

impl Default for ContextUsage {
    fn default() -> Self {
        Self {
            access_count: 0,
            last_access: std::time::Instant::now(),
            total_loaded_duration: std::time::Duration::ZERO,
            token_cost: 0,
        }
    }
}

/// Skill context entry
#[derive(Debug, Clone)]
pub struct SkillContextEntry {
    /// Skill name
    pub name: String,

    /// Current context level
    pub level: ContextLevel,

    /// Skill metadata (always available)
    pub manifest: SkillManifest,

    /// Skill instructions (loaded on demand)
    pub instructions: Option<String>,

    /// Full skill object (loaded on demand)
    pub skill: Option<Skill>,

    /// Usage tracking
    pub usage: ContextUsage,

    /// Memory size estimate (bytes)
    pub memory_size: usize,
}

/// Progressive context manager
#[derive(Clone)]
pub struct ContextManager {
    config: ContextConfig,
    inner: Arc<Mutex<ContextManagerInner>>,
}

/// Inner state for ContextManager to be wrapped in Arc<Mutex<>>
struct ContextManagerInner {
    /// Active skill contexts (metadata only)
    active_skills: HashMap<String, SkillContextEntry>,

    /// LRU cache for loaded skills
    loaded_skills: LruCache<String, SkillContextEntry>,

    /// Current context usage in tokens
    current_token_usage: usize,

    /// Context usage statistics
    stats: ContextStats,
}

/// Context management statistics
#[derive(Debug, Default, Clone)]
pub struct ContextStats {
    pub total_skills_loaded: u64,
    pub total_skills_evicted: u64,
    pub total_tokens_loaded: u64,
    pub total_tokens_evicted: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub peak_token_usage: usize,
    pub current_token_usage: usize,
}

impl ContextManager {
    /// Create new context manager with default configuration
    pub fn new() -> Self {
        Self::with_config(ContextConfig::default())
    }

    /// Create new context manager with custom configuration
    pub fn with_config(config: ContextConfig) -> Self {
        let max_cached_skills = config.max_cached_skills.max(1);
        if max_cached_skills != config.max_cached_skills {
            warn!(
                configured = config.max_cached_skills,
                effective = max_cached_skills,
                "max_cached_skills must be at least 1; using fallback value"
            );
        }
        let loaded_skills = LruCache::new(
            std::num::NonZeroUsize::new(max_cached_skills).unwrap_or(std::num::NonZeroUsize::MIN),
        );

        Self {
            config: config.clone(),
            inner: Arc::new(Mutex::new(ContextManagerInner {
                active_skills: HashMap::new(),
                loaded_skills,
                current_token_usage: 0,
                stats: ContextStats::default(),
            })),
        }
    }
}

impl Default for ContextManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextManager {
    /// Register skill metadata (Level 1 loading)
    pub fn register_skill_metadata(&self, manifest: SkillManifest) -> Result<()> {
        let name = manifest.name.clone();

        let mut inner = self.inner.lock().unwrap_or_else(|poisoned| {
            warn!("ContextManager lock poisoned while registering skill metadata; recovering");
            poisoned.into_inner()
        });

        let entry = SkillContextEntry {
            name: name.clone(),
            level: ContextLevel::Metadata,
            manifest: manifest.clone(),
            instructions: None,
            skill: None,
            usage: ContextUsage {
                access_count: 0,
                last_access: std::time::Instant::now(),
                total_loaded_duration: std::time::Duration::ZERO,
                token_cost: self.config.metadata_token_cost,
            },
            memory_size: std::mem::size_of::<SkillContextEntry>()
                + name.len()
                + manifest.description.len(),
        };

        // Update token usage
        inner.current_token_usage += self.config.metadata_token_cost;
        inner.stats.current_token_usage = inner.current_token_usage;
        inner.stats.peak_token_usage = inner.stats.peak_token_usage.max(inner.current_token_usage);

        inner.active_skills.insert(name, entry);
        info!("Registered skill metadata: {}", manifest.name);

        Ok(())
    }

    /// Load skill instructions (Level 2 loading)
    pub fn load_skill_instructions(&self, name: &str, instructions: String) -> Result<()> {
        let mut inner = self.inner.lock().unwrap_or_else(|poisoned| {
            warn!("ContextManager lock poisoned while loading skill instructions; recovering");
            poisoned.into_inner()
        });

        // Calculate simple size metric (characters) instead of tokens
        let instruction_size = instructions.len();

        // Check context budget (using character count instead of tokens)
        if inner.current_token_usage + instruction_size > self.config.max_context_tokens {
            // Need to evict skills to make room
            self.evict_skills_to_make_room_internal(&mut inner, instruction_size)?;
        }

        // Get or create entry
        let mut entry = match inner.loaded_skills.get_mut(name) {
            Some(entry) => entry.clone(),
            None => {
                // Create new entry from active skills
                match inner.active_skills.get(name) {
                    Some(active_entry) => active_entry.clone(),
                    None => return Err(anyhow!("Skill '{}' not found in active skills", name)),
                }
            }
        };

        // Update entry
        entry.level = ContextLevel::Instructions;
        entry.instructions = Some(instructions.clone());
        entry.usage.token_cost = instruction_size;
        entry.memory_size += instructions.len();

        // Update usage
        inner.current_token_usage += instruction_size;
        inner.stats.current_token_usage = inner.current_token_usage;
        inner.stats.peak_token_usage = inner.stats.peak_token_usage.max(inner.current_token_usage);
        inner.stats.total_tokens_loaded += instruction_size as u64;

        // Cache the entry
        inner.loaded_skills.put(name.to_string(), entry);

        info!(
            "Loaded instructions for skill: {} ({} chars)",
            name, instruction_size
        );

        Ok(())
    }

    /// Load full skill with resources (Level 3 loading)
    pub fn load_full_skill(&self, skill: Skill) -> Result<()> {
        let name = skill.name().to_string();
        let mut inner = self.inner.lock().unwrap_or_else(|poisoned| {
            warn!("ContextManager lock poisoned while loading full skill; recovering");
            poisoned.into_inner()
        });

        // Calculate size-based cost for resources and instructions (characters instead of tokens)
        let instruction_size = skill.instructions.len();
        let resource_size = skill.list_resources().len() * self.config.resource_token_cost * 4; // Approximate
        let incremental_cost = instruction_size + resource_size;

        // Check context budget
        if inner.current_token_usage + incremental_cost > self.config.max_context_tokens {
            self.evict_skills_to_make_room_internal(&mut inner, incremental_cost)?;
        }

        // Create entry
        let entry = SkillContextEntry {
            name: name.clone(),
            level: ContextLevel::Full,
            manifest: skill.manifest.clone(),
            instructions: Some(skill.instructions.clone()),
            skill: Some(skill),
            usage: ContextUsage {
                access_count: 0,
                last_access: std::time::Instant::now(),
                total_loaded_duration: std::time::Duration::ZERO,
                token_cost: incremental_cost,
            },
            memory_size: std::mem::size_of::<Skill>() + name.len() * 2,
        };

        // Update usage
        inner.current_token_usage += incremental_cost;
        inner.stats.current_token_usage = inner.current_token_usage;
        inner.stats.peak_token_usage = inner.stats.peak_token_usage.max(inner.current_token_usage);
        inner.stats.total_skills_loaded += 1;
        inner.stats.total_tokens_loaded += incremental_cost as u64;

        // Cache the entry
        let entry_name = entry.name.clone();
        inner.loaded_skills.put(name, entry);

        info!(
            "Loaded full skill: {} ({} tokens)",
            entry_name,
            incremental_cost + self.config.metadata_token_cost
        );

        Ok(())
    }

    /// Get skill context (with automatic loading)
    pub fn get_skill_context(&self, name: &str) -> Option<SkillContextEntry> {
        let mut inner = self.inner.lock().unwrap_or_else(|poisoned| {
            warn!("ContextManager lock poisoned while fetching skill context; recovering");
            poisoned.into_inner()
        });

        // Try loaded skills first
        if let Some(mut entry) = inner.loaded_skills.get_mut(name).cloned() {
            entry.usage.access_count += 1;
            entry.usage.last_access = std::time::Instant::now();
            inner.stats.cache_hits += 1;
            return Some(entry);
        }

        // Fall back to active skills (metadata only)
        if let Some(mut entry) = inner.active_skills.get(name).cloned() {
            entry.usage.access_count += 1;
            entry.usage.last_access = std::time::Instant::now();
            inner.stats.cache_misses += 1;
            return Some(entry);
        }

        None
    }

    /// Evict skills to make room for new ones
    fn evict_skills_to_make_room_internal(
        &self,
        inner: &mut ContextManagerInner,
        required_tokens: usize,
    ) -> Result<()> {
        let mut freed_tokens = 0;
        let mut evicted_skills = Vec::new();

        // Use LRU eviction
        while freed_tokens < required_tokens && !inner.loaded_skills.is_empty() {
            if let Some((name, entry)) = inner.loaded_skills.pop_lru() {
                freed_tokens += entry.usage.token_cost;
                evicted_skills.push(name);

                inner.stats.total_skills_evicted += 1;
                inner.stats.total_tokens_evicted += entry.usage.token_cost as u64;
            } else {
                break;
            }
        }

        inner.current_token_usage -= freed_tokens;
        inner.stats.current_token_usage = inner.current_token_usage;

        info!(
            "Evicted {} skills to free {} tokens",
            evicted_skills.len(),
            freed_tokens
        );
        debug!("Evicted skills: {:?}", evicted_skills);

        if freed_tokens < required_tokens {
            return Err(anyhow!(
                "Unable to free enough tokens. Required: {}, Freed: {}",
                required_tokens,
                freed_tokens
            ));
        }

        Ok(())
    }

    /// Get current context usage statistics
    pub fn get_stats(&self) -> ContextStats {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| {
                warn!("ContextManager lock poisoned while reading stats; recovering");
                poisoned.into_inner()
            })
            .stats
            .clone()
    }

    /// Get current token usage
    pub fn get_token_usage(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| {
                warn!("ContextManager lock poisoned while reading token usage; recovering");
                poisoned.into_inner()
            })
            .current_token_usage
    }

    /// Clear all loaded skills (keep metadata)
    pub fn clear_loaded_skills(&self) {
        let mut inner = self.inner.lock().unwrap_or_else(|poisoned| {
            warn!("ContextManager lock poisoned while clearing loaded skills; recovering");
            poisoned.into_inner()
        });

        let evicted_count = inner.loaded_skills.len();
        let evicted_tokens = inner.stats.current_token_usage
            - (inner.active_skills.len() * self.config.metadata_token_cost);

        inner.loaded_skills.clear();
        inner.current_token_usage = inner.active_skills.len() * self.config.metadata_token_cost;
        inner.stats.current_token_usage = inner.current_token_usage;
        inner.stats.total_skills_evicted += evicted_count as u64;
        inner.stats.total_tokens_evicted += evicted_tokens as u64;

        info!(
            "Cleared {} loaded skills ({} tokens)",
            evicted_count, evicted_tokens
        );
    }

    /// Get all active skill names
    pub fn get_active_skills(&self) -> Vec<String> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| {
                warn!("ContextManager lock poisoned while reading active skills; recovering");
                poisoned.into_inner()
            })
            .active_skills
            .keys()
            .cloned()
            .collect()
    }

    /// Get memory usage estimate
    pub fn get_memory_usage(&self) -> usize {
        let inner = self.inner.lock().unwrap_or_else(|poisoned| {
            warn!("ContextManager lock poisoned while calculating memory usage; recovering");
            poisoned.into_inner()
        });
        let active_memory: usize = inner
            .active_skills
            .values()
            .map(|entry| entry.memory_size)
            .sum();

        let loaded_memory: usize = inner
            .loaded_skills
            .iter()
            .map(|(_, entry)| entry.memory_size)
            .sum();

        active_memory + loaded_memory
    }
}

/// Context manager with persistence support
pub struct PersistentContextManager {
    inner: ContextManager,
    cache_path: PathBuf,
}

impl PersistentContextManager {
    /// Create new persistent context manager
    pub fn new(cache_path: PathBuf, config: ContextConfig) -> Result<Self> {
        let mut manager = Self {
            inner: ContextManager::with_config(config),
            cache_path,
        };

        // Try to load cached state
        if let Err(e) = manager.load_cache() {
            debug!("Failed to load context cache: {}", e);
        }

        Ok(manager)
    }

    /// Load cached context state
    fn load_cache(&mut self) -> Result<()> {
        if !self.cache_path.exists() {
            return Ok(());
        }

        let cache: ContextCache = read_json_file_sync(&self.cache_path)?;

        // Restore active skills
        let skill_count = cache.active_skills.len();
        for manifest in cache.active_skills {
            self.inner.register_skill_metadata(manifest)?;
        }

        info!("Loaded {} cached skills", skill_count);
        Ok(())
    }

    /// Save context state to cache
    pub fn save_cache(&self) -> Result<()> {
        let inner = self
            .inner
            .inner
            .lock()
            .map_err(|err| anyhow!("context manager lock poisoned while saving cache: {err}"))
            .context("Failed to save context manager cache state")?;
        let cache = ContextCache {
            version: 1,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs(),
            active_skills: inner
                .active_skills
                .values()
                .map(|entry| entry.manifest.clone())
                .collect(),
        };

        write_json_file_sync(&self.cache_path, &cache)?;

        info!("Saved {} skills to cache", cache.active_skills.len());
        Ok(())
    }

    /// Get inner context manager
    pub fn inner(&self) -> &ContextManager {
        &self.inner
    }

    /// Get mutable inner context manager
    pub fn inner_mut(&mut self) -> &mut ContextManager {
        &mut self.inner
    }
}

/// Cache structure for persistence
#[derive(Debug, Serialize, Deserialize)]
struct ContextCache {
    version: u32,
    timestamp: u64,
    active_skills: Vec<SkillManifest>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_config_default() {
        let config = ContextConfig::default();
        assert_eq!(config.max_context_tokens, 50_000);
        assert_eq!(config.max_cached_skills, 100);
    }

    #[test]
    fn test_context_manager_creation() {
        let manager = ContextManager::new();
        assert_eq!(manager.get_token_usage(), 0);
        assert_eq!(manager.get_active_skills().len(), 0);
    }

    #[test]
    fn test_skill_metadata_registration() {
        let manager = ContextManager::new();

        let manifest = SkillManifest {
            name: "test-skill".to_string(),
            description: "Test skill".to_string(),
            version: Some("1.0.0".to_string()),
            author: Some("Test".to_string()),
            vtcode_native: Some(true),
            ..Default::default()
        };

        assert!(manager.register_skill_metadata(manifest).is_ok());
        assert_eq!(manager.get_active_skills().len(), 1);
        assert_eq!(manager.get_token_usage(), 50); // metadata_token_cost
    }

    #[test]
    fn test_skill_context_retrieval() {
        let manager = ContextManager::new();

        let manifest = SkillManifest {
            name: "test-skill".to_string(),
            description: "Test skill".to_string(),
            ..Default::default()
        };

        manager.register_skill_metadata(manifest.clone()).unwrap();

        let context = manager.get_skill_context("test-skill");
        assert!(context.is_some());
        assert_eq!(context.unwrap().manifest.name, "test-skill");
    }
}
