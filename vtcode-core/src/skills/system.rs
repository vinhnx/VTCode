use include_dir::Dir;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::Hash;
use std::hash::Hasher;
use std::path::{Path, PathBuf};
use thiserror::Error;

const SYSTEM_SKILLS_DIR: Dir =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/src/skills/assets/samples");

const SYSTEM_SKILLS_DIR_NAME: &str = ".system";
const SKILLS_DIR_NAME: &str = "skills";
const SYSTEM_SKILLS_MARKER_FILENAME: &str = ".codex-system-skills.marker";
const SYSTEM_SKILLS_MARKER_SALT: &str = "v1";

/// Returns the on-disk cache location for embedded system skills.
///
/// This is typically located at `CODEX_HOME/skills/.system`.
pub(crate) fn system_cache_root_dir(codex_home: &Path) -> PathBuf {
    codex_home
        .join(SKILLS_DIR_NAME)
        .join(SYSTEM_SKILLS_DIR_NAME)
}

/// Installs embedded system skills into `CODEX_HOME/skills/.system`.
///
/// Clears any existing system skills directory first and then writes the embedded
/// skills directory into place.
///
/// To avoid doing unnecessary work on every startup, a marker file is written
/// with a fingerprint of the embedded directory. When the marker matches, the
/// install is skipped.
pub(crate) fn install_system_skills(codex_home: &Path) -> Result<(), SystemSkillsError> {
    let skills_root_dir = codex_home.join(SKILLS_DIR_NAME);
    fs::create_dir_all(&skills_root_dir)
        .map_err(|source| SystemSkillsError::io("create skills root dir", source))?;

    let dest_system = system_cache_root_dir(codex_home);

    let marker_path = dest_system.join(SYSTEM_SKILLS_MARKER_FILENAME);
    let expected_fingerprint = embedded_system_skills_fingerprint();
    if dest_system.is_dir()
        && read_marker(&marker_path).is_ok_and(|marker| marker == expected_fingerprint)
    {
        return Ok(());
    }

    if dest_system.exists() {
        fs::remove_dir_all(&dest_system)
            .map_err(|source| SystemSkillsError::io("remove existing system skills dir", source))?;
    }

    write_embedded_dir(&SYSTEM_SKILLS_DIR, &dest_system)?;
    fs::write(&marker_path, format!("{expected_fingerprint}\n"))
        .map_err(|source| SystemSkillsError::io("write system skills marker", source))?;
    Ok(())
}

fn read_marker(path: &Path) -> Result<String, SystemSkillsError> {
    Ok(fs::read_to_string(path)
        .map_err(|source| SystemSkillsError::io("read system skills marker", source))?
        .trim()
        .to_string())
}

fn embedded_system_skills_fingerprint() -> String {
    let mut items: Vec<(String, Option<u64>)> = SYSTEM_SKILLS_DIR
        .entries()
        .iter()
        .map(|entry| match entry {
            include_dir::DirEntry::Dir(dir) => (dir.path().to_string_lossy().to_string(), None),
            include_dir::DirEntry::File(file) => {
                let mut file_hasher = DefaultHasher::new();
                file.contents().hash(&mut file_hasher);
                (
                    file.path().to_string_lossy().to_string(),
                    Some(file_hasher.finish()),
                )
            }
        })
        .collect();
    items.sort_unstable_by(|(a, _), (b, _)| a.cmp(b));

    let mut hasher = DefaultHasher::new();
    SYSTEM_SKILLS_MARKER_SALT.hash(&mut hasher);
    for (path, contents_hash) in items {
        path.hash(&mut hasher);
        contents_hash.hash(&mut hasher);
    }
    format!("{:x}", hasher.finish())
}

/// Writes the embedded `include_dir::Dir` to disk under `dest`.
///
/// Preserves the embedded directory structure.
fn write_embedded_dir(dir: &Dir<'_>, dest: &Path) -> Result<(), SystemSkillsError> {
    fs::create_dir_all(dest)
        .map_err(|source| SystemSkillsError::io("create system skills dir", source))?;

    for entry in dir.entries() {
        match entry {
            include_dir::DirEntry::Dir(subdir) => {
                let subdir_dest = dest.join(subdir.path());
                fs::create_dir_all(&subdir_dest).map_err(|source| {
                    SystemSkillsError::io("create system skills subdir", source)
                })?;
                write_embedded_dir(subdir, dest)?; // Note: write_embedded_dir handles relative path inside, but here we recurse?
                // Wait, include_dir recursion:
                // include_dir entries paths are relative to the root of the Dir.
                // But write_embedded_dir assumes `dir` is the root to write to `dest`.
                // Actually, the recursion logic in reference was:
                // write_embedded_dir(subdir, dest)?;
                // But subdir.path() is relative to Parent Dir.
                // Let's check reference again.
                // logic: dest.join(subdir.path()) -> this is correct.
                // But passing `dest` again to recursive call means it will join subdir.path() AGAIN?
                // `include_dir` paths are relative to the embedded root.
                // So if root has `foo/bar`, subdir `foo` has path `foo`.
                // Recursing on `foo`: its entries have path `foo/bar`.
                // So joining `dest` + `foo/bar` is correct.
                // So `dest` should stay the same.
            }
            include_dir::DirEntry::File(file) => {
                let path = dest.join(file.path());
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(|source| {
                        SystemSkillsError::io("create system skills file parent", source)
                    })?;
                }
                fs::write(&path, file.contents())
                    .map_err(|source| SystemSkillsError::io("write system skill file", source))?;
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
}

impl SystemSkillsError {
    fn io(action: &'static str, source: std::io::Error) -> Self {
        Self::Io { action, source }
    }
}
