//! File classification for the file picker.
//!
//! The picker distinguishes a small, stable set of kinds so rows can render a
//! type glyph and so special entries (symlinks, executables, images) get
//! accurate treatment without the render path ever touching the filesystem.
//! Metadata inspection happens once, off the render path, in
//! [`DirEntryInfo::from_path`].

use std::path::{Path, PathBuf};

/// Coarse classification of a picker row.
///
/// This is intentionally small: it drives the row glyph and styling, not
/// content handling. Directories are handled separately by the palette's
/// navigation (`is_dir`), so `FileKind` only needs to enrich files and
/// symlinks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// A directory (or symlink to one).
    Directory,
    /// A source/text/config file recognized by extension.
    Code,
    /// A recognized image file.
    Image,
    /// An executable: Unix exec bit, or a known script/binary extension.
    Executable,
    /// Anything else.
    Other,
}

/// Extensions treated as source, config, or documentation text.
///
/// Kept as a bounded lookup so classification is allocation-free and stable.
const CODE_EXTENSIONS: &[&str] = &[
    "rs",
    "py",
    "pyi",
    "js",
    "mjs",
    "cjs",
    "ts",
    "tsx",
    "jsx",
    "go",
    "java",
    "kt",
    "kts",
    "scala",
    "swift",
    "c",
    "h",
    "cc",
    "cpp",
    "cxx",
    "hpp",
    "hh",
    "m",
    "mm",
    "cs",
    "rb",
    "php",
    "lua",
    "pl",
    "r",
    "jl",
    "dart",
    "ex",
    "exs",
    "erl",
    "hrl",
    "hs",
    "ml",
    "mli",
    "clj",
    "cljs",
    "cljc",
    "zig",
    "nim",
    "v",
    "d",
    "proto",
    "thrift",
    "sql",
    "sh",
    "bash",
    "zsh",
    "fish",
    "ps1",
    "bat",
    "cmd",
    "toml",
    "yaml",
    "yml",
    "json",
    "jsonc",
    "json5",
    "xml",
    "ini",
    "cfg",
    "conf",
    "env",
    "properties",
    "gradle",
    "cmake",
    "mk",
    "makefile",
    "dockerfile",
    "containerfile",
    "md",
    "mdx",
    "rst",
    "txt",
    "tex",
    "css",
    "scss",
    "sass",
    "less",
    "html",
    "htm",
    "vue",
    "svelte",
    "astro",
    "graphql",
    "gql",
    "tf",
    "tfvars",
    "nix",
    "lock",
    "gitignore",
    "gitattributes",
    "editorconfig",
];

/// Extensions treated as executable even when the platform has no exec bit
/// (Windows) or the file lives on a noexec mount.
const EXECUTABLE_EXTENSIONS: &[&str] = &["exe", "com", "msi", "bat", "cmd", "ps1", "app", "run", "bin"];

impl FileKind {
    /// Classify a path from already-known directory status and executable bit.
    ///
    /// Pure (no filesystem access) so it can be unit-tested without a fixture.
    pub(crate) fn classify(path: &Path, is_dir: bool, is_executable: bool) -> Self {
        if is_dir {
            return Self::Directory;
        }
        if is_executable {
            return Self::Executable;
        }
        if vtcode_commons::fs::is_image_path(path) {
            return Self::Image;
        }

        let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
            // Extensionless names such as `Makefile`/`Dockerfile` are still code.
            return match path.file_name().and_then(|name| name.to_str()) {
                Some(name) if is_extensionless_code_name(name) => Self::Code,
                _ => Self::Other,
            };
        };

        let extension = extension.to_ascii_lowercase();
        if CODE_EXTENSIONS.contains(&extension.as_str()) {
            Self::Code
        } else {
            Self::Other
        }
    }

    /// Whether a path is executable by extension, used on platforms without a
    /// POSIX exec bit.
    pub(crate) fn has_executable_extension(path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase)
            .is_some_and(|ext| EXECUTABLE_EXTENSIONS.contains(&ext.as_str()))
    }

    /// Single-column glyph shown before the file name.
    pub(crate) fn glyph(self) -> &'static str {
        match self {
            Self::Directory => "▸",
            Self::Code => " ",
            Self::Image => "▧",
            Self::Executable => "⚙",
            Self::Other => " ",
        }
    }
}

fn is_extensionless_code_name(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "makefile" | "dockerfile" | "containerfile" | "rakefile" | "gemfile" | "procfile" | "justfile" | "brewfile"
    )
}

/// Metadata about one directory child, produced by the picker's [`super::DirLister`].
///
/// The binary supplies these so the render path never performs filesystem IO:
/// [`DirEntryInfo::from_path`] runs once per child when a directory is listed,
/// and the resulting metadata is cached with the entry.
#[derive(Debug, Clone)]
pub struct DirEntryInfo {
    /// Absolute path of the child.
    pub path: PathBuf,
    /// Whether the child is a directory (or a symlink resolving to one).
    pub is_dir: bool,
    /// Type glyph classification.
    pub kind: FileKind,
    /// Resolved symlink target, when `path` is a symbolic link.
    pub symlink_target: Option<PathBuf>,
    /// `true` when `path` is a symlink whose target does not exist.
    pub symlink_broken: bool,
}

impl DirEntryInfo {
    /// Inspect a path and capture everything the picker needs: kind, symlink
    /// target, and broken-link status. Called once per child when a directory is
    /// listed, so the render path can stay IO-free.
    pub fn from_path(path: PathBuf, is_dir: bool) -> Self {
        let symlink_meta = std::fs::symlink_metadata(&path).ok();
        let is_symlink = symlink_meta.as_ref().is_some_and(|meta| meta.file_type().is_symlink());

        let symlink_target = is_symlink.then(|| std::fs::read_link(&path).ok()).flatten();
        let symlink_broken = symlink_target.as_ref().is_some_and(|target| {
            let resolved = if target.is_absolute() {
                target.clone()
            } else {
                path.parent().map_or_else(|| target.clone(), |parent| parent.join(target))
            };
            !resolved.exists()
        });

        // Extension-based executables apply to every kind. The POSIX exec bit is
        // only meaningful for regular files: probing it follows symlinks and
        // would mislabel a link to a directory (directories are executable).
        let is_executable =
            FileKind::has_executable_extension(&path) || (!is_dir && !is_symlink && is_posix_executable(&path));

        Self {
            kind: FileKind::classify(&path, is_dir, is_executable),
            path,
            is_dir,
            symlink_target,
            symlink_broken,
        }
    }
}

#[cfg(unix)]
fn is_posix_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    std::fs::metadata(path)
        .map(|meta| meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_posix_executable(_path: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_directory_wins_over_extension() {
        assert_eq!(FileKind::classify(Path::new("/w/src"), true, false), FileKind::Directory);
        // A directory named `foo.rs` is still a directory.
        assert_eq!(FileKind::classify(Path::new("/w/foo.rs"), true, true), FileKind::Directory);
    }

    #[test]
    fn classify_code_image_executable_other() {
        assert_eq!(FileKind::classify(Path::new("/w/main.rs"), false, false), FileKind::Code);
        assert_eq!(FileKind::classify(Path::new("/w/icon.png"), false, false), FileKind::Image);
        assert_eq!(FileKind::classify(Path::new("/w/tool"), false, true), FileKind::Executable);
        assert_eq!(FileKind::classify(Path::new("/w/archive.tar"), false, false), FileKind::Other);
    }

    #[test]
    fn classify_is_case_insensitive_for_extensions() {
        assert_eq!(FileKind::classify(Path::new("/w/MAIN.RS"), false, false), FileKind::Code);
        assert_eq!(FileKind::classify(Path::new("/w/Icon.PNG"), false, false), FileKind::Image);
    }

    #[test]
    fn classify_extensionless_makefile_is_code() {
        assert_eq!(FileKind::classify(Path::new("/w/Makefile"), false, false), FileKind::Code);
        assert_eq!(FileKind::classify(Path::new("/w/Dockerfile"), false, false), FileKind::Code);
        assert_eq!(FileKind::classify(Path::new("/w/LICENSE"), false, false), FileKind::Other);
    }

    #[test]
    fn executable_extension_detection() {
        assert!(FileKind::has_executable_extension(Path::new("/w/setup.EXE")));
        assert!(!FileKind::has_executable_extension(Path::new("/w/run.sh")));
        assert!(!FileKind::has_executable_extension(Path::new("/w/main.rs")));
    }

    #[cfg(unix)]
    #[test]
    fn from_path_detects_broken_symlink() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().expect("tempdir");
        let link = dir.path().join("dangling");
        symlink(dir.path().join("missing"), &link).expect("symlink");

        let info = DirEntryInfo::from_path(link, false);
        assert!(info.symlink_broken, "broken symlink should be flagged");
        assert!(info.symlink_target.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn from_path_detects_executable_bit() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let script = dir.path().join("run");
        std::fs::write(&script, "#!/bin/sh\n").expect("write");
        let mut perms = std::fs::metadata(&script).expect("meta").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).expect("chmod");

        let info = DirEntryInfo::from_path(script, false);
        assert_eq!(info.kind, FileKind::Executable);
    }

    #[cfg(unix)]
    #[test]
    fn from_path_symlink_to_directory_is_not_executable() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("realdir");
        std::fs::create_dir(&target).expect("mkdir target");
        let link = dir.path().join("dirlink");
        symlink(&target, &link).expect("symlink");

        // `is_dir` is resolved by the caller's walker; the symlink itself must
        // not inherit the target directory's executable bit.
        let info = DirEntryInfo::from_path(link, true);
        assert_eq!(info.kind, FileKind::Directory);
    }
}
