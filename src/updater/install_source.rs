use std::env;
use std::path::Path;

const CURL_INSTALL_COMMAND: &str =
    "curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InstallSource {
    Standalone,
    Homebrew,
    Cargo,
    Npm,
}

impl InstallSource {
    pub(crate) fn is_managed(self) -> bool {
        !matches!(self, Self::Standalone)
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Standalone => "standalone",
            Self::Homebrew => "homebrew",
            Self::Cargo => "cargo",
            Self::Npm => "npm",
        }
    }

    pub(crate) fn update_command(self) -> &'static str {
        match self {
            Self::Standalone => CURL_INSTALL_COMMAND,
            Self::Homebrew => "brew upgrade vinhnx/tap/vtcode",
            Self::Cargo => "cargo install vtcode --force",
            Self::Npm => "npm install -g vtcode@latest",
        }
    }
}

pub(super) fn detect_install_source() -> InstallSource {
    let exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(_) => return InstallSource::Standalone,
    };

    let canonical = std::fs::canonicalize(&exe).unwrap_or(exe);
    detect_install_source_from_path(&canonical)
}

pub(super) fn detect_install_source_from_path(path: &Path) -> InstallSource {
    let path_text = path.to_string_lossy().to_ascii_lowercase();

    if path_text.contains("/cellar/")
        || path_text.contains("/homebrew/")
        || path_text.contains("/linuxbrew/")
        || path_text.contains("/opt/homebrew/")
    {
        return InstallSource::Homebrew;
    }

    if path_text.contains("/.cargo/bin/") {
        return InstallSource::Cargo;
    }

    if path_text.contains("/node_modules/") || path_text.contains("/npm/") {
        return InstallSource::Npm;
    }

    InstallSource::Standalone
}

pub(super) fn get_target_triple() -> Option<&'static str> {
    match (env::consts::OS, env::consts::ARCH) {
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        ("linux", "x86_64") => Some("x86_64-unknown-linux-musl"),
        ("linux", "aarch64") => Some("aarch64-unknown-linux-gnu"),
        ("windows", "x86_64") => Some("x86_64-pc-windows-msvc"),
        _ => None,
    }
}
