use anyhow::{Context, Result, anyhow};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::{LazyLock, Mutex};

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
        return Ok(path);
    }

    let auth_dir = dirs::home_dir()
        .ok_or_else(|| anyhow!("could not determine home directory"))?
        .join(".vtcode")
        .join("auth");

    fs::create_dir_all(&auth_dir).context("failed to create auth directory")?;
    Ok(auth_dir)
}

pub(crate) fn legacy_auth_storage_path() -> Result<PathBuf> {
    #[cfg(test)]
    if let Some(path) = AUTH_DIR_OVERRIDE
        .lock()
        .map_err(|_| anyhow!("auth storage override mutex poisoned"))?
        .clone()
    {
        fs::create_dir_all(&path).context("failed to create auth directory")?;
        return Ok(path.join("auth.json"));
    }

    let base_dir = dirs::home_dir()
        .ok_or_else(|| anyhow!("could not determine home directory"))?
        .join(".vtcode");

    fs::create_dir_all(&base_dir).context("failed to create auth directory")?;
    Ok(base_dir.join("auth.json"))
}

#[cfg(unix)]
pub(crate) fn write_private_file(path: &Path, contents: &[u8]) -> Result<()> {
    use std::fs::OpenOptions;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("failed to open private file {}", path.display()))?;
    file.write_all(contents)
        .with_context(|| format!("failed to write private file {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("failed to sync private file {}", path.display()))?;
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .with_context(|| format!("failed to set permissions on {}", path.display()))?;
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn write_private_file(path: &Path, contents: &[u8]) -> Result<()> {
    fs::write(path, contents)
        .with_context(|| format!("failed to write private file {}", path.display()))
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
