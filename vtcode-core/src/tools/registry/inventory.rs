use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::sandbox::SandboxProfile;
use crate::tools::ast_grep::AstGrepEngine;
use crate::tools::bash_tool::BashTool;
use crate::tools::command::CommandTool;
use crate::tools::curl_tool::CurlTool;
use crate::tools::file_ops::FileOpsTool;
use crate::tools::git_diff::GitDiffTool;
use crate::tools::grep_file::GrepSearchManager;
use crate::tools::plan::PlanManager;
use crate::tools::pty::PtyManager;
use crate::tools::search::SearchTool;
use crate::tools::simple_search::SimpleSearchTool;
use crate::tools::srgn::SrgnTool;

use super::registration::ToolRegistration;

#[derive(Clone)]
pub(super) struct ToolInventory {
    workspace_root: PathBuf,
    search_tool: SearchTool,
    simple_search_tool: SimpleSearchTool,
    bash_tool: BashTool,
    file_ops_tool: FileOpsTool,
    command_tool: CommandTool,
    curl_tool: CurlTool,
    grep_search: Arc<GrepSearchManager>,
    git_diff_tool: GitDiffTool,
    ast_grep_engine: Option<Arc<AstGrepEngine>>,
    srgn_tool: SrgnTool,
    plan_manager: PlanManager,
    tool_registrations: Vec<ToolRegistration>,
    tool_lookup: HashMap<&'static str, usize>,
}

impl ToolInventory {
    pub fn new(workspace_root: PathBuf) -> Self {
        let grep_search = Arc::new(GrepSearchManager::new(workspace_root.clone()));

        let search_tool = SearchTool::new(workspace_root.clone(), grep_search.clone());
        let simple_search_tool = SimpleSearchTool::new(workspace_root.clone());
        let bash_tool = BashTool::new(workspace_root.clone());
        let file_ops_tool = FileOpsTool::new(workspace_root.clone(), grep_search.clone());
        let command_tool = CommandTool::new(workspace_root.clone());
        let curl_tool = CurlTool::new();
        let git_diff_tool = GitDiffTool::new(workspace_root.clone());
        let srgn_tool = SrgnTool::new(workspace_root.clone());
        let plan_manager = PlanManager::new();

        let ast_grep_engine = match AstGrepEngine::new() {
            Ok(engine) => Some(Arc::new(engine)),
            Err(err) => {
                eprintln!("Warning: Failed to initialize AST-grep engine: {}", err);
                None
            }
        };

        Self {
            workspace_root,
            search_tool,
            simple_search_tool,
            bash_tool,
            file_ops_tool,
            command_tool,
            curl_tool,
            grep_search,
            git_diff_tool,
            ast_grep_engine,
            srgn_tool,
            plan_manager,
            tool_registrations: Vec::new(),
            tool_lookup: HashMap::new(),
        }
    }

    pub fn workspace_root(&self) -> &PathBuf {
        &self.workspace_root
    }

    pub fn search_tool(&self) -> &SearchTool {
        &self.search_tool
    }

    pub fn simple_search_tool(&self) -> &SimpleSearchTool {
        &self.simple_search_tool
    }

    pub fn bash_tool(&self) -> &BashTool {
        &self.bash_tool
    }

    pub fn set_bash_sandbox(&mut self, profile: Option<SandboxProfile>) {
        self.bash_tool.set_sandbox_profile(profile);
    }

    pub fn set_pty_manager(&mut self, manager: PtyManager) {
        self.bash_tool.set_pty_manager(manager);
    }

    pub fn file_ops_tool(&self) -> &FileOpsTool {
        &self.file_ops_tool
    }

    pub fn command_tool(&self) -> &CommandTool {
        &self.command_tool
    }

    pub fn curl_tool(&self) -> &CurlTool {
        &self.curl_tool
    }

    pub fn git_diff_tool(&self) -> &GitDiffTool {
        &self.git_diff_tool
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

    pub fn srgn_tool(&self) -> &SrgnTool {
        &self.srgn_tool
    }

    pub fn plan_manager(&self) -> PlanManager {
        self.plan_manager.clone()
    }

    pub fn register_tool(&mut self, registration: ToolRegistration) -> anyhow::Result<()> {
        if self.tool_lookup.contains_key(registration.name()) {
            return Err(anyhow::anyhow!(format!(
                "Tool '{}' is already registered",
                registration.name()
            )));
        }

        let index = self.tool_registrations.len();
        self.tool_lookup.insert(registration.name(), index);
        self.tool_registrations.push(registration);
        Ok(())
    }

    pub fn registration_for(&self, name: &str) -> Option<&ToolRegistration> {
        self.tool_lookup
            .get(name)
            .and_then(|index| self.tool_registrations.get(*index))
    }

    pub fn has_tool(&self, name: &str) -> bool {
        self.tool_lookup.contains_key(name)
    }

    pub fn available_tools(&self) -> Vec<String> {
        self.tool_registrations
            .iter()
            .map(|registration| registration.name().to_string())
            .collect()
    }
}
