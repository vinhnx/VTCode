use include_dir::Dir;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;
use vtcode_commons::fs::{ensure_dir_exists_sync, read_file_with_context_sync, write_file_with_context_sync};
use vtcode_commons::utils::calculate_sha256;

const SYSTEM_SKILLS_DIR: Dir = include_dir::include_dir!("$CARGO_MANIFEST_DIR/src/skills/assets/samples");

const SYSTEM_SKILLS_DIR_NAME: &str = ".system";
const SKILLS_DIR_NAME: &str = "skills";
const SYSTEM_SKILLS_MARKER_FILENAME: &str = ".codex-system-skills.marker";
/// Bump this version to force reinstallation of system skills.
/// v2: Updated skills with Codex-compatible patterns (XML wrapping, workflows, scripts).
const SYSTEM_SKILLS_MARKER_SALT: &str = "v2";

/// Returns the on-disk cache location for embedded system skills.
///
/// This is typically located at `CODEX_HOME/skills/.system`.
pub fn system_cache_root_dir(codex_home: &Path) -> PathBuf {
    codex_home.join(SKILLS_DIR_NAME).join(SYSTEM_SKILLS_DIR_NAME)
}

/// Installs embedded system skills into `CODEX_HOME/skills/.system`.
///
/// Clears any existing system skills directory first and then writes the embedded
/// skills directory into place.
///
/// To avoid doing unnecessary work on every startup, a marker file is written
/// with a fingerprint of the embedded directory. When the marker matches, the
/// install is skipped.
pub fn install_system_skills(codex_home: &Path) -> Result<(), SystemSkillsError> {
    let skills_root_dir = codex_home.join(SKILLS_DIR_NAME);
    ensure_dir_exists_sync(&skills_root_dir)
        .map_err(|source| SystemSkillsError::io("create skills root dir", anyhow_to_io(source)))?;

    let dest_system = system_cache_root_dir(codex_home);

    let marker_path = dest_system.join(SYSTEM_SKILLS_MARKER_FILENAME);
    let expected_fingerprint = embedded_system_skills_fingerprint();
    if dest_system.is_dir() && read_marker(&marker_path).is_ok_and(|marker| marker == expected_fingerprint) {
        return Ok(());
    }

    if dest_system.exists() {
        fs::remove_dir_all(&dest_system)
            .map_err(|source| SystemSkillsError::io("remove existing system skills dir", source))?;
    }

    write_embedded_dir(&SYSTEM_SKILLS_DIR, &dest_system)?;
    write_file_with_context_sync(&marker_path, &format!("{expected_fingerprint}\n"), "system skills marker")
        .map_err(|source| SystemSkillsError::io("write system skills marker", anyhow_to_io(source)))?;
    Ok(())
}

pub fn uninstall_system_skills(codex_home: &Path) {
    let _ = fs::remove_dir_all(system_cache_root_dir(codex_home));
}

fn anyhow_to_io(err: anyhow::Error) -> std::io::Error {
    std::io::Error::other(err.to_string())
}

fn read_marker(path: &Path) -> Result<String, SystemSkillsError> {
    Ok(read_file_with_context_sync(path, "system skills marker")
        .map_err(|source| SystemSkillsError::io("read system skills marker", anyhow_to_io(source)))?
        .trim()
        .to_string())
}

fn stable_manifest_fingerprint<'a, I>(salt: &str, items: I) -> String
where
    I: IntoIterator<Item = (&'a str, Option<&'a str>)>,
{
    let mut manifest = String::new();
    manifest.push_str("salt\t");
    manifest.push_str(salt);
    manifest.push('\n');

    for (path, contents_hash) in items {
        manifest.push_str(path);
        manifest.push('\t');
        manifest.push_str(contents_hash.unwrap_or("<dir>"));
        manifest.push('\n');
    }

    calculate_sha256(manifest.as_bytes())
}

fn embedded_system_skills_fingerprint() -> String {
    let mut items: Vec<(String, Option<String>)> = SYSTEM_SKILLS_DIR
        .entries()
        .iter()
        .map(|entry| match entry {
            include_dir::DirEntry::Dir(dir) => (dir.path().to_string_lossy().to_string(), None),
            include_dir::DirEntry::File(file) => {
                (file.path().to_string_lossy().to_string(), Some(calculate_sha256(file.contents())))
            }
        })
        .collect();
    items.sort_unstable_by(|(a, _), (b, _)| a.cmp(b));

    stable_manifest_fingerprint(
        SYSTEM_SKILLS_MARKER_SALT,
        items
            .iter()
            .map(|(path, contents_hash)| (path.as_str(), contents_hash.as_deref())),
    )
}

/// Writes the embedded `include_dir::Dir` to disk under `dest`.
///
/// Preserves the embedded directory structure.
fn write_embedded_dir(dir: &Dir<'_>, dest: &Path) -> Result<(), SystemSkillsError> {
    ensure_dir_exists_sync(dest)
        .map_err(|source| SystemSkillsError::io("create system skills dir", anyhow_to_io(source)))?;

    for entry in dir.entries() {
        match entry {
            include_dir::DirEntry::Dir(subdir) => {
                let subdir_dest = dest.join(subdir.path());
                ensure_dir_exists_sync(&subdir_dest)
                    .map_err(|source| SystemSkillsError::io("create system skills subdir", anyhow_to_io(source)))?;
                write_embedded_dir(subdir, dest)?;
            }
            include_dir::DirEntry::File(file) => {
                let path = dest.join(file.path());
                if let Some(parent) = path.parent() {
                    ensure_dir_exists_sync(parent).map_err(|source| {
                        SystemSkillsError::io("create system skills file parent", anyhow_to_io(source))
                    })?;
                }
                let contents = std::str::from_utf8(file.contents())
                    .map_err(|source| SystemSkillsError::Utf8 { path: file.path().to_path_buf(), source })?;
                write_file_with_context_sync(&path, contents, "system skill file")
                    .map_err(|source| SystemSkillsError::io("write system skill file", anyhow_to_io(source)))?;
            }
        }
    }

    Ok(())
}

#[derive(Debug, Error)]
pub enum SystemSkillsError {
    #[error("io error while {action}: {source}")]
    Io {
        action: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("embedded system skill {path} is not valid UTF-8: {source}")]
    Utf8 {
        path: PathBuf,
        #[source]
        source: std::str::Utf8Error,
    },
}

impl SystemSkillsError {
    fn io(action: &'static str, source: std::io::Error) -> Self {
        Self::Io { action, source }
    }
}

#[cfg(test)]
mod tests {
    use super::{SYSTEM_SKILLS_MARKER_SALT, embedded_system_skills_fingerprint, stable_manifest_fingerprint};

    #[test]
    fn stable_manifest_fingerprint_matches_known_digest() {
        let fingerprint = stable_manifest_fingerprint(
            SYSTEM_SKILLS_MARKER_SALT,
            [
                ("alpha", None),
                ("alpha/config.json", Some("77c7ce9a5d86bb386d443bb96390dcf0ecf6afb7b74f84a88d64e6d4e8dcb5e0")),
                ("beta.md", Some("3f89f6a04a1a1f8cb46fc4b356f7b81b8d18102fdb8b795f5b2f89e7cfefb3af")),
            ],
        );

        assert_eq!(fingerprint, "218ca69badb47208804e293074956e23ed68d6946dde5d36e6fe8d56df170170");
    }

    #[test]
    fn embedded_system_skills_fingerprint_uses_full_sha256_digest() {
        assert_eq!(embedded_system_skills_fingerprint().len(), 64);
    }
}
