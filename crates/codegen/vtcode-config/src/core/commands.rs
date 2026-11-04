use serde::{Deserialize, Serialize};

use crate::constants::commands as command_constants;

/// Command execution configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CommandsConfig {
    /// Commands that can be executed without prompting
    #[serde(default = "default_allow_list")]
    pub allow_list: Vec<String>,

    /// Command prefixes that skip future shell approval prompts when matched
    #[serde(default)]
    pub approval_prefixes: Vec<String>,

    /// Additional directories that should be searched/prepended to PATH for command execution
    #[serde(default = "default_extra_path_entries")]
    pub extra_path_entries: Vec<String>,

    /// Commands that are always denied
    #[serde(default = "default_deny_list")]
    pub deny_list: Vec<String>,

    /// Glob patterns allowed for shell commands (applies to Bash)
    #[serde(default = "default_allow_glob")]
    pub allow_glob: Vec<String>,

    /// Glob patterns denied for shell commands
    #[serde(default = "default_deny_glob")]
    pub deny_glob: Vec<String>,

    /// Regex allow patterns for shell commands
    #[serde(default = "default_allow_regex")]
    pub allow_regex: Vec<String>,

    /// Regex deny patterns for shell commands
    #[serde(default = "default_deny_regex")]
    pub deny_regex: Vec<String>,
}

const DEFAULT_ALLOW_LIST: &[&str] = &[
    // File and directory operations
    "ls",
    "pwd",
    "cat",
    "grep",
    "find",
    "head",
    "tail",
    "wc",
    "tree",
    "stat",
    "file",
    "sort",
    "uniq",
    "cut",
    "awk",
    "sed",
    // Archive operations
    "tar",
    "zip",
    "unzip",
    "gzip",
    "gunzip",
    // Build tools
    "make",
    "cmake",
    "ninja",
    "which",
    "echo",
    "printf",
    "read",
    "date",
    "sleep",
    // Version control
    "git status",
    "git diff",
    "git log",
    "git show",
    "git branch",
    "git remote",
    // Rust ecosystem
    "cargo check",
    "cargo build",
    "cargo build --release",
    "cargo build --profile release",
    "cargo test",
    "cargo run",
    "cargo clippy",
    "cargo fmt",
    "cargo tree",
    "cargo metadata",
    "cargo doc",
    "rustc",
    // Python ecosystem
    "python3",
    "python3 -m pip install",
    "python3 -m pytest",
    "python3 -m build",
    "python",
    "pip3",
    "pip",
    "virtualenv",
    // Node.js ecosystem
    "node",
    "npm",
    "npm run build",
    "npm run test",
    "npm install",
    "yarn",
    "yarn build",
    "yarn test",
    "pnpm",
    "pnpm build",
    "pnpm test",
    "bun",
    "bun install",
    "bun run",
    "bun test",
    "npx",
    // Go ecosystem
    "go",
    "go build",
    "go test",
    // C/C++
    "gcc",
    "g++",
    "clang",
    "clang++",
    // Java ecosystem
    "javac",
    "java",
    "mvn",
    "gradle",
    // Container operations
    "docker",
    "docker-compose",
];

impl Default for CommandsConfig {
    fn default() -> Self {
        Self {
            allow_list: default_allow_list(),
            approval_prefixes: Vec::new(),
            extra_path_entries: default_extra_path_entries(),
            deny_list: default_deny_list(),
            allow_glob: default_allow_glob(),
            deny_glob: default_deny_glob(),
            allow_regex: default_allow_regex(),
            deny_regex: default_deny_regex(),
        }
    }
}

/// Built-in `allow_list` applied when the key is absent from a `[commands]` section.
fn default_allow_list() -> Vec<String> {
    DEFAULT_ALLOW_LIST.iter().map(|s| (*s).into()).collect()
}

/// Built-in `deny_list` applied when the key is absent from a `[commands]` section.
fn default_deny_list() -> Vec<String> {
    vec![
        // Dangerous file deletion
        "rm".into(),
        "rm -rf /".into(),
        "rm -rf ~".into(),
        "rm -rf /*".into(),
        "rm -rf /home".into(),
        "rm -rf /usr".into(),
        "rm -rf /etc".into(),
        "rm -rf /var".into(),
        "rm -rf /opt".into(),
        "rmdir /".into(),
        "rmdir /home".into(),
        "rmdir /usr".into(),
        // System control
        "shutdown".into(),
        "reboot".into(),
        "halt".into(),
        "poweroff".into(),
        "init 0".into(),
        "init 6".into(),
        "systemctl poweroff".into(),
        "systemctl reboot".into(),
        "systemctl halt".into(),
        // Privilege escalation
        "sudo rm".into(),
        "sudo chmod 777".into(),
        "sudo chown".into(),
        "sudo passwd".into(),
        "sudo su".into(),
        "sudo -i".into(),
        "sudo bash".into(),
        "su root".into(),
        "su -".into(),
        // Disk operations
        "format".into(),
        "fdisk".into(),
        "mkfs".into(),
        "mkfs.ext4".into(),
        "mkfs.xfs".into(),
        "mkfs.vfat".into(),
        "dd if=/dev/zero".into(),
        "dd if=/dev/random".into(),
        "dd if=/dev/urandom".into(),
        // Security risks
        "wget --no-check-certificate".into(),
        ":(){ :|:& };:".into(), // Fork bomb
        "nohup bash -i".into(),
        "exec bash -i".into(),
        "eval".into(),
        // Shell configuration
        "source /etc/bashrc".into(),
        "source ~/.bashrc".into(),
        // Permission changes
        "chmod 777".into(),
        "chmod -R 777".into(),
        "chown -R".into(),
        "chgrp -R".into(),
        // SSH key destruction
        "rm ~/.ssh/*".into(),
        "rm -r ~/.ssh".into(),
        // Sensitive file access
        "cat /etc/passwd".into(),
        "cat /etc/shadow".into(),
        "cat ~/.ssh/id_*".into(),
        "tail -f /var/log".into(),
        "head -n 1 /var/log".into(),
    ]
}

/// Built-in `allow_glob` applied when the key is absent from a `[commands]` section.
fn default_allow_glob() -> Vec<String> {
    vec![
        // Version control
        "git *".into(),
        // Rust ecosystem
        "cargo *".into(),
        "rustc *".into(),
        // Python ecosystem
        "python *".into(),
        "python3 *".into(),
        "pip *".into(),
        "pip3 *".into(),
        "virtualenv *".into(),
        // Node.js ecosystem
        "node *".into(),
        "npm *".into(),
        "npm run *".into(),
        "yarn *".into(),
        "yarn run *".into(),
        "pnpm *".into(),
        "pnpm run *".into(),
        "bun *".into(),
        "bun run *".into(),
        "npx *".into(),
        // Go
        "go *".into(),
        // C/C++
        "gcc *".into(),
        "g++ *".into(),
        "clang *".into(),
        "clang++ *".into(),
        // Java
        "javac *".into(),
        "java *".into(),
        "mvn *".into(),
        "gradle *".into(),
        // Build tools
        "make *".into(),
        "cmake *".into(),
        "ninja *".into(),
        // Containers
        "docker *".into(),
        "docker-compose *".into(),
        // Archive tools
        "tar *".into(),
        "zip *".into(),
        "unzip *".into(),
        "gzip *".into(),
        "gunzip *".into(),
    ]
}

/// Built-in `deny_glob` applied when the key is absent from a `[commands]` section.
fn default_deny_glob() -> Vec<String> {
    vec![
        // File deletion
        "rm *".into(),
        // Privilege escalation
        "sudo *".into(),
        // Permission changes
        "chmod *".into(),
        "chown *".into(),
        // Process termination
        "kill *".into(),
        "pkill *".into(),
        // System services
        "systemctl *".into(),
        "service *".into(),
        // Mount operations
        "mount *".into(),
        "umount *".into(),
        // Dangerous container operations
        "docker run *".into(),
        // Kubernetes (admin access)
        "kubectl *".into(),
    ]
}

/// Built-in `allow_regex` applied when the key is absent from a `[commands]` section.
fn default_allow_regex() -> Vec<String> {
    vec![
                // File and text utilities
                r"^(ls|pwd|cat|grep|find|head|tail|wc|echo|printf|read|date|sleep|tree|stat|file|sort|uniq|cut|awk|sed|tar|zip|unzip|gzip|gunzip)\b".into(),
                // Version control
                r"^git (status|diff|log|show|branch|remote)\b".into(),
                // Rust
                r"^cargo (check|build|test|run|doc|clippy|fmt|tree|metadata)\b".into(),
                r"^rustc\b".into(),
                // Python
                r"^(python|python3) (-m | )?\w*".into(),
                r"^(pip|pip3)\b".into(),
                r"^virtualenv\b".into(),
                // Node.js
                r"^(node|npm|yarn|pnpm|bun|npx)\b".into(),
                // Go
                r"^go\b".into(),
                // C/C++
                r"^(gcc|g\+\+|clang|clang\++)\b".into(),
                // Java
                r"^(javac|java)\b".into(),
                r"^(mvn|gradle)\b".into(),
                // Build tools
                r"^(make|cmake)\b".into(),
                // Containers
                r"^(docker|docker-compose)\b".into(),
            ]
}

/// Built-in `deny_regex` applied when the key is absent from a `[commands]` section.
fn default_deny_regex() -> Vec<String> {
    vec![
        // Force removal
        r"rm\s+(-rf|--force)".into(),
        // Sudo commands
        r"sudo\s+.*".into(),
        // Permission changes
        r"chmod\s+.*".into(),
        r"chown\s+.*".into(),
        // Privileged containers
        r"docker\s+run\s+.*--privileged".into(),
        // Dangerous kubectl operations
        r"kubectl\s+(delete|drain|uncordon)".into(),
    ]
}

fn default_extra_path_entries() -> Vec<String> {
    command_constants::DEFAULT_EXTRA_PATH_ENTRIES
        .iter()
        .map(|value| (*value).into())
        .collect()
}
