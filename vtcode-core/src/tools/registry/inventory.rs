use rustc_hash::{FxHashMap, FxHashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::info;

use super::registration::{ToolMetadata, ToolRegistration};
use crate::exec::skill_manager::SkillManager;
use crate::tools::command::CommandTool;
use crate::tools::file_ops::FileOpsTool;
use crate::tools::grep_file::GrepSearchManager;

/// Metrics for alias usage tracking
#[derive(Debug, Default, Clone)]
pub struct AliasMetrics {
    /// Map of alias name to (canonical_name, usage_count)
    pub usage: FxHashMap<String, (String, u64)>,
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
    tools: Arc<std::sync::RwLock<FxHashMap<String, Arc<ToolCacheEntry>>>>,
    /// Map of lowercase alias name to canonical tool name
    aliases: Arc<std::sync::RwLock<FxHashMap<String, String>>>,
    frequently_used: Arc<std::sync::RwLock<FxHashSet<String>>>,
    last_cache_cleanup: Arc<std::sync::RwLock<Instant>>,
    /// HP-7: Maintain sorted list of tool names for O(1) available_tools() calls
    sorted_names: Arc<std::sync::RwLock<Vec<String>>>,
    /// Track alias usage for analytics and debugging
    alias_metrics: Arc<std::sync::Mutex<AliasMetrics>>,

    // Common tools that are used frequently
    file_ops_tool: FileOpsTool,
    command_tool: Arc<std::sync::RwLock<CommandTool>>,
    grep_search: Arc<GrepSearchManager>,
    skill_manager: SkillManager,
}

impl ToolInventory {
    pub fn new(workspace_root: PathBuf) -> Self {
        // Clone once for command_tool (needs ownership), share reference for others
        let command_tool = CommandTool::new(workspace_root.clone());
        let grep_search = Arc::new(GrepSearchManager::new(workspace_root.clone()));
        let file_ops_tool = FileOpsTool::new(workspace_root.clone(), Arc::clone(&grep_search));
        let skill_manager = SkillManager::new(&workspace_root);

        Self {
            workspace_root,
            tools: Arc::new(std::sync::RwLock::new(FxHashMap::default())),
            aliases: Arc::new(std::sync::RwLock::new(FxHashMap::default())),
            frequently_used: Arc::new(std::sync::RwLock::new(FxHashSet::default())),
            last_cache_cleanup: Arc::new(std::sync::RwLock::new(Instant::now())),
            sorted_names: Arc::new(std::sync::RwLock::new(Vec::new())),
            alias_metrics: Arc::new(std::sync::Mutex::new(AliasMetrics::default())),
            file_ops_tool,
            command_tool: Arc::new(std::sync::RwLock::new(command_tool)),
            grep_search,
            skill_manager,
        }
    }

    /// Get alias usage metrics for debugging and analytics
    #[allow(dead_code)]
    pub fn alias_metrics(&self) -> AliasMetrics {
        self.alias_metrics
            .lock()
            .ok()
            .map(|m| m.clone())
            .unwrap_or_default()
    }

    /// Reset alias metrics
    #[allow(dead_code)]
    pub fn reset_alias_metrics(&self) {
        if let Ok(mut m) = self.alias_metrics.lock() {
            *m = AliasMetrics::default();
        }
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

    pub fn skill_manager(&self) -> &SkillManager {
        &self.skill_manager
    }

    pub fn register_tool(&self, registration: ToolRegistration) -> anyhow::Result<()> {
        let name = registration.name().to_owned();
        let name_lower = name.to_ascii_lowercase();
        let aliases = registration.metadata().aliases().to_vec();

        {
            let tools = self
                .tools
                .read()
                .map_err(|e| anyhow::anyhow!("tool registry read lock poisoned: {e}"))?;
            let aliases_map = self
                .aliases
                .read()
                .map_err(|e| anyhow::anyhow!("alias registry read lock poisoned: {e}"))?;

            // Validate aliases don't conflict with existing tool names BEFORE registration
            for alias in &aliases {
                let alias_lower = alias.to_ascii_lowercase();
                if tools.contains_key(&alias_lower) {
                    return Err(anyhow::anyhow!(
                        "Cannot register alias '{}' for tool '{}': alias conflicts with existing tool name",
                        alias,
                        name
                    ));
                }
                // Also check if it conflicts with an existing alias
                if let Some(existing_target) = aliases_map.get(&alias_lower) {
                    // Only warn if the alias is being registered for a DIFFERENT tool
                    if existing_target != &name_lower {
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
            let mut tools = self
                .tools
                .write()
                .map_err(|e| anyhow::anyhow!("tool registry write lock poisoned: {e}"))?;
            use std::collections::hash_map::Entry;
            match tools.entry(name_lower.clone()) {
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
                    let mut sorted = self
                        .sorted_names
                        .write()
                        .map_err(|e| anyhow::anyhow!("sorted names write lock poisoned: {e}"))?;
                    let pos = sorted.binary_search(&name_lower).unwrap_or_else(|e| e);
                    sorted.insert(pos, name_lower.clone());
                }
            }
        }

        // Add to frequently used set if it's a common tool
        if self.is_common_tool(&name_lower)
            && let Ok(mut freq) = self.frequently_used.write()
        {
            freq.insert(name_lower.clone());
        }

        // Register case-insensitive aliases and track metrics
        if !aliases.is_empty() {
            self.register_aliases(&name_lower, &aliases);
        }

        // Clean up old cache entries if needed
        self.cleanup_cache_if_needed();

        Ok(())
    }

    /// Register case-insensitive aliases for a tool and track metrics
    fn register_aliases(&self, canonical_name_lower: &str, aliases: &[String]) {
        let Ok(mut aliases_map) = self.aliases.write() else {
            return;
        };
        let Ok(mut metrics) = self.alias_metrics.lock() else {
            return;
        };
        for alias in aliases {
            let alias_lower = alias.to_ascii_lowercase();
            let target = canonical_name_lower.to_owned();

            // Store lowercase -> canonical mapping
            aliases_map.insert(alias_lower.clone(), target.clone());

            // Initialize metrics for this alias
            metrics.usage.insert(alias_lower, (target, 0));
        }
    }

    pub fn registration_for(&self, name: &str) -> Option<ToolRegistration> {
        // Check if name exists directly or resolve via case-insensitive alias
        // IMPORTANT: Prefer alias resolution over direct registration when the direct
        // registration is not LLM-visible. This allows LLMs to call internal tool names
        // (like "read_file") and have them routed to their visible parent (like "unified_file").
        let name_lower = name.to_ascii_lowercase();

        let resolved_name = {
            let tools = self.tools.read().ok()?;
            let aliases = self.aliases.read().ok()?;

            // First check if there's an alias mapping for this name
            // This takes priority when the direct tool is not LLM-visible
            let alias_target = aliases.get(&name_lower).cloned();

            if let Some(entry) = tools.get(&name_lower) {
                // Direct tool exists - check if it's LLM-visible
                if entry.registration.expose_in_llm() {
                    // LLM-visible tool: use direct registration
                    name_lower.clone()
                } else if let Some(ref aliased) = alias_target {
                    // Not LLM-visible but has an alias: prefer the alias target
                    // This routes "read_file" → "unified_file" when called by LLM
                    if let Ok(mut metrics) = self.alias_metrics.lock()
                        && let Some((canonical, count)) = metrics.usage.get_mut(&name_lower)
                    {
                        *count += 1;
                        let count_val = *count;
                        let canonical_val = canonical.clone();
                        drop(metrics);
                        info!(
                            alias = %name,
                            canonical = %canonical_val,
                            count = count_val,
                            "Internal tool routed via alias to LLM-visible parent"
                        );
                    }
                    aliased.clone()
                } else {
                    // Not LLM-visible and no alias: use direct registration
                    // (for internal tool-to-tool calls)
                    name_lower.clone()
                }
            } else if let Some(aliased) = alias_target {
                // No direct registration but alias exists
                if let Ok(mut metrics) = self.alias_metrics.lock()
                    && let Some((canonical, count)) = metrics.usage.get_mut(&name_lower)
                {
                    *count += 1;
                    let count_val = *count;
                    let canonical_val = canonical.clone();
                    drop(metrics);
                    info!(
                        alias = %name,
                        canonical = %canonical_val,
                        count = count_val,
                        "Tool alias resolved and usage tracked"
                    );
                }
                aliased
            } else {
                return None;
            }
        };

        // Now get with the resolved name
        let tools = self.tools.read().ok()?;
        if let Some(entry) = tools.get(&resolved_name) {
            if let Ok(mut last) = entry.last_used.write() {
                *last = Instant::now();
            }
            entry
                .use_count
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

            // Track frequently used for aliased tools
            if resolved_name != name_lower
                && let Ok(mut freq) = self.frequently_used.write()
            {
                freq.insert(resolved_name);
            }
            return Some(entry.registration.clone());
        }

        None
    }

    /// Get a tool registration without updating usage metrics
    pub fn get_registration(&self, name: &str) -> Option<ToolRegistration> {
        let name_lower = name.to_ascii_lowercase();
        let tools = self.tools.read().ok()?;
        let aliases = self.aliases.read().ok()?;

        let resolved_name = if tools.contains_key(&name_lower) {
            &name_lower
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
        self.tools
            .read()
            .ok()
            .is_some_and(|t| t.contains_key(&name_lower))
            || self
                .aliases
                .read()
                .ok()
                .is_some_and(|a| a.contains_key(&name_lower))
    }

    pub fn available_tools(&self) -> Vec<String> {
        self.sorted_names
            .read()
            .ok()
            .map(|s| s.clone())
            .unwrap_or_default()
    }

    pub fn registered_aliases(&self) -> Vec<String> {
        self.aliases
            .read()
            .ok()
            .map(|a| a.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Get all registered aliases with their canonical targets
    #[allow(dead_code)]
    pub fn all_aliases(&self) -> Vec<(String, String)> {
        self.aliases
            .read()
            .ok()
            .map(|a| a.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default()
    }

    /// Snapshot registration metadata for policy/catalog synchronization
    pub fn registration_metadata(&self) -> Vec<(String, ToolMetadata)> {
        self.tools
            .read()
            .ok()
            .map(|t| {
                t.iter()
                    .map(|(name, entry)| (name.clone(), entry.registration.metadata().clone()))
                    .collect()
            })
            .unwrap_or_default()
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
        let Ok(last_cleanup) = self.last_cache_cleanup.read() else {
            return;
        };
        if last_cleanup.elapsed() < CACHE_CLEANUP_INTERVAL {
            return;
        }
        drop(last_cleanup);

        let Ok(mut tools) = self.tools.write() else {
            return;
        };
        if tools.len() < MAX_TOOLS {
            return;
        }

        let now = Instant::now();
        let old_len = tools.len();
        let frequently_used_snapshot = self
            .frequently_used
            .read()
            .ok()
            .map(|f| f.clone())
            .unwrap_or_default();

        // Remove tools that haven't been used in a while and aren't frequently used
        tools.retain(|name, entry| {
            // Keep frequently used tools
            if frequently_used_snapshot.contains(name) {
                return true;
            }

            // Keep tools used recently
            let last_used = entry.last_used.read().ok().map(|t| *t).unwrap_or(now);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::CapabilityLevel;
    use crate::tools::registry::registration::ToolRegistration;
    use serde_json::Value;
    use std::path::PathBuf;

    fn make_test_inventory() -> ToolInventory {
        ToolInventory::new(PathBuf::from("/tmp/vtcode-test"))
    }

    fn make_visible_registration(name: &'static str) -> ToolRegistration {
        ToolRegistration::new(name, CapabilityLevel::Basic, false, |_, _| {
            Box::pin(async { Ok(Value::Null) })
        })
    }

    fn make_hidden_registration(name: &'static str) -> ToolRegistration {
        ToolRegistration::new(name, CapabilityLevel::Basic, false, |_, _| {
            Box::pin(async { Ok(Value::Null) })
        })
        .with_llm_visibility(false)
    }

    #[test]
    fn test_alias_preferred_over_hidden_direct_tool() {
        let inventory = make_test_inventory();

        // Register a visible "parent" tool with an alias
        let parent = make_visible_registration("unified_file").with_aliases(["read_file"]);
        inventory.register_tool(parent).unwrap();

        // Register a hidden "internal" tool with the same name as the alias
        let internal = make_hidden_registration("read_file");
        inventory.register_tool(internal).unwrap();

        // When we look up "read_file", it should resolve to "unified_file"
        // because the direct "read_file" registration is not LLM-visible
        let registration = inventory.registration_for("read_file").unwrap();
        assert_eq!(
            registration.name(),
            "unified_file",
            "Hidden tool should be routed through its visible alias parent"
        );
    }

    #[test]
    fn test_visible_direct_tool_takes_precedence() {
        let inventory = make_test_inventory();

        // Register a visible "parent" tool with an alias
        let parent = make_visible_registration("unified_file").with_aliases(["read_file"]);
        inventory.register_tool(parent).unwrap();

        // Register a VISIBLE tool with the same name as the alias
        let visible_direct = make_visible_registration("read_file");
        inventory.register_tool(visible_direct).unwrap();

        // When we look up "read_file", it should resolve to "read_file"
        // because the direct registration is LLM-visible
        let registration = inventory.registration_for("read_file").unwrap();
        assert_eq!(
            registration.name(),
            "read_file",
            "Visible direct tool should take precedence"
        );
    }

    #[test]
    fn test_hidden_tool_without_alias_still_works() {
        let inventory = make_test_inventory();

        // Register only a hidden tool with no alias
        let internal = make_hidden_registration("internal_only");
        inventory.register_tool(internal).unwrap();

        // Should still find the tool for internal tool-to-tool calls
        let registration = inventory.registration_for("internal_only").unwrap();
        assert_eq!(
            registration.name(),
            "internal_only",
            "Hidden tool without alias should still be accessible"
        );
    }

    #[test]
    fn test_alias_metrics_tracked_for_hidden_tool_routing() {
        let inventory = make_test_inventory();

        // Register a visible tool with an alias
        let parent = make_visible_registration("unified_file").with_aliases(["read_file"]);
        inventory.register_tool(parent).unwrap();

        // Register a hidden internal tool
        let internal = make_hidden_registration("read_file");
        inventory.register_tool(internal).unwrap();

        // Get initial state - registration adds the entry with count 0
        let initial_metrics = inventory.alias_metrics();
        let initial_entry = initial_metrics.usage.get("read_file");
        assert!(
            initial_entry.is_some(),
            "Alias entry should be created during registration"
        );
        let initial_count = initial_entry.unwrap().1;

        // Look up via the hidden tool name twice
        inventory.registration_for("read_file");
        inventory.registration_for("read_file");

        // Verify metrics were incremented
        let metrics = inventory.alias_metrics();
        let usage_entry = metrics.usage.get("read_file");
        assert!(usage_entry.is_some(), "Alias usage should still be tracked");
        let (canonical, count) = usage_entry.unwrap();
        assert_eq!(canonical, "unified_file");
        assert_eq!(
            *count,
            initial_count + 2,
            "Usage count should have increased by 2"
        );
    }

    #[test]
    fn test_case_insensitive_alias_lookup() {
        let inventory = make_test_inventory();

        let tool = make_visible_registration("unified_file").with_aliases(["Read_File"]);
        inventory.register_tool(tool).unwrap();

        // Should resolve regardless of case
        assert!(inventory.registration_for("read_file").is_some());
        assert!(inventory.registration_for("READ_FILE").is_some());
        assert!(inventory.registration_for("Read_File").is_some());
    }

    #[test]
    fn test_duplicate_tool_registration_fails() {
        let inventory = make_test_inventory();

        let tool1 = make_visible_registration("my_tool");
        let tool2 = make_visible_registration("my_tool");

        inventory.register_tool(tool1).unwrap();
        let result = inventory.register_tool(tool2);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("already registered")
        );
    }

    #[test]
    fn test_alias_conflict_with_existing_tool_fails() {
        let inventory = make_test_inventory();

        // Register a tool first
        let tool1 = make_visible_registration("existing_tool");
        inventory.register_tool(tool1).unwrap();

        // Try to register a new tool with an alias matching the existing tool name
        let tool2 = make_visible_registration("new_tool").with_aliases(["existing_tool"]);
        let result = inventory.register_tool(tool2);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("conflicts"));
    }

    #[test]
    fn test_nonexistent_tool_returns_none() {
        let inventory = make_test_inventory();

        assert!(inventory.registration_for("nonexistent").is_none());
        assert!(!inventory.has_tool("nonexistent"));
    }

    #[test]
    fn test_cleanup_uses_frequently_used_snapshot() {
        let inventory = make_test_inventory();
        let stale = Instant::now() - Duration::from_secs(3601);

        for idx in 0..1001 {
            let name = format!("tool_{idx}");
            let leaked_name: &'static str = Box::leak(name.into_boxed_str());
            let registration =
                ToolRegistration::new(leaked_name, CapabilityLevel::Basic, false, |_, _| {
                    Box::pin(async { Ok(Value::Null) })
                });
            inventory.register_tool(registration).unwrap();
        }

        // Force all tools to look stale.
        {
            let tools = inventory.tools.read().unwrap();
            for entry in tools.values() {
                *entry.last_used.write().unwrap() = stale;
            }
        }

        // Keep one stale tool by marking it frequently used.
        {
            let mut freq = inventory.frequently_used.write().unwrap();
            freq.insert("tool_0".to_string());
        }
        {
            let mut last_cleanup = inventory.last_cache_cleanup.write().unwrap();
            *last_cleanup = Instant::now() - Duration::from_secs(301);
        }

        inventory.cleanup_cache_if_needed();

        let tools = inventory.tools.read().unwrap();
        assert!(tools.contains_key("tool_0"));
        assert!(tools.len() < 1001);
    }
}
