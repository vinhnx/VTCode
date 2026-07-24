pub trait EnvironmentProbe: Send + Sync {
    fn check(&self, workspace: &std::path::Path) -> bool;
}

pub struct CommandProbe {
    command: String,
    args: Vec<String>,
}
impl CommandProbe {
    pub fn new(command: String, args: Vec<String>) -> Self {
        Self { command, args }
    }
}
impl EnvironmentProbe for CommandProbe {
    fn check(&self, workspace: &std::path::Path) -> bool {
        use std::process::Command;
        Command::new(&self.command)
            .args(&self.args)
            .current_dir(workspace)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

pub struct FileExistsProbe {
    path: std::path::PathBuf,
}
impl FileExistsProbe {
    pub fn new(path: std::path::PathBuf) -> Self {
        Self { path }
    }
}
impl EnvironmentProbe for FileExistsProbe {
    fn check(&self, _workspace: &std::path::Path) -> bool {
        self.path.exists()
    }
}

pub struct GitCleanProbe;
impl EnvironmentProbe for GitCleanProbe {
    fn check(&self, workspace: &std::path::Path) -> bool {
        use std::process::Command;
        Command::new("git")
            .args(["diff", "--quiet"])
            .current_dir(workspace)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}
