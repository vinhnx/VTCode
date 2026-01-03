use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::info;

use super::registration::{ToolMetadata, ToolRegistration};
use crate::exec::skill_manager::SkillManager;
use crate::tools::code_intelligence::CodeIntelligenceTool;
use crate::tools::command::CommandTool;
use crate::tools::file_ops::FileOpsTool;
use crate::tools::grep_file::GrepSearchManager;

/// Metrics for alias usage tracking
#[derive(Debug, Default, Clone)]
pub struct AliasMetrics {
    /// Map of alias name to (canonical_name, usage_count)
    pub usage: HashMap<String, (String, u64)>,
}

#[derive(Debug)]
struct ToolCacheEntry {
    registration: ToolRegistration,
    last_used: std::sync::RwLock<Instant>,
    use_count: std::sync::atomic::AtomicU64,
}

#[derive(Clone)]
pub(super) struct ToolInventory {
    workspace_root: PathBuf,
    tools: Arc<std::sync::RwLock<HashMap<String, Arc<ToolCacheEntry>>>>,
    /// Map of lowercase alias name to canonical tool name
    aliases: Arc<std::sync::RwLock<HashMap<String, String>>>,
    frequently_used: Arc<std::sync::RwLock<HashSet<String>>>,
    last_cache_cleanup: Arc<std::sync::RwLock<Instant>>,
    /// HP-7: Maintain sorted list of tool names for O(1) available_tools() calls
    sorted_names: Arc<std::sync::RwLock<Vec<String>>>,
    /// Track alias usage for analytics and debugging
    alias_metrics: Arc<std::sync::Mutex<AliasMetrics>>,

    // Common tools that are used frequently
    file_ops_tool: FileOpsTool,
    command_tool: Arc<std::sync::RwLock<CommandTool>>,
    grep_search: Arc<GrepSearchManager>,
    code_intelligence: CodeIntelligenceTool,
    skill_manager: SkillManager,
}

impl ToolInventory {
    pub fn new(workspace_root: PathBuf) -> Self {
        // Clone once for command_tool (needs ownership), share reference for others
        let command_tool = CommandTool::new(workspace_root.clone());
        let grep_search = Arc::new(GrepSearchManager::new(workspace_root.clone()));
        let file_ops_tool = FileOpsTool::new(workspace_root.clone(), Arc::clone(&grep_search));
        let code_intelligence = CodeIntelligenceTool::new(workspace_root.clone());
        let skill_manager = SkillManager::new(&workspace_root);

        Self {
            workspace_root,
            tools: Arc::new(std::sync::RwLock::new(HashMap::new())),
            aliases: Arc::new(std::sync::RwLock::new(HashMap::new())),
            frequently_used: Arc::new(std::sync::RwLock::new(HashSet::new())),
            last_cache_cleanup: Arc::new(std::sync::RwLock::new(Instant::now())),
            sorted_names: Arc::new(std::sync::RwLock::new(Vec::new())),
            alias_metrics: Arc::new(std::sync::Mutex::new(AliasMetrics::default())),
            file_ops_tool,
            command_tool: Arc::new(std::sync::RwLock::new(command_tool)),
            grep_search,
            code_intelligence,
            skill_manager,
        }
    }

    /// Get alias usage metrics for debugging and analytics
    #[allow(dead_code)]
    pub fn alias_metrics(&self) -> AliasMetrics {
        self.alias_metrics.lock().unwrap().clone()
    }

    /// Reset alias metrics
    #[allow(dead_code)]
    pub fn reset_alias_metrics(&self) {
        *self.alias_metrics.lock().unwrap() = AliasMetrics::default();
    }

    pub fn workspace_root(&self) -> &PathBuf {
        &self.workspace_root
    }

    pub fn file_ops_tool(&self) -> &FileOpsTool {
        &self.file_ops_tool
    }

    #[allow(dead_code)]
    pub fn command_tool(&self) -> Arc<std::sync::RwLock<CommandTool>> {
        self.command_tool.clone()
    }

    pub fn grep_file_manager(&self) -> Arc<GrepSearchManager> {
        self.grep_search.clone()
    }

    pub fn code_intelligence_tool(&self) -> &CodeIntelligenceTool {
        &self.code_intelligence
    }

    pub fn skill_manager(&self) -> &SkillManager {
        &self.skill_manager
    }

    pub fn register_tool(&self, registration: ToolRegistration) -> anyhow::Result<()> {
        let name = registration.name().to_owned();
        let aliases = registration.metadata().aliases().to_vec();

        {
            let tools = self.tools.read().unwrap();
            let aliases_map = self.aliases.read().unwrap();

            // Validate aliases don't conflict with existing tool names BEFORE registration
            for alias in &aliases {
                if tools.contains_key(alias) {
                    return Err(anyhow::anyhow!(
                        "Cannot register alias '{}' for tool '{}': alias conflicts with existing tool name",
                        alias,
                        name
                    ));
                }
                // Also check if it conflicts with an existing alias
                let alias_lower = alias.to_ascii_lowercase();
                if let Some(existing_target) = aliases_map.get(&alias_lower) {
                    // Only warn if the alias is being registered for a DIFFERENT tool
                    if existing_target != &name {
                        return Err(anyhow::anyhow!(
                            "Cannot register alias '{}' for tool '{}': alias already exists for tool '{}'",
                            alias,
                            name,
                            existing_target
                        ));
                    }
                    // If it's the same tool being re-registered, just skip it silently
                    continue;
                }
            }
        }

        // Use entry API to check and insert in one operation
        {
            let mut tools = self.tools.write().unwrap();
            use std::collections::hash_map::Entry;
            match tools.entry(name.clone()) {
                Entry::Occupied(_) => {
                    return Err(anyhow::anyhow!("Tool '{}' is already registered", name));
                }
                Entry::Vacant(entry) => {
                    entry.insert(Arc::new(ToolCacheEntry {
                        registration,
                        last_used: std::sync::RwLock::new(Instant::now()),
                        use_count: std::sync::atomic::AtomicU64::new(0),
                    }));
                    // HP-7: Maintain sorted invariant - insert at correct position
                    let mut sorted = self.sorted_names.write().unwrap();
                    let pos = sorted.binary_search(&name).unwrap_or_else(|e| e);
                    sorted.insert(pos, name.clone());
                }
            }
        }

        // Add to frequently used set if it's a common tool
        if self.is_common_tool(&name) {
            self.frequently_used.write().unwrap().insert(name.clone());
        }

        // Register case-insensitive aliases and track metrics
        if !aliases.is_empty() {
            self.register_aliases(&name, &aliases);
        }

        // Clean up old cache entries if needed
        self.cleanup_cache_if_needed();

        Ok(())
    }

    /// Register case-insensitive aliases for a tool and track metrics
    fn register_aliases(&self, tool_name: &str, aliases: &[String]) {
        let mut aliases_map = self.aliases.write().unwrap();
        let mut metrics = self.alias_metrics.lock().unwrap();
        for alias in aliases {
            let alias_lower = alias.to_ascii_lowercase();
            let target = tool_name.to_owned();

            // Store lowercase -> canonical mapping
            aliases_map.insert(alias_lower.clone(), target.clone());

            // Initialize metrics for this alias
            metrics.usage.insert(alias_lower, (target, 0));
        }
    }

    pub fn registration_for(&self, name: &str) -> Option<ToolRegistration> {
        // Check if name exists directly or resolve via case-insensitive alias
        let name_lower = name.to_ascii_lowercase();

        let resolved_name = {
            let tools = self.tools.read().unwrap();
            let aliases = self.aliases.read().unwrap();

            if tools.contains_key(name) {
                name.to_owned()
            } else if let Some(aliased) = aliases.get(&name_lower) {
                // Track alias usage metrics
                let mut metrics = self.alias_metrics.lock().unwrap();
                if let Some((canonical, count)) = metrics.usage.get_mut(&name_lower) {
                    *count += 1;
                    let count_val = *count;
                    let canonical_val = canonical.clone();
                    drop(metrics); // Drop lock before logging
                    info!(
                        alias = %name,
                        canonical = %canonical_val,
                        count = count_val,
                        "Tool alias resolved and usage tracked"
                    );
                }
                aliased.clone()
            } else {
                return None;
            }
        };

        // Now get with the resolved name
        let tools = self.tools.read().unwrap();
        if let Some(entry) = tools.get(&resolved_name) {
            if let Ok(mut last) = entry.last_used.write() {
                *last = Instant::now();
            }
            entry
                .use_count
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

            // Track frequently used for aliased tools
            if resolved_name != name {
                self.frequently_used.write().unwrap().insert(resolved_name);
            }
            return Some(entry.registration.clone());
        }

        None
    }

    /// Get a tool registration without updating usage metrics
    pub fn get_registration(&self, name: &str) -> Option<ToolRegistration> {
        let name_lower = name.to_ascii_lowercase();
        let tools = self.tools.read().unwrap();
        let aliases = self.aliases.read().unwrap();
        let resolved_name = if tools.contains_key(name) {
            name
        } else if let Some(aliased) = aliases.get(&name_lower) {
            aliased
        } else {
            return None;
        };

        tools
            .get(resolved_name)
            .map(|entry| entry.registration.clone())
    }

    pub fn has_tool(&self, name: &str) -> bool {
        let name_lower = name.to_ascii_lowercase();
        self.tools.read().unwrap().contains_key(name)
            || self.aliases.read().unwrap().contains_key(&name_lower)
    }

    pub fn available_tools(&self) -> Vec<String> {
        self.sorted_names.read().unwrap().clone()
    }

    pub fn registered_aliases(&self) -> Vec<String> {
        self.aliases.read().unwrap().keys().cloned().collect()
    }

    /// Get all registered aliases with their canonical targets
    #[allow(dead_code)]
    pub fn all_aliases(&self) -> Vec<(String, String)> {
        self.aliases
            .read()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// Snapshot registration metadata for policy/catalog synchronization
    pub fn registration_metadata(&self) -> Vec<(String, ToolMetadata)> {
        self.tools
            .read()
            .unwrap()
            .iter()
            .map(|(name, entry)| (name.clone(), entry.registration.metadata().clone()))
            .collect()
    }

    /// Check if a tool is commonly used
    fn is_common_tool(&self, name: &str) -> bool {
        matches!(name, "file_ops" | "command" | "grep" | "plan")
    }

    /// Clean up old cache entries if needed
    fn cleanup_cache_if_needed(&self) {
        const CACHE_CLEANUP_INTERVAL: Duration = Duration::from_secs(300); // 5 minutes
        const MAX_TOOLS: usize = 1000;

        // Only clean up if enough time has passed
        if self.last_cache_cleanup.read().unwrap().elapsed() < CACHE_CLEANUP_INTERVAL {
            return;
        }

        let mut tools = self.tools.write().unwrap();
        if tools.len() < MAX_TOOLS {
            return;
        }

        let now = Instant::now();
        let old_len = tools.len();

        // Remove tools that haven't been used in a while and aren't frequently used
        tools.retain(|name, entry| {
            // Keep frequently used tools
            if self.frequently_used.read().unwrap().contains(name) {
                return true;
            }

            // Keep tools used recently
            let last_used = *entry.last_used.read().unwrap();
            now.duration_since(last_used) < Duration::from_secs(3600) // 1 hour
        });

        if let Ok(mut last_cleanup) = self.last_cache_cleanup.write() {
            *last_cleanup = now;
        }

        let new_len = tools.len();
        if new_len < old_len {
            tracing::debug!(
                "Cleaned up {} unused tools from cache. Old: {}, New: {}",
                old_len - new_len,
                old_len,
                new_len
            );
        }
    }
}
