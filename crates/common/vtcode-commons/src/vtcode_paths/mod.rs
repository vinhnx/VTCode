//! Native and XDG-compliant storage paths for VT Code.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use fs2::FileExt;

#[path = "../vtcode_paths_migration.rs"]
mod migration;
pub use migration::{
    LegacyMigrator, MigrationEntry, MigrationFailure, MigrationReport, MigrationSkip, MigrationSkipReason,
};

const APP: &str = "vtcode";
const MARKER: &str = "legacy-v1.complete";
const PRIVATE_FILE_LOCK_ATTEMPTS: usize = 100;
const PRIVATE_FILE_LOCK_DELAY: Duration = Duration::from_millis(10);

struct NativeRoots {
    config_dir: PathBuf,
    data_dir: PathBuf,
    state_dir: PathBuf,
    cache_dir: PathBuf,
    runtime_dir: Option<PathBuf>,
    executable_dir: PathBuf,
}

fn native_roots(
    #[cfg_attr(
        target_os = "windows",
        allow(
            unused_variables,
            reason = "home_dir is only consumed by macOS and XDG root resolution; Windows uses known-folder APIs"
        )
    )]
    home_dir: &Path,
) -> Result<NativeRoots> {
    #[cfg(target_os = "macos")]
    {
        let root = dirs::data_local_dir()
            .ok_or_else(|| anyhow!("could not determine the macOS application support directory"))?
            .join("com.vinhnx.vtcode");
        Ok(NativeRoots {
            config_dir: root.clone(),
            data_dir: root.clone(),
            state_dir: root.join("state"),
            cache_dir: dirs::cache_dir()
                .ok_or_else(|| anyhow!("could not determine the macOS cache directory"))?
                .join("com.vinhnx.vtcode"),
            runtime_dir: None,
            executable_dir: home_dir.join(".local/bin"),
        })
    }
    #[cfg(target_os = "windows")]
    {
        let root = dirs::data_dir()
            .ok_or_else(|| anyhow!("could not determine the Windows application data directory"))?
            .join("vinhnx")
            .join(APP);
        Ok(NativeRoots {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            state_dir: root.join("state"),
            cache_dir: root.join("cache"),
            runtime_dir: None,
            executable_dir: root.join("bin"),
        })
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Ok(NativeRoots {
            config_dir: home_dir.join(".config").join(APP),
            data_dir: home_dir.join(".local/share").join(APP),
            state_dir: home_dir.join(".local/state").join(APP),
            cache_dir: home_dir.join(".cache").join(APP),
            runtime_dir: None,
            executable_dir: home_dir.join(".local/bin"),
        })
    }
}

/// Validated native/XDG storage roots and typed VT Code child paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VtCodePaths {
    config_dir: PathBuf,
    data_dir: PathBuf,
    state_dir: PathBuf,
    cache_dir: PathBuf,
    runtime_dir: PathBuf,
    executable_dir: PathBuf,
    system_config_dirs: Vec<PathBuf>,
    system_data_dirs: Vec<PathBuf>,
    legacy_home_dir: PathBuf,
}

impl VtCodePaths {
    /// Resolves paths from the process environment.
    pub fn from_env() -> Result<Self> {
        Self::from_environment_os(&std::env::vars_os().collect())
    }

    /// Alias for [`Self::from_env`], useful during startup resolution.
    pub fn resolve() -> Result<Self> {
        Self::from_env()
    }

    /// Resolves paths from an explicit environment, suitable for deterministic tests.
    pub fn from_environment(environment: &[(&str, &str)]) -> Result<Self> {
        Self::from_environment_os(
            &environment
                .iter()
                .map(|(key, value)| (OsString::from(key), OsString::from(value)))
                .collect(),
        )
    }

    fn from_environment_os(environment: &BTreeMap<OsString, OsString>) -> Result<Self> {
        let home_dir = environment
            .get(OsStr::new("HOME"))
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(dirs::home_dir)
            .ok_or_else(|| anyhow!("could not determine the user home directory"))?;
        let native = native_roots(&home_dir)?;
        let home_override = env_path(environment, "VTCODE_HOME");
        if let Some(path) = &home_override {
            validate_absolute("VTCODE_HOME", path)?;
        }
        let legacy_home_dir = home_override.clone().unwrap_or_else(|| home_dir.join(".vtcode"));

        let config_dir = match env_path(environment, "VTCODE_CONFIG") {
            Some(path) => {
                validate_absolute("VTCODE_CONFIG", &path)?;
                path
            }
            None => xdg_app_dir(environment, "XDG_CONFIG_HOME", &native.config_dir)?,
        };
        let data_dir = match env_path(environment, "VTCODE_DATA") {
            Some(path) => {
                validate_absolute("VTCODE_DATA", &path)?;
                path
            }
            None => xdg_app_dir(environment, "XDG_DATA_HOME", &native.data_dir)?,
        };
        let state_dir = xdg_app_dir(environment, "XDG_STATE_HOME", &native.state_dir)?;
        let cache_dir = xdg_app_dir(environment, "XDG_CACHE_HOME", &native.cache_dir)?;
        let runtime_dir = match () {
            _ if is_xdg_platform() => match env_path(environment, "XDG_RUNTIME_DIR") {
                Some(path) if path.is_absolute() => path.join(APP),
                None => state_dir.join("runtime"),
                Some(_) => state_dir.join("runtime"),
            },
            _ => native.runtime_dir.unwrap_or_else(|| state_dir.join("runtime")),
        };
        for (name, path) in [
            ("configuration directory", &config_dir),
            ("data directory", &data_dir),
            ("state directory", &state_dir),
            ("cache directory", &cache_dir),
            ("runtime directory", &runtime_dir),
        ] {
            validate_absolute(name, path)?;
        }
        Ok(Self {
            config_dir,
            data_dir,
            state_dir,
            cache_dir,
            runtime_dir,
            executable_dir: executable_dir(environment, native.executable_dir)?,
            system_config_dirs: system_config_dirs(environment)?,
            system_data_dirs: system_data_dirs(environment)?,
            legacy_home_dir,
        })
    }

    /// Canonical directory for user configuration.
    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }
    /// Canonical directory for durable user data.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }
    /// Canonical directory for durable mutable state.
    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }
    /// Canonical directory for recreatable cached data.
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }
    /// Private directory for runtime-only files.
    pub fn runtime_dir(&self) -> &Path {
        &self.runtime_dir
    }
    /// User executable helper directory.
    pub fn executable_dir(&self) -> &Path {
        &self.executable_dir
    }
    /// System configuration candidates, lowest priority first.
    pub fn system_config_dirs(&self) -> &[PathBuf] {
        &self.system_config_dirs
    }
    /// System data candidates without an application suffix.
    pub fn system_data_dirs(&self) -> &[PathBuf] {
        &self.system_data_dirs
    }
    /// Resolves a relative config path against all system candidates in
    /// low-to-high precedence order. `system_config_dirs()` retains the XDG
    /// preference order, where the first directory is most important.
    pub fn system_config_paths(&self, relative: impl AsRef<Path>) -> Result<Vec<PathBuf>> {
        let relative = relative.as_ref();
        validate_relative_path("system configuration path", relative)?;
        let mut paths = if cfg!(unix) {
            vec![PathBuf::from("/etc/vtcode").join(relative)]
        } else {
            Vec::new()
        };
        paths.extend(self.system_config_dirs.iter().rev().map(|base| base.join(APP).join(relative)));
        paths.dedup();
        Ok(paths)
    }
    /// Resolves a relative data path against all system candidates.
    pub fn system_data_paths(&self, relative: impl AsRef<Path>) -> Result<Vec<PathBuf>> {
        let relative = relative.as_ref();
        validate_relative_path("system data path", relative)?;
        Ok(self.system_data_dirs.iter().map(|base| base.join(APP).join(relative)).collect())
    }
    /// Legacy global VT Code root, used only as a migration source.
    pub fn legacy_home_dir(&self) -> &Path {
        &self.legacy_home_dir
    }

    /// Alias for [`Self::legacy_home_dir`] using the public contract name.
    pub fn legacy_dir(&self) -> &Path {
        self.legacy_home_dir()
    }

    /// Resolve a relative child path under the canonical configuration root.
    pub fn config_path(&self, relative: impl AsRef<Path>) -> Result<PathBuf> {
        child_path(&self.config_dir, relative.as_ref(), "configuration")
    }

    /// Resolve a relative child path under the canonical data root.
    pub fn data_path(&self, relative: impl AsRef<Path>) -> Result<PathBuf> {
        child_path(&self.data_dir, relative.as_ref(), "data")
    }

    /// Resolve a relative child path under the persistent state root.
    pub fn state_path(&self, relative: impl AsRef<Path>) -> Result<PathBuf> {
        child_path(&self.state_dir, relative.as_ref(), "state")
    }

    /// Resolve a relative child path under the cache root.
    pub fn cache_path(&self, relative: impl AsRef<Path>) -> Result<PathBuf> {
        child_path(&self.cache_dir, relative.as_ref(), "cache")
    }

    /// Resolve a relative child path under the runtime root.
    pub fn runtime_path(&self, relative: impl AsRef<Path>) -> Result<PathBuf> {
        child_path(&self.runtime_dir, relative.as_ref(), "runtime")
    }

    /// Resolve a relative child path under the managed executable root.
    pub fn executable_path(&self, relative: impl AsRef<Path>) -> Result<PathBuf> {
        child_path(&self.executable_dir, relative.as_ref(), "executable")
    }

    /// Main user configuration file.
    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("vtcode.toml")
    }
    /// User-installed and downloaded skills.
    pub fn skills_dir(&self) -> PathBuf {
        self.data_dir.join("skills")
    }
    /// User-installed plugins.
    pub fn plugins_dir(&self) -> PathBuf {
        self.data_dir.join("plugins")
    }
    /// Private authentication storage.
    pub fn auth_dir(&self) -> PathBuf {
        self.config_dir.join("auth")
    }
    /// Legacy plaintext authentication file location.
    pub fn auth_file(&self) -> PathBuf {
        self.auth_dir().join("auth.json")
    }
    /// Durable log files.
    pub fn logs_dir(&self) -> PathBuf {
        self.state_dir.join("logs")
    }
    /// Persisted cross-workspace session data.
    pub fn sessions_dir(&self) -> PathBuf {
        self.state_dir.join("sessions")
    }
    /// Telemetry storage.
    pub fn telemetry_dir(&self) -> PathBuf {
        self.state_dir.join("telemetry")
    }
    /// Completion marker for one-time legacy migration.
    pub fn migration_marker_path(&self) -> PathBuf {
        self.state_dir.join("migration").join(MARKER)
    }

    /// Versioned diagnostic report emitted alongside the completion marker.
    pub fn migration_report_path(&self) -> PathBuf {
        self.state_dir.join("migration").join("legacy-v1.json")
    }

    /// Creates the runtime directory with private permissions.
    pub fn ensure_runtime_dir(&self) -> Result<&Path> {
        ensure_private_dir(&self.runtime_dir).context("could not create VT Code runtime directory")?;
        Ok(&self.runtime_dir)
    }

    /// Creates the canonical user configuration directory when needed.
    /// Existing directories retain their permissions.
    pub fn ensure_config_dir(&self) -> Result<&Path> {
        ensure_user_dir(&self.config_dir).context("could not create VT Code configuration directory")?;
        Ok(&self.config_dir)
    }

    /// Creates the canonical user data directory when needed.
    /// Existing directories retain their permissions.
    pub fn ensure_data_dir(&self) -> Result<&Path> {
        ensure_user_dir(&self.data_dir).context("could not create VT Code data directory")?;
        Ok(&self.data_dir)
    }

    /// Creates the canonical user state directory when needed.
    /// Existing directories retain their permissions.
    pub fn ensure_state_dir(&self) -> Result<&Path> {
        ensure_user_dir(&self.state_dir).context("could not create VT Code state directory")?;
        Ok(&self.state_dir)
    }

    /// Creates the canonical user cache directory when needed.
    /// Existing directories retain their permissions.
    pub fn ensure_cache_dir(&self) -> Result<&Path> {
        ensure_user_dir(&self.cache_dir).context("could not create VT Code cache directory")?;
        Ok(&self.cache_dir)
    }

    /// Creates the managed executable directory when needed.
    /// Existing directories retain their permissions.
    pub fn ensure_executable_dir(&self) -> Result<&Path> {
        ensure_user_dir(&self.executable_dir).context("could not create VT Code executable directory")?;
        Ok(&self.executable_dir)
    }

    /// Creates an arbitrary user-owned directory without following symlinks.
    /// Existing directories retain their permissions; newly created Unix
    /// directories use mode `0700`.
    pub fn ensure_user_dir(path: impl AsRef<Path>) -> Result<PathBuf> {
        let path = path.as_ref();
        ensure_user_dir(path).with_context(|| format!("could not create user directory {}", path.display()))?;
        Ok(path.to_path_buf())
    }

    /// Creates a new private file without following a final symlink.
    ///
    /// The parent is validated component by component before the file is
    /// opened. This is the common primitive for caches, locks, and other
    /// category-owned files that must never be redirected through a symlink.
    pub fn create_private_file(path: impl AsRef<Path>) -> Result<File> {
        let path = path.as_ref();
        ensure_file_parent(path)?;
        create_private_new_file(path).with_context(|| format!("could not create private file {}", path.display()))
    }

    /// Opens an append-only private file without following a final symlink.
    pub fn open_private_append_file(path: impl AsRef<Path>) -> Result<File> {
        let path = path.as_ref();
        ensure_file_parent(path)?;
        open_private_append(path).with_context(|| format!("could not open private file {}", path.display()))
    }

    /// Reads a regular file without following a final symlink.
    pub fn read_file_no_follow(path: impl AsRef<Path>) -> Result<Vec<u8>> {
        let path = path.as_ref();
        validate_no_escaping_symlink_ancestors(path, false)
            .with_context(|| format!("could not validate file path {}", path.display()))?;
        let mut file = open_no_follow(path).with_context(|| format!("could not open file {}", path.display()))?;
        let metadata = file
            .metadata()
            .with_context(|| format!("could not inspect file {}", path.display()))?;
        if !metadata.is_file() {
            bail!("{} is not a regular file", path.display());
        }
        let mut contents = Vec::new();
        let _bytes_read = file
            .read_to_end(&mut contents)
            .with_context(|| format!("could not read file {}", path.display()))?;
        Ok(contents)
    }

    /// Atomically writes a private file, replacing an existing regular file.
    ///
    /// The destination is never opened for writing. A private, exclusive
    /// temporary file is written and then renamed into place, so a pre-existing
    /// symlink cannot redirect the contents outside the approved parent.
    pub fn write_private_file_atomic(path: impl AsRef<Path>, contents: &[u8]) -> Result<()> {
        let destination = path.as_ref();
        ensure_file_parent(destination)?;
        validate_file_destination(destination)?;
        let parent = destination
            .parent()
            .ok_or_else(|| anyhow!("private file {} has no parent", destination.display()))?;
        let stem = destination.file_name().unwrap_or_else(|| OsStr::new("file"));
        let (temporary, mut file) = unique_private_file(parent, stem)?;
        let result: io::Result<()> = (|| {
            file.write_all(contents)?;
            file.sync_all()?;
            drop(file);
            // rename replaces existing files on Windows too. Removing the
            // destination first would lose the last committed file on failure.
            fs::rename(&temporary, destination)
        })();
        if result.is_err() {
            remove_temporary_file(&temporary);
        }
        result.with_context(|| format!("could not atomically write {}", destination.display()))
    }

    /// Atomically publishes a private file only when the destination is absent.
    ///
    /// The temporary file is linked into place instead of renamed over the
    /// destination. This makes legacy-cache republishing safe when multiple
    /// VT Code processes initialize the same cache concurrently: the first
    /// publisher wins and a newer canonical cache cannot be clobbered.
    pub fn write_private_file_atomic_if_absent(path: impl AsRef<Path>, contents: &[u8]) -> Result<bool> {
        let destination = path.as_ref();
        ensure_file_parent(destination)?;
        validate_file_destination(destination)?;
        if fs::symlink_metadata(destination).is_ok() {
            return Ok(false);
        }

        let parent = destination
            .parent()
            .ok_or_else(|| anyhow!("private file {} has no parent", destination.display()))?;
        let stem = destination.file_name().unwrap_or_else(|| OsStr::new("file"));
        let (temporary, mut file) = unique_private_file(parent, stem)?;
        let result: io::Result<bool> = (|| {
            file.write_all(contents)?;
            file.sync_all()?;
            drop(file);
            match fs::hard_link(&temporary, destination) {
                Ok(()) => Ok(true),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
                Err(error) => Err(error),
            }
        })();
        remove_temporary_file(&temporary);
        result.with_context(|| format!("could not atomically create {}", destination.display()))
    }

    /// Runs an operation while holding an exclusive private lock adjacent to a file.
    ///
    /// Lock acquisition is bounded and released by the operating system when
    /// the owning process exits, so an abandoned lock cannot permanently
    /// disable cache writes.
    pub fn with_private_file_lock<T>(path: impl AsRef<Path>, operation: impl FnOnce() -> Result<T>) -> Result<T> {
        let destination = path.as_ref();
        ensure_file_parent(destination)?;
        let parent = destination
            .parent()
            .ok_or_else(|| anyhow!("private file {} has no parent", destination.display()))?;
        let stem = destination.file_name().unwrap_or_else(|| OsStr::new("file"));
        let lock_path = parent.join(format!(".{}.lock", stem.to_string_lossy()));
        let _lock = acquire_private_file_lock(&lock_path)?;
        operation()
    }

    /// Creates a private runtime child directory after validating its path.
    pub fn ensure_runtime_child_dir(&self, relative: impl AsRef<Path>) -> Result<PathBuf> {
        let path = self.runtime_path(relative)?;
        ensure_private_dir(&path).context("could not create VT Code runtime child directory")?;
        Ok(path)
    }

    /// Creates a user configuration child directory after validating its path.
    pub fn ensure_config_child_dir(&self, relative: impl AsRef<Path>) -> Result<PathBuf> {
        let path = self.config_path(relative)?;
        ensure_user_dir(&path).context("could not create VT Code configuration child directory")?;
        Ok(path)
    }

    /// Creates a user data child directory after validating its path.
    pub fn ensure_data_child_dir(&self, relative: impl AsRef<Path>) -> Result<PathBuf> {
        let path = self.data_path(relative)?;
        ensure_user_dir(&path).context("could not create VT Code data child directory")?;
        Ok(path)
    }

    /// Creates a user state child directory after validating its path.
    pub fn ensure_state_child_dir(&self, relative: impl AsRef<Path>) -> Result<PathBuf> {
        let path = self.state_path(relative)?;
        ensure_user_dir(&path).context("could not create VT Code state child directory")?;
        Ok(path)
    }

    /// Creates a user cache child directory after validating its path.
    pub fn ensure_cache_child_dir(&self, relative: impl AsRef<Path>) -> Result<PathBuf> {
        let path = self.cache_path(relative)?;
        ensure_user_dir(&path).context("could not create VT Code cache child directory")?;
        Ok(path)
    }

    /// Creates a managed executable child directory after validating its path.
    pub fn ensure_executable_child_dir(&self, relative: impl AsRef<Path>) -> Result<PathBuf> {
        let path = self.executable_path(relative)?;
        ensure_user_dir(&path).context("could not create VT Code executable child directory")?;
        Ok(path)
    }

    /// Creates private auth storage and returns its path.
    pub fn ensure_auth_dir(&self) -> Result<PathBuf> {
        let path = self.auth_dir();
        ensure_private_dir(&path).context("could not create VT Code authentication directory")?;
        Ok(path)
    }

    /// Creates an empty, private auth file. The name must not contain a path.
    pub fn create_auth_file(&self, name: impl AsRef<str>) -> Result<PathBuf> {
        let name = name.as_ref();
        if !is_safe_file_name(name) {
            bail!("authentication file name '{name}' must be one normal path component");
        }
        let path = self.ensure_auth_dir()?.join(name);
        let _file = create_private_new_file(&path)
            .with_context(|| format!("could not create authentication file {}", path.display()))?;
        Ok(path)
    }

    /// Copies eligible legacy global data without modifying its source.
    pub fn migrate_legacy(&self) -> Result<MigrationReport> {
        LegacyMigrator::new(self.clone()).run()
    }
}

fn env_path(environment: &BTreeMap<OsString, OsString>, name: &str) -> Option<PathBuf> {
    environment
        .get(OsStr::new(name))
        .filter(|value| !value.is_empty())
        .and_then(|value| {
            if let Some(text) = value.to_str() {
                let trimmed = text.trim();
                (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
            } else {
                Some(PathBuf::from(value))
            }
        })
}
fn xdg_app_dir(environment: &BTreeMap<OsString, OsString>, name: &str, native: &Path) -> Result<PathBuf> {
    if is_xdg_platform()
        && let Some(path) = env_path(environment, name)
        && path.is_absolute()
    {
        return Ok(path.join(APP));
    }
    Ok(native.to_path_buf())
}
fn executable_dir(environment: &BTreeMap<OsString, OsString>, native: PathBuf) -> Result<PathBuf> {
    if is_xdg_platform()
        && let Some(path) = env_path(environment, "XDG_BIN_HOME")
        && path.is_absolute()
    {
        return Ok(path);
    }
    Ok(native)
}
fn system_config_dirs(environment: &BTreeMap<OsString, OsString>) -> Result<Vec<PathBuf>> {
    if !is_xdg_platform() {
        return Ok(Vec::new());
    }
    let configured = environment.get(OsStr::new("XDG_CONFIG_DIRS")).map(OsString::as_os_str);
    let mut paths = configured
        .into_iter()
        .flat_map(std::env::split_paths)
        .filter(|path| path.is_absolute())
        .collect::<Vec<_>>();
    if paths.is_empty() {
        paths.push(PathBuf::from("/etc/xdg"));
    }
    Ok(paths)
}
fn system_data_dirs(environment: &BTreeMap<OsString, OsString>) -> Result<Vec<PathBuf>> {
    if !is_xdg_platform() {
        return Ok(Vec::new());
    }
    let configured = environment.get(OsStr::new("XDG_DATA_DIRS")).map(OsString::as_os_str);
    let mut paths = configured
        .into_iter()
        .flat_map(std::env::split_paths)
        .filter(|path| path.is_absolute())
        .collect::<Vec<_>>();
    if paths.is_empty() {
        paths.extend([PathBuf::from("/usr/local/share"), PathBuf::from("/usr/share")]);
    }
    Ok(paths)
}
fn validate_absolute(name: &str, path: &Path) -> Result<()> {
    if path.is_absolute() {
        Ok(())
    } else {
        bail!("{name} must be an absolute path, got '{}'", path.display())
    }
}
fn validate_relative_path(name: &str, path: &Path) -> Result<()> {
    if !path.as_os_str().is_empty() && path.components().all(|component| matches!(component, Component::Normal(_))) {
        Ok(())
    } else {
        bail!("{name} must be a non-empty relative path without traversal, got '{}'", path.display())
    }
}

fn child_path(root: &Path, relative: &Path, category: &str) -> Result<PathBuf> {
    validate_relative_path(&format!("{category} child path"), relative)?;
    Ok(root.join(relative))
}
const fn is_xdg_platform() -> bool {
    cfg!(any(
        target_os = "linux",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "dragonfly"
    ))
}
fn is_safe_file_name(name: &str) -> bool {
    let mut parts = Path::new(name).components();
    matches!(parts.next(), Some(Component::Normal(_))) && parts.next().is_none()
}
fn ensure_private_dir(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(io::Error::other(format!("refusing symlink directory {}", path.display())));
        }
        Ok(metadata) if !metadata.is_dir() => {
            return Err(io::Error::other(format!("{} is not a directory", path.display())));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            ensure_private_parent_dir(path)?;
            create_private_dir(path)?;
        }
        Err(error) => return Err(error),
    }
    set_private_permissions(path)
}

/// Ensure a user-owned directory exists without changing the mode of any
/// directory that was already present. Every directory created by this
/// helper is private on Unix.
fn ensure_user_dir(path: &Path) -> io::Result<()> {
    validate_no_escaping_symlink_ancestors(path, true)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(io::Error::other(format!("refusing symlink directory {}", path.display())));
        }
        Ok(metadata) if !metadata.is_dir() => {
            return Err(io::Error::other(format!("{} is not a directory", path.display())));
        }
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }

    let Some(parent) = path.parent() else {
        return Err(io::Error::other(format!("{} has no parent directory", path.display())));
    };
    if parent != path {
        ensure_user_dir(parent)?;
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(io::Error::other(format!("refusing symlink directory {}", path.display())))
        }
        Ok(metadata) if !metadata.is_dir() => Err(io::Error::other(format!("{} is not a directory", path.display()))),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => create_user_dir(path),
        Err(error) => Err(error),
    }
}

fn ensure_private_parent_dir(path: &Path) -> io::Result<()> {
    validate_no_escaping_symlink_ancestors(path, true)?;
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    if parent == path {
        return Ok(());
    }
    match fs::symlink_metadata(parent) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(io::Error::other(format!("refusing symlink directory {}", parent.display())))
        }
        Ok(metadata) if !metadata.is_dir() => Err(io::Error::other(format!("{} is not a directory", parent.display()))),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            ensure_private_parent_dir(parent)?;
            create_user_dir(parent)
        }
        Err(error) => Err(error),
    }
}

/// Ensure migration-created directories are private while preserving the mode
/// of directories that already existed at the destination.
fn ensure_migration_dir(path: &Path) -> io::Result<()> {
    ensure_user_dir(path)
}

fn create_user_dir(path: &Path) -> io::Result<()> {
    match fs::create_dir(path) {
        Ok(()) => set_private_permissions(path),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                Err(io::Error::other(format!("refusing symlink directory {}", path.display())))
            }
            Ok(metadata) if !metadata.is_dir() => {
                Err(io::Error::other(format!("{} is not a directory", path.display())))
            }
            Ok(_) => Ok(()),
            Err(error) => Err(error),
        },
        Err(error) => Err(error),
    }
}

fn create_private_dir(path: &Path) -> io::Result<()> {
    match fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                Err(io::Error::other(format!("refusing symlink directory {}", path.display())))
            }
            Ok(metadata) if !metadata.is_dir() => {
                Err(io::Error::other(format!("{} is not a directory", path.display())))
            }
            Ok(_) => Ok(()),
            Err(error) => Err(error),
        },
        Err(error) => Err(error),
    }
}

fn create_private_new_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    let _ = options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let _ = options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path)
}
fn open_no_follow(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    let _ = options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let _ = options.custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path)
}

fn open_private_append(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    let _ = options.create(true).append(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let _ = options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        let file = options.open(path)?;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        Ok(file)
    }
    #[cfg(not(unix))]
    options.open(path)
}

fn ensure_file_parent(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("file {} has no parent directory", path.display()))?;
    let parent = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    ensure_user_dir(parent).with_context(|| format!("could not create file parent {}", parent.display()))?;
    Ok(())
}

fn validate_file_destination(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!("refusing to replace symlinked file {}", path.display())
        }
        Ok(metadata) if !metadata.is_file() => bail!("{} is not a regular file", path.display()),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("could not inspect {}", path.display())),
    }
}

/// Validate every existing path component and reject escaping symlinks.
///
/// Platform aliases such as macOS `/tmp -> /private/tmp` are accepted when a
/// symlink resolves beneath its containing directory. Missing trailing
/// components are allowed so callers can create them safely afterwards.
fn validate_no_escaping_symlink_ancestors(path: &Path, allow_missing_tail: bool) -> io::Result<()> {
    let components = path.components().collect::<Vec<_>>();
    let mut current = PathBuf::new();
    for (index, component) in components.iter().enumerate() {
        if matches!(component, Component::ParentDir) {
            return Err(io::Error::other(format!("path contains traversal: {}", path.display())));
        }
        if matches!(component, Component::CurDir) {
            continue;
        }
        current.push(component.as_os_str());
        let is_leaf = index + 1 == components.len();
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if allow_missing_tail && error.kind() == io::ErrorKind::NotFound => break,
            Err(error) => return Err(error),
        };
        if metadata.file_type().is_symlink() {
            if is_leaf {
                return Err(io::Error::other(format!("refusing symlink path {}", current.display())));
            }
            let parent = current
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            let canonical_parent = crate::canonicalize(parent)?;
            let canonical_target = crate::canonicalize(&current)?;
            if !canonical_target.starts_with(&canonical_parent) {
                return Err(io::Error::other(format!(
                    "path component {} escapes its containing directory",
                    current.display()
                )));
            }
            if !fs::metadata(&current)?.is_dir() {
                return Err(io::Error::other(format!("path component {} is not a directory", current.display())));
            }
        } else if !is_leaf && !metadata.is_dir() {
            return Err(io::Error::other(format!("path component {} is not a directory", current.display())));
        }
    }
    Ok(())
}

fn unique_private_file(parent: &Path, stem: &OsStr) -> Result<(PathBuf, File)> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let stem = stem.to_string_lossy();
    for attempt in 0..32u8 {
        let temporary = parent.join(format!(".{stem}.{}.{}.{}.tmp", std::process::id(), timestamp, attempt));
        match create_private_new_file(&temporary) {
            Ok(file) => return Ok((temporary, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error).with_context(|| format!("could not create {}", temporary.display())),
        }
    }
    bail!("could not allocate a unique private temporary file in {}", parent.display())
}

struct PrivateFileLock {
    _file: File,
}

fn acquire_private_file_lock(path: &Path) -> Result<PrivateFileLock> {
    let file = open_private_lock_file(path)?;
    for attempt in 0..PRIVATE_FILE_LOCK_ATTEMPTS {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(PrivateFileLock { _file: file }),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if attempt + 1 < PRIVATE_FILE_LOCK_ATTEMPTS {
                    std::thread::sleep(PRIVATE_FILE_LOCK_DELAY);
                }
            }
            Err(error) => return Err(error).with_context(|| format!("could not lock {}", path.display())),
        }
    }
    bail!("timed out waiting for private file lock {}", path.display())
}

fn open_private_lock_file(path: &Path) -> Result<File> {
    match create_private_lock_file(path) {
        Ok(file) => Ok(file),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            validate_file_destination(path)?;
            let mut options = OpenOptions::new();
            let _ = options.read(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                let _ = options.custom_flags(libc::O_NOFOLLOW);
            }
            options
                .open(path)
                .with_context(|| format!("could not open private lock {}", path.display()))
        }
        Err(error) => Err(error).with_context(|| format!("could not create lock {}", path.display())),
    }
}

fn create_private_lock_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    let _ = options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let _ = options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path)
}

impl Drop for PrivateFileLock {
    fn drop(&mut self) {
        if let Err(error) = self._file.unlock() {
            tracing::debug!(%error, "failed to release private file lock");
        }
    }
}

fn remove_temporary_file(path: &Path) {
    if let Err(error) = fs::remove_file(path)
        && error.kind() != io::ErrorKind::NotFound
    {
        tracing::debug!(path = %path.display(), %error, "failed to remove private temporary file");
    }
}
#[cfg_attr(
    not(unix),
    allow(unused_variables, reason = "only the Unix permission block consumes the path")
)]
fn set_private_permissions(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
