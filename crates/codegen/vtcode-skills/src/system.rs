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

/// Names of the skills embedded in [`SYSTEM_SKILLS_DIR`].
///
/// Kept as an explicit list (pinned against the embedded directory by test)
/// so decline-path errors can name bundled skills without touching the disk.
#[must_use]
pub fn bundled_skill_names() -> &'static [&'static str] {
    &["cmd-review", "skill-creator", "skill-installer"]
}

/// Returns the on-disk cache location for embedded system skills.
///
/// This is typically located at `<home>/skills/.system`, where `home` is the
/// unified [`bundled_skills_home`] in production or a caller-supplied
/// override (hermetic tests, benchmarks).
pub fn system_cache_root_dir(codex_home: &Path) -> PathBuf {
    codex_home.join(SKILLS_DIR_NAME).join(SYSTEM_SKILLS_DIR_NAME)
}

/// Resolve the unified home directory for bundled system skills.
///
/// This is always VT Code's own data directory — never another tool's home.
/// Resolution failure means installs are impossible, so callers must fail
/// closed (skip the install and the system discovery root) instead of
/// falling back to a relative directory.
pub fn bundled_skills_home() -> Result<PathBuf, SystemSkillsError> {
    let paths = vtcode_commons::VtCodePaths::resolve()
        .map_err(|source| SystemSkillsError::io("resolve the bundled skills home", anyhow_to_io(source)))?;
    Ok(paths.data_dir().to_path_buf())
}

/// Whether a bundled-skills cache marker exists under `home`.
///
/// Presence (not fingerprint match) is the grandfather signal: any marker
/// proves a previous release installed here, including older embedded sets.
#[must_use]
pub fn has_bundled_skills_cache(home: &Path) -> bool {
    system_cache_root_dir(home).join(SYSTEM_SKILLS_MARKER_FILENAME).is_file()
}

/// Whether a pre-consent cache exists in the legacy `~/.codex` location.
///
/// Read-only grandfather probe. VT Code never writes to or deletes from
/// another tool's home directory; a marker here only permits refreshing
/// VT Code's own unified cache without re-asking.
#[must_use]
pub fn legacy_bundled_skills_cache_present() -> bool {
    let legacy_home = std::env::var_os("CODEX_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")));
    legacy_home.is_some_and(|home| has_bundled_skills_cache(&home))
}

/// Whether a pre-consent install grandfathers bundled-skills refreshes.
///
/// True when our own unified cache or the legacy `~/.codex` cache carries a
/// marker from a previous release. Fail-closed: resolution errors mean no.
#[must_use]
pub fn bundled_skills_grandfathered() -> bool {
    bundled_skills_home().is_ok_and(|home| has_bundled_skills_cache(&home)) || legacy_bundled_skills_cache_present()
}

/// Installs embedded system skills into `<home>/skills/.system`.
///
/// Clears any existing system skills directory first and then writes the embedded
/// skills directory into place.
///
/// To avoid doing unnecessary work on every startup, a marker file is written
/// with a fingerprint of the embedded directory. When the marker matches, the
/// install is skipped.
///
/// Callers must gate this on consent (`vtcode_config::consent`). Relative
/// homes are refused outright so a misconfigured override can never scatter
/// a cache into the current working directory.
pub fn install_system_skills(codex_home: &Path) -> Result<(), SystemSkillsError> {
    require_absolute_home(codex_home)?;
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
    if require_absolute_home(codex_home).is_err() {
        return;
    }
    let _ = fs::remove_dir_all(system_cache_root_dir(codex_home));
}

fn require_absolute_home(codex_home: &Path) -> Result<(), SystemSkillsError> {
    if codex_home.is_absolute() {
        Ok(())
    } else {
        Err(SystemSkillsError::NonAbsoluteHome { home: codex_home.to_path_buf() })
    }
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
    #[error("refusing to manage system skills under relative home {}", home.display())]
    NonAbsoluteHome { home: PathBuf },
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
    use super::{
        SYSTEM_SKILLS_DIR, SYSTEM_SKILLS_MARKER_SALT, bundled_skill_names, bundled_skills_grandfathered,
        embedded_system_skills_fingerprint, has_bundled_skills_cache, install_system_skills,
        legacy_bundled_skills_cache_present, stable_manifest_fingerprint, system_cache_root_dir,
        uninstall_system_skills,
    };
    use serial_test::serial;

    #[test]
    fn bundled_skill_names_match_embedded_samples() {
        let mut embedded: Vec<String> = SYSTEM_SKILLS_DIR
            .entries()
            .iter()
            .filter_map(|entry| match entry {
                include_dir::DirEntry::Dir(dir) => {
                    dir.path().file_name().and_then(|name| name.to_str()).map(str::to_string)
                }
                include_dir::DirEntry::File(_) => None,
            })
            .collect();
        embedded.sort_unstable();
        let mut named: Vec<String> = bundled_skill_names().iter().map(ToString::to_string).collect();
        named.sort_unstable();
        assert_eq!(named, embedded);
    }

    #[test]
    fn install_and_uninstall_refuse_relative_homes() {
        let relative = std::path::PathBuf::from("relative-skills-home");
        let error = install_system_skills(&relative).expect_err("relative install must fail");
        assert!(error.to_string().contains("relative home"), "error: {error}");
        assert!(!system_cache_root_dir(&relative).exists(), "refused install must not create anything");
        // Uninstall is best-effort by design; the guard makes it a silent no-op.
        uninstall_system_skills(&relative);
        uninstall_system_skills(std::path::Path::new(""));
    }

    #[test]
    fn cache_presence_tracks_install_lifecycle() {
        let home = tempfile::TempDir::new().expect("temp home");
        assert!(!has_bundled_skills_cache(home.path()));
        install_system_skills(home.path()).expect("install");
        assert!(has_bundled_skills_cache(home.path()));
        uninstall_system_skills(home.path());
        assert!(!has_bundled_skills_cache(home.path()));
    }

    #[test]
    #[serial]
    fn grandfather_detects_legacy_codex_marker() {
        let legacy = tempfile::TempDir::new().expect("legacy home");
        let guard = vtcode_commons::env_lock::lock();
        let previous = std::env::var_os("CODEX_HOME");
        guard.set_var("CODEX_HOME", legacy.path());
        // A hermetic VTCODE_DATA keeps the unified-home probe hermetic too.
        let data = tempfile::TempDir::new().expect("data dir");
        let previous_data = std::env::var_os("VTCODE_DATA");
        guard.set_var("VTCODE_DATA", data.path());

        assert!(!legacy_bundled_skills_cache_present());
        assert!(!bundled_skills_grandfathered());
        install_system_skills(legacy.path()).expect("legacy install");
        assert!(legacy_bundled_skills_cache_present());
        assert!(bundled_skills_grandfathered());

        guard.restore_var("CODEX_HOME", previous.as_deref());
        guard.restore_var("VTCODE_DATA", previous_data.as_deref());
    }

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
