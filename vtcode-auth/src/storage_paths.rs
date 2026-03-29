use anyhow::{Context, Result, anyhow};
use std::path::PathBuf;
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
        std::fs::create_dir_all(&path).context("failed to create auth directory")?;
        return Ok(path);
    }

    let auth_dir = dirs::home_dir()
        .ok_or_else(|| anyhow!("could not determine home directory"))?
        .join(".vtcode")
        .join("auth");

    std::fs::create_dir_all(&auth_dir).context("failed to create auth directory")?;
    Ok(auth_dir)
}

pub(crate) fn legacy_auth_storage_path() -> Result<PathBuf> {
    #[cfg(test)]
    if let Some(path) = AUTH_DIR_OVERRIDE
        .lock()
        .map_err(|_| anyhow!("auth storage override mutex poisoned"))?
        .clone()
    {
        std::fs::create_dir_all(&path).context("failed to create auth directory")?;
        return Ok(path.join("auth.json"));
    }

    let base_dir = dirs::home_dir()
        .ok_or_else(|| anyhow!("could not determine home directory"))?
        .join(".vtcode");

    std::fs::create_dir_all(&base_dir).context("failed to create auth directory")?;
    Ok(base_dir.join("auth.json"))
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
