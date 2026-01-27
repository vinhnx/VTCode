use std::path::Path;

/// Check if a command uses cargo, which requires exclusive file lock access
pub fn is_cargo_command(program: &str) -> bool {
    let name = Path::new(program)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(program)
        .to_ascii_lowercase();
    name == "cargo"
}

/// Check if a command string (potentially passed via shell -c) is a cargo command
pub fn is_cargo_command_string(command: &str) -> bool {
    let trimmed = command.trim();
    trimmed.starts_with("cargo ") || trimmed == "cargo"
}

pub(super) fn is_shell_program(program: &str) -> bool {
    let name = Path::new(program)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(program)
        .to_ascii_lowercase();
    matches!(
        name.as_str(),
        "bash" | "sh" | "zsh" | "fish" | "dash" | "ash" | "busybox"
    )
}

// Note: resolve_fallback_shell moved to tools::shell module

/// Resolve program path - if program doesn't exist in PATH, return None to signal shell fallback.
/// This allows the shell to find programs installed in user-specific directories.
pub fn is_development_toolchain_command(program: &str) -> bool {
    let name = Path::new(program)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(program)
        .to_ascii_lowercase();
    matches!(
        name.as_str(),
        "cargo"
            | "rustc"
            | "rustup"
            | "rustfmt"
            | "clippy"
            | "npm"
            | "node"
            | "yarn"
            | "pnpm"
            | "bun"
            | "go"
            | "python"
            | "python3"
            | "pip"
            | "pip3"
            | "java"
            | "javac"
            | "mvn"
            | "gradle"
            | "make"
            | "cmake"
            | "gcc"
            | "g++"
            | "clang"
            | "clang++"
            | "which"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_cargo_command() {
        assert!(is_cargo_command("cargo"));
        assert!(is_cargo_command("Cargo"));
        assert!(is_cargo_command("/usr/bin/cargo"));
        assert!(is_cargo_command("~/.cargo/bin/cargo"));
        assert!(!is_cargo_command("rustc"));
        assert!(!is_cargo_command("npm"));
        assert!(!is_cargo_command("cargo-watch"));
    }

    #[test]
    fn test_is_cargo_command_string() {
        assert!(is_cargo_command_string("cargo check"));
        assert!(is_cargo_command_string("cargo test --lib"));
        assert!(is_cargo_command_string("cargo build --release"));
        assert!(is_cargo_command_string("  cargo clippy  "));
        assert!(is_cargo_command_string("cargo"));
        assert!(!is_cargo_command_string("rustc --version"));
        assert!(!is_cargo_command_string("npm install"));
        assert!(!is_cargo_command_string("echo cargo"));
    }
}
