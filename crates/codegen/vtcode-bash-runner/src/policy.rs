use crate::executor::{CommandCategory, CommandInvocation};
use anyhow::{Result, bail};
use hashbrown::HashSet;
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
        vtcode_commons::paths::ensure_path_within_workspace(path, root).map_err(|error| {
            error.context(format!(
                "path `{}` escapes the workspace root `{}`",
                path.display(),
                root.display()
            ))
        })?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::ShellKind;
    use std::path::PathBuf;

    struct StaticWorkspace {
        root: PathBuf,
    }

    impl WorkspacePaths for StaticWorkspace {
        fn workspace_root(&self) -> &Path {
            &self.root
        }

        fn config_dir(&self) -> PathBuf {
            self.root.join("config")
        }
    }

    fn policy() -> WorkspaceGuardPolicy {
        WorkspaceGuardPolicy::new(Arc::new(StaticWorkspace {
            root: PathBuf::from("/tmp/workspace"),
        }))
    }

    fn invocation(working_dir: &str, touched: &[&str]) -> CommandInvocation {
        CommandInvocation::new(
            ShellKind::Unix,
            "true".to_string(),
            CommandCategory::ListDirectory,
            PathBuf::from(working_dir),
        )
        .with_paths(touched.iter().map(PathBuf::from).collect())
    }

    #[test]
    fn accepts_paths_inside_workspace() {
        let invocation = invocation("/tmp/workspace/src", &["/tmp/workspace/file.txt"]);
        assert!(policy().check(&invocation).is_ok());
    }

    #[test]
    fn rejects_working_dir_traversal_escape() {
        let invocation = invocation("/tmp/workspace/../etc", &[]);
        assert!(policy().check(&invocation).is_err());
    }

    #[test]
    fn rejects_touched_path_traversal_escape() {
        let invocation = invocation("/tmp/workspace", &["/tmp/workspace/../../etc/passwd"]);
        assert!(policy().check(&invocation).is_err());
    }

    #[test]
    fn accepts_traversal_that_stays_inside_workspace() {
        let invocation = invocation("/tmp/workspace/src/../src", &[]);
        assert!(policy().check(&invocation).is_ok());
    }

    #[test]
    fn rejects_path_outside_workspace() {
        let invocation = invocation("/tmp/other", &[]);
        assert!(policy().check(&invocation).is_err());
    }
}
