use serde::{Deserialize, Serialize};

/// Command execution configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CommandsConfig {
    /// Commands that can be executed without prompting
    #[serde(default)]
    pub allow_list: Vec<String>,

    /// Commands that are always denied
    #[serde(default)]
    pub deny_list: Vec<String>,

    /// Glob patterns allowed for shell commands (applies to run_terminal_cmd/Bash)
    #[serde(default)]
    pub allow_glob: Vec<String>,

    /// Glob patterns denied for shell commands
    #[serde(default)]
    pub deny_glob: Vec<String>,

    /// Regex allow patterns for shell commands
    #[serde(default)]
    pub allow_regex: Vec<String>,

    /// Regex deny patterns for shell commands
    #[serde(default)]
    pub deny_regex: Vec<String>,
}

impl Default for CommandsConfig {
    fn default() -> Self {
        Self {
            allow_list: vec![
                "ls".to_string(),
                "pwd".to_string(),
                "cat".to_string(),
                "grep".to_string(),
                "find".to_string(),
                "head".to_string(),
                "tail".to_string(),
                "wc".to_string(),
                "git status".to_string(),
                "git diff".to_string(),
                "git log".to_string(),
                "cargo check".to_string(),
                "cargo tree".to_string(),
                "cargo metadata".to_string(),
                "which".to_string(),
                "echo".to_string(),
            ],
            deny_list: vec![
                "rm -rf /".to_string(),
                "rm -rf ~".to_string(),
                "rm -rf /*".to_string(),
                "shutdown".to_string(),
                "reboot".to_string(),
                "halt".to_string(),
                "poweroff".to_string(),
                "sudo rm".to_string(),
                "sudo chmod".to_string(),
                "sudo chown".to_string(),
                "format".to_string(),
                "fdisk".to_string(),
                "mkfs".to_string(),
                "dd if=".to_string(),
                "wget".to_string(),
                "curl".to_string(),
                ":(){ :|:& };:".to_string(), // Fork bomb
            ],
            allow_glob: vec![
                "git *".to_string(),
                "cargo *".to_string(),
                "rustc *".to_string(),
                "python -m *".to_string(),
                "node *".to_string(),
                "npm *".to_string(),
                "yarn *".to_string(),
                "pnpm *".to_string(),
            ],
            deny_glob: vec![
                "rm *".to_string(),
                "sudo *".to_string(),
                "chmod *".to_string(),
                "chown *".to_string(),
                "kill *".to_string(),
                "pkill *".to_string(),
                "systemctl *".to_string(),
                "service *".to_string(),
                "mount *".to_string(),
                "umount *".to_string(),
                "docker run *".to_string(),
                "kubectl *".to_string(),
            ],
            allow_regex: vec![
                r"^(ls|pwd|cat|grep|find|head|tail|wc)\b".to_string(),
                r"^git (status|diff|log|show|branch)\b".to_string(),
                r"^cargo (check|build|test|doc|clippy|fmt)\b".to_string(),
            ],
            deny_regex: vec![
                r"rm\s+(-rf|--force)".to_string(),
                r"sudo\s+.*".to_string(),
                r"chmod\s+.*".to_string(),
                r"chown\s+.*".to_string(),
                r"docker\s+run\s+.*--privileged".to_string(),
                r"kubectl\s+(delete|drain|uncordon)".to_string(),
            ],
        }
    }
}
