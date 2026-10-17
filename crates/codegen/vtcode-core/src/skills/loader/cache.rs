use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, SystemTime};

use hashbrown::HashMap;
use tracing::warn;

use super::SkillLoaderConfig;
use crate::skills::model::SkillLoadOutcome;

const LIGHTWEIGHT_SKILL_CACHE_TTL: Duration = Duration::from_secs(5 * 60);
const LIGHTWEIGHT_SKILL_CACHE_MAX_ENTRIES: usize = 32;

static LIGHTWEIGHT_SKILL_METADATA_CACHE: OnceLock<
    RwLock<HashMap<LightweightSkillCacheKey, CachedLightweightSkillOutcome>>,
> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct LightweightSkillCacheKey {
    codex_home: PathBuf,
    cwd: PathBuf,
    project_root: Option<PathBuf>,
    include_bundled_system_skills: bool,
    home_dir: Option<PathBuf>,
}

impl LightweightSkillCacheKey {
    pub(crate) fn new(config: &SkillLoaderConfig, home_dir: Option<&Path>) -> Self {
        Self {
            codex_home: normalize_cache_path(&config.codex_home),
            cwd: normalize_cache_path(&config.cwd),
            project_root: config.project_root.as_deref().map(normalize_cache_path),
            include_bundled_system_skills: config.include_bundled_system_skills,
            home_dir: home_dir.map(normalize_cache_path),
        }
    }
}

#[derive(Clone)]
struct CachedLightweightSkillOutcome {
    outcome: SkillLoadOutcome,
    timestamp: SystemTime,
}

impl CachedLightweightSkillOutcome {
    fn is_expired(&self) -> bool {
        self.timestamp.elapsed().unwrap_or(LIGHTWEIGHT_SKILL_CACHE_TTL) > LIGHTWEIGHT_SKILL_CACHE_TTL
    }
}

fn normalize_cache_path(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn lightweight_skill_metadata_cache()
-> &'static RwLock<HashMap<LightweightSkillCacheKey, CachedLightweightSkillOutcome>> {
    LIGHTWEIGHT_SKILL_METADATA_CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

pub(crate) fn get_cached_lightweight_skill_outcome(key: &LightweightSkillCacheKey) -> Option<SkillLoadOutcome> {
    match lightweight_skill_metadata_cache().read() {
        Ok(cache) => cache
            .get(key)
            .filter(|cached| !cached.is_expired())
            .map(|cached| cached.outcome.clone()),
        Err(_) => {
            warn!("lightweight skill metadata cache lock poisoned while reading cache");
            None
        }
    }
}

pub(crate) fn cache_lightweight_skill_outcome(key: LightweightSkillCacheKey, outcome: &SkillLoadOutcome) {
    match lightweight_skill_metadata_cache().write() {
        Ok(mut cache) => {
            if cache.len() >= LIGHTWEIGHT_SKILL_CACHE_MAX_ENTRIES && !cache.contains_key(&key) {
                let expired: Vec<_> = cache
                    .iter()
                    .filter(|(_, value)| value.is_expired())
                    .map(|(cache_key, _)| cache_key.clone())
                    .collect();

                for cache_key in expired {
                    cache.remove(&cache_key);
                }

                if cache.len() >= LIGHTWEIGHT_SKILL_CACHE_MAX_ENTRIES {
                    let oldest_key = cache
                        .iter()
                        .min_by_key(|(_, value)| value.timestamp)
                        .map(|(cache_key, _)| cache_key.clone());
                    if let Some(oldest_key) = oldest_key {
                        cache.remove(&oldest_key);
                    }
                }
            }

            cache.insert(
                key,
                CachedLightweightSkillOutcome {
                    outcome: outcome.clone(),
                    timestamp: SystemTime::now(),
                },
            );
        }
        Err(_) => warn!("lightweight skill metadata cache lock poisoned while writing cache"),
    }
}

pub(crate) fn clear_lightweight_skill_metadata_cache() {
    match lightweight_skill_metadata_cache().write() {
        Ok(mut cache) => cache.clear(),
        Err(_) => warn!("lightweight skill metadata cache lock poisoned while clearing cache"),
    }
}
