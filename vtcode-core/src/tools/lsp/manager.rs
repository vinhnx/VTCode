use std::path::PathBuf;
use std::process::Command;

/// Detects if common LSP servers are available in the PATH
pub fn detect_server(lang_ext: &str) -> Option<String> {
    // Mapping extension to common server commands
    let candidates = match lang_ext {
        "rs" => vec!["rust-analyzer"],
        "py" => vec!["pyright-langserver", "pylsp"],
        "go" => vec!["gopls"],
        "js" | "ts" | "jsx" | "tsx" => vec!["typescript-language-server"],
        _ => vec![],
    };

    for cmd in candidates {
        if is_executable_in_path(cmd) {
            return Some(cmd.to_string());
        }
    }
    None
}

fn is_executable_in_path(cmd: &str) -> bool {
    // Simple check: try 'which' or run with --version
    // Using --version is safer
    Command::new(cmd).arg("--version").output().is_ok()
}
