use crate::executor::{CommandCategory, CommandInvocation};
use anyhow::{Result, bail};
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use vtcode_commons::WorkspacePaths;

pub trait CommandPolicy: Send + Sync {
    fn check(&self, invocation: &CommandInvocation) -> Result<()>;
}

pub struct AllowAllPolicy;

impl CommandPolicy for AllowAllPolicy {
    fn check(&self, _invocation: &CommandInvocation) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone)]
pub struct WorkspaceGuardPolicy {
    workspace: Arc<dyn WorkspacePaths>,
    allowed_commands: Option<HashSet<CommandCategory>>,
}

impl WorkspaceGuardPolicy {
    pub fn new(workspace: Arc<dyn WorkspacePaths>) -> Self {
        Self {
            workspace,
            allowed_commands: None,
        }
    }

    pub fn with_allowed_commands(
        mut self,
        commands: impl IntoIterator<Item = CommandCategory>,
    ) -> Self {
        self.allowed_commands = Some(commands.into_iter().collect());
        self
    }

    fn ensure_within_workspace(&self, path: &Path) -> Result<()> {
        let root = self.workspace.workspace_root();
        if !path.starts_with(root) {
            bail!(
                "path `{}` escapes the workspace root `{}`",
                path.display(),
                root.display()
            );
        }
        Ok(())
    }
}

impl CommandPolicy for WorkspaceGuardPolicy {
    fn check(&self, invocation: &CommandInvocation) -> Result<()> {
        if let Some(allowed) = &self.allowed_commands
            && !allowed.contains(&invocation.category)
        {
            bail!(
                "command category {:?} is not permitted",
                invocation.category
            );
        }

        self.ensure_within_workspace(&invocation.working_dir)?;

        for path in &invocation.touched_paths {
            self.ensure_within_workspace(path)?;
        }

        Ok(())
    }
}
