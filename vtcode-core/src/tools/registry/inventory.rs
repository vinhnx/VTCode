use rustc_hash::{FxHashMap, FxHashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::info;

use super::registration::ToolRegistration;
use crate::exec::skill_manager::SkillManager;
use crate::tools::edited_file_monitor::EditedFileMonitor;
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

#[derive(Debug)]
struct ToolInventoryState {
    aliases: FxHashMap<String, String>,
    frequently_used: FxHashSet<String>,
    last_cache_cleanup: Instant,
    sorted_names: Vec<String>,
}

#[derive(Clone)]
pub(super) struct ToolInventory {
    workspace_root: PathBuf,
    tools: Arc<std::sync::RwLock<FxHashMap<String, Arc<ToolCacheEntry>>>>,
    state: Arc<std::sync::RwLock<ToolInventoryState>>,
    /// Track alias usage for analytics and debugging
    alias_metrics: Arc<std::sync::Mutex<AliasMetrics>>,

    // Common tools that are used frequently
    file_ops_tool: FileOpsTool,
    command_tool: Arc<std::sync::RwLock<CommandTool>>,
    grep_search: Arc<GrepSearchManager>,
    skill_manager: SkillManager,
}

impl ToolInventory {
    pub fn new(workspace_root: PathBuf, edited_file_monitor: Arc<EditedFileMonitor>) -> Self {
        // Clone once for command_tool (needs ownership), share reference for others
        let command_tool = CommandTool::new(workspace_root.clone());
        let grep_search = Arc::new(GrepSearchManager::new(workspace_root.clone()));
        let file_ops_tool = FileOpsTool::new_with_monitor(
            workspace_root.clone(),
            Arc::clone(&grep_search),
            edited_file_monitor,
        );
        let skill_manager = SkillManager::new(&workspace_root);

        Self {
            workspace_root,
            tools: Arc::new(std::sync::RwLock::new(FxHashMap::default())),
            state: Arc::new(std::sync::RwLock::new(ToolInventoryState {
                aliases: FxHashMap::default(),
                frequently_used: FxHashSet::default(),
                last_cache_cleanup: Instant::now(),
                sorted_names: Vec::new(),
            })),
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
            let state = self
                .state
                .read()
                .map_err(|e| anyhow::anyhow!("inventory state read lock poisoned: {e}"))?;

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
                if let Some(existing_target) = state.aliases.get(&alias_lower) {
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
                    let mut state = self
                        .state
                        .write()
                        .map_err(|e| anyhow::anyhow!("inventory state write lock poisoned: {e}"))?;
                    let pos = state
                        .sorted_names
                        .binary_search(&name_lower)
                        .unwrap_or_else(|e| e);
                    state.sorted_names.insert(pos, name_lower.clone());
                }
            }
        }

        // Add to frequently used set if it's a common tool
        if self.is_common_tool(&name_lower)
            && let Ok(mut state) = self.state.write()
        {
            state.frequently_used.insert(name_lower.clone());
        }

        // Register case-insensitive aliases and track metrics
        if !aliases.is_empty() {
            self.register_aliases(&name_lower, &aliases);
        }

        // Clean up old cache entries if needed
        self.cleanup_cache_if_needed();

        Ok(())
    }

    pub fn remove_tool(&self, name: &str) -> anyhow::Result<Option<ToolRegistration>> {
        let name_lower = name.to_ascii_lowercase();
        let removed = {
            let mut tools = self
                .tools
                .write()
                .map_err(|e| anyhow::anyhow!("tool registry write lock poisoned: {e}"))?;
            tools.remove(&name_lower)
        };

        let Some(removed) = removed else {
            return Ok(None);
        };

        {
            let mut state = self
                .state
                .write()
                .map_err(|e| anyhow::anyhow!("inventory state write lock poisoned: {e}"))?;
            state
                .sorted_names
                .retain(|registered| registered != &name_lower);
            state.aliases.retain(|_, target| target != &name_lower);
            state.frequently_used.remove(&name_lower);
        }

        if let Ok(mut metrics) = self.alias_metrics.lock() {
            metrics
                .usage
                .retain(|_, (canonical, _)| canonical != &name_lower);
        }

        Ok(Some(removed.registration.clone()))
    }

    /// Register case-insensitive aliases for a tool and track metrics
    fn register_aliases(&self, canonical_name_lower: &str, aliases: &[String]) {
        let Ok(mut state) = self.state.write() else {
            return;
        };
        let Ok(mut metrics) = self.alias_metrics.lock() else {
            return;
        };
        for alias in aliases {
            let alias_lower = alias.to_ascii_lowercase();
            let target = canonical_name_lower.to_owned();

            // Store lowercase -> canonical mapping
            state.aliases.insert(alias_lower.clone(), target.clone());

            // Initialize metrics for this alias
            metrics.usage.insert(alias_lower, (target, 0));
        }
    }

    pub fn registration_for(&self, name: &str) -> Option<ToolRegistration> {
        // Check if name exists directly or resolve via case-insensitive alias.
        // Public tool routing happens earlier in the registry assembly; the inventory
        // stays a simple direct/alias lookup for canonical internal execution.
        let name_lower = name.to_ascii_lowercase();

        let resolved_name = {
            let tools = self.tools.read().ok()?;
            let state = self.state.read().ok()?;

            if let Some(entry) = tools.get(&name_lower) {
                let _ = entry;
                name_lower.clone()
            } else if let Some(aliased) = state.aliases.get(&name_lower).cloned() {
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
                && let Ok(mut state) = self.state.write()
            {
                state.frequently_used.insert(resolved_name);
            }
            return Some(entry.registration.clone());
        }

        None
    }

    /// Get a tool registration without updating usage metrics
    pub fn get_registration(&self, name: &str) -> Option<ToolRegistration> {
        let name_lower = name.to_ascii_lowercase();
        let tools = self.tools.read().ok()?;
        let state = self.state.read().ok()?;

        let resolved_name = if tools.contains_key(&name_lower) {
            &name_lower
        } else if let Some(aliased) = state.aliases.get(&name_lower) {
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
                .state
                .read()
                .ok()
                .is_some_and(|s| s.aliases.contains_key(&name_lower))
    }

    /// Get all registered aliases with their canonical targets
    #[allow(dead_code)]
    pub fn all_aliases(&self) -> Vec<(String, String)> {
        self.state
            .read()
            .ok()
            .map(|s| {
                s.aliases
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn registrations_snapshot(&self) -> Vec<ToolRegistration> {
        self.tools
            .read()
            .ok()
            .map(|tools| {
                tools
                    .values()
                    .map(|entry| entry.registration.clone())
                    .collect::<Vec<_>>()
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
        let Ok(state) = self.state.read() else {
            return;
        };
        if state.last_cache_cleanup.elapsed() < CACHE_CLEANUP_INTERVAL {
            return;
        }
        drop(state);

        let Ok(mut tools) = self.tools.write() else {
            return;
        };
        if tools.len() < MAX_TOOLS {
            return;
        }

        let now = Instant::now();
        let old_len = tools.len();
        let frequently_used_snapshot = self
            .state
            .read()
            .ok()
            .map(|s| s.frequently_used.clone())
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

        if let Ok(mut state) = self.state.write() {
            state.last_cache_cleanup = now;
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
    use crate::tools::edited_file_monitor::EditedFileMonitor;
    use crate::tools::registry::registration::ToolRegistration;
    use serde_json::Value;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn make_test_inventory() -> ToolInventory {
        ToolInventory::new(
            PathBuf::from("/tmp/vtcode-test"),
            Arc::new(EditedFileMonitor::new()),
        )
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
    fn test_hidden_direct_tool_takes_precedence_over_alias_parent() {
        let inventory = make_test_inventory();

        // Register a visible "parent" tool with an alias
        let parent = make_visible_registration("unified_file").with_aliases(["read_file"]);
        inventory.register_tool(parent).unwrap();

        // Register a hidden "internal" tool with the same name as the alias
        let internal = make_hidden_registration("read_file");
        inventory.register_tool(internal).unwrap();

        // Direct registration should still win for internal execution even when hidden.
        let registration = inventory.registration_for("read_file").unwrap();
        assert_eq!(
            registration.name(),
            "read_file",
            "Direct hidden registration should remain addressable for internal callers"
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
    fn test_direct_hidden_lookup_does_not_increment_alias_metrics() {
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

        // Direct lookups should not consume the alias path anymore.
        inventory.registration_for("read_file");
        inventory.registration_for("read_file");

        // Verify metrics were incremented
        let metrics = inventory.alias_metrics();
        let usage_entry = metrics.usage.get("read_file");
        assert!(usage_entry.is_some(), "Alias usage should still be tracked");
        let (canonical, count) = usage_entry.unwrap();
        assert_eq!(canonical, "unified_file");
        assert_eq!(
            *count, initial_count,
            "Direct hidden registration lookups should not increment alias usage"
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
            let mut state = inventory.state.write().unwrap();
            state.frequently_used.insert("tool_0".to_string());
            state.last_cache_cleanup = Instant::now() - Duration::from_secs(301);
        }

        inventory.cleanup_cache_if_needed();

        let tools = inventory.tools.read().unwrap();
        assert!(tools.contains_key("tool_0"));
        assert!(tools.len() < 1001);
    }
}
