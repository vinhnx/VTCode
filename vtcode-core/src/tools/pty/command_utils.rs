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

/// Commands that use lock files and need serialization.
/// These commands access lock files (Cargo.lock, package-lock.json, etc.)
/// and can cause "blocking waiting for file lock" errors if run concurrently.
const LOCKFILE_COMMANDS: &[&str] = &[
    "cargo",    // Cargo.lock
    "npm",      // package-lock.json
    "pnpm",     // pnpm-lock.yaml
    "yarn",     // yarn.lock
    "bun",      // bun.lockb
    "go",       // go.sum
    "gradle",   // gradle.lockfile
    "mvn",      // uses local repo locks
    "pip",      // pip can have concurrent install issues
    "pip3",     // pip3 same as pip
    "poetry",   // poetry.lock
    "composer", // composer.lock (PHP)
    "bundler",  // Gemfile.lock (Ruby)
    "bundle",   // Gemfile.lock (Ruby)
];

fn normalize_program_name(program: &str) -> String {
    Path::new(program)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(program)
        .to_ascii_lowercase()
}

/// Check if a command requires serialization due to lock file access.
/// These commands should not be run concurrently within the same workspace
/// to prevent lock file contention errors.
pub fn is_lockfile_command(program: &str) -> bool {
    let name = normalize_program_name(program);
    LOCKFILE_COMMANDS.contains(&name.as_str())
}

/// Check if a command should be serialized because it is long-running.
pub fn is_long_running_command(program: &str) -> bool {
    let name = normalize_program_name(program);
    is_development_toolchain_command(&name) || is_lockfile_command(&name)
}

/// Check if a command string (potentially passed via shell -c) is a lockfile command
pub fn is_lockfile_command_string(command: &str) -> bool {
    let trimmed = command.trim();
    LOCKFILE_COMMANDS
        .iter()
        .any(|&cmd| trimmed.starts_with(&format!("{cmd} ")) || trimmed == cmd)
}

/// Check if a command string (potentially passed via shell -c) is long-running
pub fn is_long_running_command_string(command: &str) -> bool {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return false;
    }
    if is_lockfile_command_string(trimmed) {
        return true;
    }
    let first = trimmed.split_whitespace().next().unwrap_or_default();
    is_development_toolchain_command(first)
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

pub(super) fn is_sandbox_wrapper_program(program: &str, args: &[String]) -> bool {
    let name = Path::new(program)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(program)
        .to_ascii_lowercase();
    if name == "sandbox-exec" {
        return true;
    }

    args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "--sandbox-policy" | "--sandbox-policy-cwd" | "--seccomp-profile" | "--resource-limits"
        )
    })
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

    #[test]
    fn test_is_long_running_command() {
        assert!(is_long_running_command("cargo"));
        assert!(is_long_running_command("cmake"));
        assert!(is_long_running_command("composer"));
        assert!(is_long_running_command("bundle"));
        assert!(is_long_running_command("poetry"));
        assert!(!is_long_running_command("rg"));
    }

    #[test]
    fn test_is_long_running_command_string() {
        assert!(is_long_running_command_string("cargo test"));
        assert!(is_long_running_command_string(" cmake --build ."));
        assert!(is_long_running_command_string("composer install"));
        assert!(is_long_running_command_string("bundle exec rake"));
        assert!(is_long_running_command_string("poetry install"));
        assert!(!is_long_running_command_string("rg \"fn main\" ."));
    }

    #[test]
    fn test_is_sandbox_wrapper_program() {
        assert!(is_sandbox_wrapper_program("sandbox-exec", &[]));
        assert!(is_sandbox_wrapper_program(
            "vtcode-linux-sandbox",
            &["--sandbox-policy".to_string()]
        ));
        assert!(!is_sandbox_wrapper_program(
            "bash",
            &["-lc".to_string(), "ls".to_string()]
        ));
    }
}
