use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::registration::ToolRegistration;
use crate::tools::command::CommandTool;
use crate::tools::file_ops::FileOpsTool;
use crate::tools::grep_file::GrepSearchManager;
use crate::tools::plan::PlanManager;

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
    aliases: HashMap<String, String>,
    frequently_used: HashSet<String>,
    last_cache_cleanup: Instant,

    // Common tools that are used frequently
    file_ops_tool: FileOpsTool,
    command_tool: CommandTool,
    grep_search: Arc<GrepSearchManager>,
    plan_manager: PlanManager,
}

impl ToolInventory {
    pub fn new(workspace_root: PathBuf) -> Self {
        // Clone once for command_tool (needs ownership), share reference for others
        let command_tool = CommandTool::new(workspace_root.clone());
        let grep_search = Arc::new(GrepSearchManager::new(workspace_root.clone()));
        let file_ops_tool = FileOpsTool::new(workspace_root.clone(), Arc::clone(&grep_search));
        let plan_manager = PlanManager::new();

        Self {
            workspace_root,
            tools: HashMap::new(),
            aliases: HashMap::new(),
            frequently_used: HashSet::new(),
            last_cache_cleanup: Instant::now(),
            file_ops_tool,
            command_tool,
            grep_search,
            plan_manager,
        }
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

    pub fn plan_manager(&self) -> PlanManager {
        self.plan_manager.clone()
    }

    pub fn register_tool(&mut self, registration: ToolRegistration) -> anyhow::Result<()> {
        let name = registration.name().to_string();

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
            }
        }

        // Add to frequently used set if it's a common tool
        if self.is_common_tool(&name) {
            self.frequently_used.insert(name);
        }

        // Clean up old cache entries if needed
        self.cleanup_cache_if_needed();

        Ok(())
    }

    pub fn registration_for(&mut self, name: &str) -> Option<&mut ToolRegistration> {
        // Check if name exists directly or resolve via alias
        let resolved_name = if self.tools.contains_key(name) {
            name.to_string()
        } else if let Some(aliased) = self.aliases.get(name) {
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

    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.contains_key(name) || self.aliases.contains_key(name)
    }

    pub fn available_tools(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
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
