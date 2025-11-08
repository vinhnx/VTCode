use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::registration::ToolRegistration;
use crate::tools::ast_grep::AstGrepEngine;
use crate::tools::command::CommandTool;
use crate::tools::file_ops::FileOpsTool;
use crate::tools::grep_file::GrepSearchManager;
use crate::tools::plan::PlanManager;
use tracing::warn;

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
    ast_grep_engine: Option<Arc<AstGrepEngine>>,
    plan_manager: PlanManager,
}

impl ToolInventory {
    pub fn new(workspace_root: PathBuf) -> Self {
        let grep_search = Arc::new(GrepSearchManager::new(workspace_root.clone()));
        let file_ops_tool = FileOpsTool::new(workspace_root.clone(), grep_search.clone());
        let command_tool = CommandTool::new(workspace_root.clone());
        let plan_manager = PlanManager::new();

        let ast_grep_engine = match AstGrepEngine::new() {
            Ok(engine) => Some(Arc::new(engine)),
            Err(err) => {
                warn!("Failed to initialize AST-grep engine: {err}");
                None
            }
        };

        Self {
            workspace_root: workspace_root.clone(),
            tools: HashMap::new(),
            aliases: HashMap::new(),
            frequently_used: HashSet::new(),
            last_cache_cleanup: Instant::now(),
            file_ops_tool,
            command_tool,
            grep_search,
            ast_grep_engine,
            plan_manager,
        }
    }

    pub fn workspace_root(&self) -> &PathBuf {
        &self.workspace_root
    }

    pub fn file_ops_tool(&self) -> &FileOpsTool {
        &self.file_ops_tool
    }

    pub fn command_tool(&self) -> &CommandTool {
        &self.command_tool
    }

    pub fn command_tool_mut(&mut self) -> &mut CommandTool {
        &mut self.command_tool
    }

    pub fn grep_file_manager(&self) -> Arc<GrepSearchManager> {
        self.grep_search.clone()
    }

    pub fn ast_grep_engine(&self) -> Option<&Arc<AstGrepEngine>> {
        self.ast_grep_engine.as_ref()
    }

    pub fn set_ast_grep_engine(&mut self, engine: Arc<AstGrepEngine>) {
        self.ast_grep_engine = Some(engine);
    }

    pub fn plan_manager(&self) -> PlanManager {
        self.plan_manager.clone()
    }

    pub fn register_tool(&mut self, registration: ToolRegistration) -> anyhow::Result<()> {
        let name = registration.name().to_string();

        // Check if tool already exists
        if self.tools.contains_key(&name) {
            return Err(anyhow::anyhow!("Tool '{}' is already registered", name));
        }

        // Add to cache
        self.tools.insert(
            name.clone(),
            ToolCacheEntry {
                registration,
                last_used: Instant::now(),
                use_count: 0,
            },
        );

        // Add to frequently used set if it's a common tool
        if self.is_common_tool(&name) {
            self.frequently_used.insert(name.clone());
        }

        // Clean up old cache entries if needed
        self.cleanup_cache_if_needed();

        Ok(())
    }

    pub fn registration_for(&mut self, name: &str) -> Option<&mut ToolRegistration> {
        // First check direct match
        if self.tools.contains_key(name) {
            let entry = self.tools.get_mut(name).unwrap();
            entry.last_used = Instant::now();
            entry.use_count += 1;
            return Some(&mut entry.registration);
        }

        // Then try alias resolution
        if let Some(actual_name) = self.aliases.get(name) {
            if self.tools.contains_key(actual_name) {
                let entry = self.tools.get_mut(actual_name).unwrap();
                entry.last_used = Instant::now();
                entry.use_count += 1;
                self.frequently_used.insert(actual_name.clone());
                return Some(&mut entry.registration);
            }
        }

        None
    }

    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.contains_key(name) || self.aliases.contains_key(name)
    }

    pub fn available_tools(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    /// Add an alias for a tool
    pub fn add_alias(&mut self, alias: &str, tool_name: &str) {
        self.aliases
            .insert(alias.to_string(), tool_name.to_string());
    }

    /// Check if a tool is commonly used
    fn is_common_tool(&self, name: &str) -> bool {
        matches!(name, "file_ops" | "command" | "grep" | "ast_grep" | "plan")
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
