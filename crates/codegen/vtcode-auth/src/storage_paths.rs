use anyhow::{Context, Result, anyhow};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::{LazyLock, Mutex};
use tempfile::Builder;
use vtcode_commons::VtCodePaths;

#[cfg(test)]
static AUTH_DIR_OVERRIDE: LazyLock<Mutex<Option<PathBuf>>> = LazyLock::new(|| Mutex::new(None));

pub(crate) fn auth_storage_dir() -> Result<PathBuf> {
    #[cfg(test)]
    if let Some(path) = AUTH_DIR_OVERRIDE
        .lock()
        .map_err(|_| anyhow!("auth storage override mutex poisoned"))?
        .clone()
    {
        fs::create_dir_all(&path).context("failed to create auth directory")?;
        set_private_directory_permissions(&path)?;
        return Ok(path);
    }

    VtCodePaths::resolve()?
        .ensure_auth_dir()
        .context("failed to create auth directory")
}

pub(crate) fn legacy_auth_storage_paths() -> Result<Vec<PathBuf>> {
    #[cfg(test)]
    if let Some(path) = AUTH_DIR_OVERRIDE
        .lock()
        .map_err(|_| anyhow!("auth storage override mutex poisoned"))?
        .clone()
    {
        return Ok(vec![path.join("auth.json")]);
    }

    let paths = VtCodePaths::resolve()?;
    let mut candidates = vec![
        paths.auth_file(),
        paths.legacy_dir().join("auth.json"),
        paths.legacy_dir().join("auth").join("auth.json"),
    ];
    candidates.dedup();
    Ok(candidates)
}

pub(crate) fn legacy_auth_file_paths(file_name: &str) -> Result<Vec<PathBuf>> {
    if file_name.is_empty() || file_name.contains('/') || file_name.contains('\\') {
        return Err(anyhow!("invalid legacy auth file name: {file_name}"));
    }

    #[cfg(test)]
    if let Some(path) = AUTH_DIR_OVERRIDE
        .lock()
        .map_err(|_| anyhow!("auth storage override mutex poisoned"))?
        .clone()
    {
        return Ok(vec![path.join(file_name)]);
    }

    let paths = VtCodePaths::resolve()?;
    let mut candidates = vec![
        paths.auth_dir().join(file_name),
        paths.legacy_dir().join("auth").join(file_name),
        paths.legacy_dir().join(file_name),
    ];
    candidates.dedup();
    Ok(candidates)
}

pub(crate) fn write_private_file(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("private file path {} has no parent directory", path.display()))?;
    let _private_parent = VtCodePaths::ensure_user_dir(parent)
        .with_context(|| format!("failed to validate private file directory {}", parent.display()))?;
    let mut temp = Builder::new()
        .prefix(".tmp.")
        .tempfile_in(parent)
        .with_context(|| format!("failed to create temporary file in {}", parent.display()))?;

    #[cfg(unix)]
    set_private_permissions(temp.as_file(), temp.path())?;
    temp.as_file_mut()
        .write_all(contents)
        .with_context(|| format!("failed to write private file {}", path.display()))?;
    temp.as_file()
        .sync_all()
        .with_context(|| format!("failed to sync private file {}", path.display()))?;

    let _persisted = temp
        .persist(path)
        .with_context(|| format!("failed to persist private file {}", path.display()))?;
    #[cfg(unix)]
    set_private_path_permissions(path)?;
    Ok(())
}

pub(crate) fn read_private_file(path: &Path) -> Result<Option<Vec<u8>>> {
    read_file_with_policy(path, true)
}

/// Reads a rollback-only legacy credential file without weakening the policy
/// for newly-owned credential storage. The caller must migrate the contents
/// into a private destination before treating them as current credentials.
pub(crate) fn read_legacy_file(path: &Path) -> Result<Option<Vec<u8>>> {
    read_file_with_policy(path, false)
}

/// Read a compatibility credential path with the policy for the root it
/// belongs to. Canonical auth files are newly owned and must remain private;
/// files under the preserved legacy root are read-only rollback inputs.
pub(crate) fn read_legacy_compatible_file(path: &Path) -> Result<Option<Vec<u8>>> {
    let canonical_auth_dir = VtCodePaths::resolve()?.auth_dir();
    if path.starts_with(canonical_auth_dir) {
        read_private_file(path)
    } else {
        read_legacy_file(path)
    }
}

#[cfg_attr(
    not(unix),
    allow(
        unused_variables,
        reason = "enforce_private_permissions only gates Unix permission checks"
    )
)]
fn read_file_with_policy(path: &Path, enforce_private_permissions: bool) -> Result<Option<Vec<u8>>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("failed to inspect private file {}", path.display())),
    };
    if metadata.file_type().is_symlink() {
        return Err(anyhow!("refusing to read symlinked private file {}", path.display()));
    }
    if !metadata.is_file() {
        return Err(anyhow!("private credential path is not a regular file: {}", path.display()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if enforce_private_permissions && metadata.permissions().mode() & 0o077 != 0 {
            return Err(anyhow!("refusing to read private file with unsafe permissions {}", path.display()));
        }
    }

    let mut options = fs::OpenOptions::new();
    let _ = options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let _ = options.custom_flags(nix::libc::O_NOFOLLOW);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("failed to open private file {}", path.display()))?;
    let mut contents = Vec::new();
    let _ = file
        .read_to_end(&mut contents)
        .with_context(|| format!("failed to read private file {}", path.display()))?;
    Ok(Some(contents))
}

#[cfg(unix)]
fn set_private_directory_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("failed to set permissions on {}", path.display()))
}

#[cfg(not(unix))]
fn set_private_directory_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_private_permissions(file: &fs::File, path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    file.set_permissions(fs::Permissions::from_mode(0o600))
        .with_context(|| format!("failed to set permissions on {}", path.display()))
}

#[cfg(unix)]
fn set_private_path_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("failed to set permissions on {}", path.display()))
}

#[cfg(test)]
pub(crate) fn set_auth_storage_dir_override_for_tests(path: Option<PathBuf>) -> Result<()> {
    let mut override_path = AUTH_DIR_OVERRIDE
        .lock()
        .map_err(|_| anyhow!("auth storage override mutex poisoned"))?;
    *override_path = path;
    Ok(())
}

#[cfg(test)]
pub(crate) fn auth_storage_dir_override_for_tests() -> Result<Option<PathBuf>> {
    AUTH_DIR_OVERRIDE
        .lock()
        .map_err(|_| anyhow!("auth storage override mutex poisoned"))
        .map(|path| path.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::TempDir;
    use vtcode_commons::env_lock;

    #[test]
    #[serial]
    fn auth_storage_dir_follows_the_current_vtcode_config_environment() {
        let env = env_lock::lock();
        let first = TempDir::new().expect("create first config root");
        let second = TempDir::new().expect("create second config root");
        let previous_config = std::env::var_os("VTCODE_CONFIG");

        env.set_var("VTCODE_CONFIG", first.path());
        assert_eq!(auth_storage_dir().expect("resolve first auth directory"), first.path().join("auth"));

        env.set_var("VTCODE_CONFIG", second.path());
        assert_eq!(auth_storage_dir().expect("resolve second auth directory"), second.path().join("auth"));

        env.restore_var("VTCODE_CONFIG", previous_config);
    }

    #[test]
    #[serial]
    fn legacy_auth_paths_include_explicit_legacy_root_and_nested_auth_file() {
        let env = env_lock::lock();
        let config = TempDir::new().expect("create config root");
        let legacy = TempDir::new().expect("create legacy root");
        let previous_config = std::env::var_os("VTCODE_CONFIG");
        let previous_home = std::env::var_os("VTCODE_HOME");
        env.set_var("VTCODE_CONFIG", config.path());
        env.set_var("VTCODE_HOME", legacy.path());

        assert_eq!(
            legacy_auth_storage_paths().expect("resolve legacy auth paths"),
            vec![
                config.path().join("auth/auth.json"),
                legacy.path().join("auth.json"),
                legacy.path().join("auth/auth.json"),
            ]
        );

        env.restore_var("VTCODE_CONFIG", previous_config);
        env.restore_var("VTCODE_HOME", previous_home);
    }

    #[cfg(unix)]
    #[test]
    fn read_private_file_rejects_group_or_world_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("auth temp directory");
        let path = directory.path().join("auth.json");
        fs::write(&path, "{}\n").expect("auth file");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("unsafe permissions");

        assert!(read_private_file(&path).is_err());
    }

    #[cfg(unix)]
    #[test]
    #[serial]
    fn canonical_compatibility_files_require_private_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let env = env_lock::lock();
        let config_root = TempDir::new().expect("config root");
        let previous_config = std::env::var_os("VTCODE_CONFIG");
        env.set_var("VTCODE_CONFIG", config_root.path());

        let auth_dir = config_root.path().join("auth");
        fs::create_dir_all(&auth_dir).expect("auth directory");
        let path = auth_dir.join("legacy-session.json");
        fs::write(&path, "{}\n").expect("credential file");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("unsafe permissions");

        assert!(read_legacy_compatible_file(&path).is_err());

        env.restore_var("VTCODE_CONFIG", previous_config);
    }
}
