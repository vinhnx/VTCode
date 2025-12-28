use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::info;

use super::registration::{ToolMetadata, ToolRegistration};
use crate::tools::command::CommandTool;
use crate::tools::file_ops::FileOpsTool;
use crate::tools::grep_file::GrepSearchManager;

/// Metrics for alias usage tracking
#[derive(Debug, Default, Clone)]
pub struct AliasMetrics {
    /// Map of alias name to (canonical_name, usage_count)
    pub usage: HashMap<String, (String, u64)>,
}

#[derive(Debug, Clone)]
struct ToolCacheEntry {
    registration: ToolRegistration,
    last_used: Instant,
    use_count: u64,
}

#[derive(Clone)]
pub(super) struct ToolInventory {
    workspace_root: PathBuf,
    tools: HashMap<String, ToolCacheEntry>,
    /// Map of lowercase alias name to canonical tool name
    aliases: HashMap<String, String>,
    frequently_used: HashSet<String>,
    last_cache_cleanup: Instant,
    /// HP-7: Maintain sorted list of tool names for O(1) available_tools() calls
    sorted_names: Vec<String>,
    /// Track alias usage for analytics and debugging
    alias_metrics: Arc<std::sync::Mutex<AliasMetrics>>,

    // Common tools that are used frequently
    file_ops_tool: FileOpsTool,
    command_tool: CommandTool,
    grep_search: Arc<GrepSearchManager>,
}

impl ToolInventory {
    pub fn new(workspace_root: PathBuf) -> Self {
        // Clone once for command_tool (needs ownership), share reference for others
        let command_tool = CommandTool::new(workspace_root.clone());
        let grep_search = Arc::new(GrepSearchManager::new(workspace_root.clone()));
        let file_ops_tool = FileOpsTool::new(workspace_root.clone(), Arc::clone(&grep_search));

        Self {
            workspace_root,
            tools: HashMap::new(),
            aliases: HashMap::new(),
            frequently_used: HashSet::new(),
            last_cache_cleanup: Instant::now(),
            sorted_names: Vec::new(),
            alias_metrics: Arc::new(std::sync::Mutex::new(AliasMetrics::default())),
            file_ops_tool,
            command_tool,
            grep_search,
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
    pub fn command_tool(&self) -> &CommandTool {
        &self.command_tool
    }

    pub fn command_tool_mut(&mut self) -> &mut CommandTool {
        &mut self.command_tool
    }

    pub fn grep_file_manager(&self) -> Arc<GrepSearchManager> {
        self.grep_search.clone()
    }

    pub fn register_tool(&mut self, registration: ToolRegistration) -> anyhow::Result<()> {
        let name = registration.name().to_owned();
        let aliases = registration.metadata().aliases().to_vec();

        // Use entry API to check and insert in one operation
        use std::collections::hash_map::Entry;
        match self.tools.entry(name.clone()) {
            Entry::Occupied(_) => {
                return Err(anyhow::anyhow!("Tool '{}' is already registered", name));
            }
            Entry::Vacant(entry) => {
                entry.insert(ToolCacheEntry {
                    registration,
                    last_used: Instant::now(),
                    use_count: 0,
                });
                // HP-7: Maintain sorted invariant - insert at correct position
                let pos = self.sorted_names.binary_search(&name).unwrap_or_else(|e| e);
                self.sorted_names.insert(pos, name.clone());
            }
        }

        // Add to frequently used set if it's a common tool
        if self.is_common_tool(&name) {
            self.frequently_used.insert(name.clone());
        }

        // Register case-insensitive aliases and track metrics
        if !aliases.is_empty() {
            self.register_aliases(&name, &aliases);
        }

        // Clean up old cache entries if needed
        self.cleanup_cache_if_needed();

        // Validate aliases don't conflict with existing tool names (case-insensitive)
        for alias in &aliases {
            if self.tools.contains_key(alias) {
                return Err(anyhow::anyhow!(
                    "Cannot register alias '{}' for tool '{}': alias conflicts with existing tool name",
                    alias,
                    name
                ));
            }
            // Also check if it conflicts with an existing alias
            let alias_lower = alias.to_ascii_lowercase();
            if self.aliases.contains_key(&alias_lower) {
                let existing_target = self.aliases.get(&alias_lower).unwrap();
                return Err(anyhow::anyhow!(
                    "Cannot register alias '{}' for tool '{}': alias already exists for tool '{}'",
                    alias,
                    name,
                    existing_target
                ));
            }
        }

        Ok(())
    }

    /// Register case-insensitive aliases for a tool and track metrics
    fn register_aliases(&mut self, tool_name: &str, aliases: &[String]) {
        for alias in aliases {
            let alias_lower = alias.to_ascii_lowercase();
            let target = tool_name.to_owned();

            // Store lowercase -> canonical mapping
            self.aliases.insert(alias_lower.clone(), target.clone());

            // Initialize metrics for this alias
            self.alias_metrics
                .lock()
                .unwrap()
                .usage
                .insert(alias_lower, (target, 0));
        }
    }

    pub fn registration_for(&mut self, name: &str) -> Option<&mut ToolRegistration> {
        // Check if name exists directly or resolve via case-insensitive alias
        let name_lower = name.to_ascii_lowercase();
        let resolved_name = if self.tools.contains_key(name) {
            name.to_owned()
        } else if let Some(aliased) = self.aliases.get(&name_lower) {
            // Track alias usage metrics
            if let Some((canonical, count)) = self
                .alias_metrics
                .lock()
                .unwrap()
                .usage
                .get_mut(&name_lower)
            {
                *count += 1;
                info!(
                    alias = %name,
                    canonical = %canonical,
                    count = *count,
                    "Tool alias resolved and usage tracked"
                );
            }
            aliased.clone()
        } else {
            return None;
        };

        // Now get_mut with the resolved name
        if let Some(entry) = self.tools.get_mut(&resolved_name) {
            entry.last_used = Instant::now();
            entry.use_count += 1;
            // Track frequently used for aliased tools
            if resolved_name != name {
                self.frequently_used.insert(resolved_name);
            }
            return Some(&mut entry.registration);
        }

        None
    }

    /// Get a tool registration without updating usage metrics
    pub fn get_registration(&self, name: &str) -> Option<&ToolRegistration> {
        let name_lower = name.to_ascii_lowercase();
        let resolved_name = if self.tools.contains_key(name) {
            name
        } else if let Some(aliased) = self.aliases.get(&name_lower) {
            aliased
        } else {
            return None;
        };

        self.tools
            .get(resolved_name)
            .map(|entry| &entry.registration)
    }

    pub fn has_tool(&self, name: &str) -> bool {
        let name_lower = name.to_ascii_lowercase();
        self.tools.contains_key(name) || self.aliases.contains_key(&name_lower)
    }

    pub fn available_tools(&self) -> &[String] {
        // HP-7: O(1) - already sorted, just return reference
        &self.sorted_names
    }

    pub fn registered_aliases(&self) -> Vec<String> {
        self.aliases.keys().cloned().collect()
    }

    /// Get all registered aliases with their canonical targets
    #[allow(dead_code)]
    pub fn all_aliases(&self) -> Vec<(String, String)> {
        self.aliases
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// Snapshot registration metadata for policy/catalog synchronization
    pub fn registration_metadata(&self) -> Vec<(String, ToolMetadata)> {
        self.tools
            .iter()
            .map(|(name, entry)| (name.clone(), entry.registration.metadata().clone()))
            .collect()
    }

    /// Check if a tool is commonly used
    fn is_common_tool(&self, name: &str) -> bool {
        matches!(name, "file_ops" | "command" | "grep" | "plan")
    }

    /// Clean up old cache entries if needed
    fn cleanup_cache_if_needed(&mut self) {
        const CACHE_CLEANUP_INTERVAL: Duration = Duration::from_secs(300); // 5 minutes
        const MAX_TOOLS: usize = 1000;

        // Only clean up if enough time has passed and we have many tools
        if self.last_cache_cleanup.elapsed() < CACHE_CLEANUP_INTERVAL
            || self.tools.len() < MAX_TOOLS
        {
            return;
        }

        let now = Instant::now();
        let old_len = self.tools.len();

        // Remove tools that haven't been used in a while and aren't frequently used
        self.tools.retain(|name, entry| {
            // Keep frequently used tools
            if self.frequently_used.contains(name) {
                return true;
            }

            // Keep tools used recently
            now.duration_since(entry.last_used) < Duration::from_secs(3600) // 1 hour
        });

        self.last_cache_cleanup = now;

        if self.tools.len() < old_len {
            tracing::debug!(
                "Cleaned up {} unused tools from cache",
                old_len - self.tools.len()
            );
        }
    }
}
