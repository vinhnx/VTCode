use crate::acp::permission_cache::PermissionGrant;
pub use crate::acp::permission_cache::ToolPermissionCache as PermissionCache;
use crate::cache::{CacheKey, EvictionPolicy, UnifiedCache};
use crate::tools::shell::ShellOutput;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::sync::{Mutex as TokioMutex, oneshot};
use vtcode_config::CommandCacheConfig;

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
struct CommandCacheKey {
    command: String,
    cwd: PathBuf,
}

impl CacheKey for CommandCacheKey {
    fn to_cache_key(&self) -> String {
        format!("{}::{}", self.command, self.cwd.display())
    }
}

struct CommandCache {
    config: Mutex<CommandCacheConfig>,
    cache: Mutex<UnifiedCache<CommandCacheKey, ShellOutput>>,
}

static COMMAND_CACHE: Lazy<CommandCache> =
    Lazy::new(|| CommandCache::new(CommandCacheConfig::default()));

pub type InFlightResult = Result<ShellOutput, String>;

pub struct InFlightToken(CommandCacheKey);

pub enum InFlightState {
    Owner(InFlightToken),
    Wait(oneshot::Receiver<InFlightResult>),
}

static IN_FLIGHT: Lazy<TokioMutex<HashMap<CommandCacheKey, Vec<oneshot::Sender<InFlightResult>>>>> =
    Lazy::new(|| TokioMutex::new(HashMap::new()));

impl PermissionCache {
    pub fn get(&mut self, key: &str) -> Option<bool> {
        match self.get_permission(key) {
            Some(PermissionGrant::Denied) => Some(false),
            Some(_) => Some(true),
            None => None,
        }
    }

    pub fn put(&mut self, key: &str, allowed: bool, _reason: &str) {
        let grant = if allowed {
            PermissionGrant::Session
        } else {
            PermissionGrant::Denied
        };
        self.cache_grant(key.to_string(), grant);
    }
}

impl CommandCache {
    fn new(config: CommandCacheConfig) -> Self {
        let cache = Self::build_cache(&config);
        Self {
            config: Mutex::new(config),
            cache: Mutex::new(cache),
        }
    }

    fn build_cache(config: &CommandCacheConfig) -> UnifiedCache<CommandCacheKey, ShellOutput> {
        UnifiedCache::new(
            config.max_entries.max(1),
            std::time::Duration::from_millis(config.ttl_ms),
            EvictionPolicy::Lru,
        )
    }

    fn configure(&self, config: &CommandCacheConfig) {
        *self.config.lock() = config.clone();
        *self.cache.lock() = Self::build_cache(config);
    }

    fn allowlisted(&self, command: &str) -> bool {
        let cfg = self.config.lock();
        if !cfg.enabled {
            return false;
        }
        let trimmed = command.trim();
        cfg.allowlist.iter().any(|entry| {
            let entry = entry.trim();
            trimmed == entry || trimmed.starts_with(&format!("{entry} "))
        })
    }

    fn get(&self, command: &str, cwd: &Path) -> Option<ShellOutput> {
        if !self.allowlisted(command) {
            return None;
        }
        let key = CommandCacheKey {
            command: command.to_string(),
            cwd: cwd.to_path_buf(),
        };
        self.cache.lock().get_owned(&key)
    }

    fn put(&self, command: &str, cwd: &Path, output: ShellOutput) {
        if !self.allowlisted(command) || output.exit_code != 0 {
            return;
        }
        let key = CommandCacheKey {
            command: command.to_string(),
            cwd: cwd.to_path_buf(),
        };
        let size = (output.stdout.len() + output.stderr.len()) as u64;
        self.cache.lock().insert(key, output, size);
    }
}

pub fn configure_command_cache(config: &CommandCacheConfig) {
    COMMAND_CACHE.configure(config);
}

pub fn get_cached_output(command: &str, cwd: &Path) -> Option<ShellOutput> {
    COMMAND_CACHE.get(command, cwd)
}

pub fn cache_output(command: &str, cwd: &Path, output: ShellOutput) {
    COMMAND_CACHE.put(command, cwd, output);
}

pub async fn enter_inflight(command: &str, cwd: &Path) -> Option<InFlightState> {
    if !COMMAND_CACHE.allowlisted(command) {
        return None;
    }

    let key = CommandCacheKey {
        command: command.to_string(),
        cwd: cwd.to_path_buf(),
    };

    let mut inflight = IN_FLIGHT.lock().await;
    if let Some(waiters) = inflight.get_mut(&key) {
        let (tx, rx) = oneshot::channel();
        waiters.push(tx);
        return Some(InFlightState::Wait(rx));
    }

    inflight.insert(key.clone(), Vec::new());
    Some(InFlightState::Owner(InFlightToken(key)))
}

pub async fn finish_inflight(token: InFlightToken, result: InFlightResult) {
    let key = token.0;
    let mut inflight = IN_FLIGHT.lock().await;
    if let Some(waiters) = inflight.remove(&key) {
        for waiter in waiters {
            let _ = waiter.send(result.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> CommandCacheConfig {
        CommandCacheConfig {
            enabled: true,
            ttl_ms: 10_000,
            max_entries: 8,
            allowlist: vec!["git status".to_string(), "echo".to_string()],
        }
    }

    #[test]
    fn allowlist_matches_prefix() {
        let cache = CommandCache::new(test_config());
        assert!(cache.allowlisted("git status"));
        assert!(cache.allowlisted("git status -s"));
        assert!(!cache.allowlisted("git diff"));
    }

    #[test]
    fn cache_stores_only_successes() {
        let cache = CommandCache::new(test_config());
        let cwd = Path::new("/tmp");

        let failed = ShellOutput {
            stdout: "nope".to_string(),
            stderr: "err".to_string(),
            exit_code: 1,
        };
        cache.put("echo bad", cwd, failed);
        assert!(cache.get("echo bad", cwd).is_none());

        let ok = ShellOutput {
            stdout: "ok".to_string(),
            stderr: String::new(),
            exit_code: 0,
        };
        cache.put("echo ok", cwd, ok.clone());
        let cached = cache.get("echo ok", cwd).expect("cached output");
        assert_eq!(cached.stdout, ok.stdout);
    }
}
