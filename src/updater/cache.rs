use anyhow::{Context, Result};
use std::path::PathBuf;
use std::time::Duration;
use vtcode_core::utils::file_utils::{ensure_dir_exists_sync, write_file_with_context_sync};

pub(super) fn is_check_due(min_interval: Duration) -> Result<bool> {
    let cache_dir = get_cache_dir()?;
    let last_check_file = cache_dir.join("last_update_check");

    if !last_check_file.exists() {
        return Ok(true);
    }

    let metadata = std::fs::metadata(&last_check_file)
        .context("Failed to read last update check timestamp")?;
    let modified = metadata
        .modified()
        .context("Failed to get modification time")?;
    let elapsed = std::time::SystemTime::now()
        .duration_since(modified)
        .context("Failed to calculate elapsed time")?;

    Ok(elapsed >= min_interval)
}

pub(super) fn record_update_check() -> Result<()> {
    let cache_dir = get_cache_dir()?;
    write_file_with_context_sync(
        &cache_dir.join("last_update_check"),
        "",
        "update check timestamp",
    )
    .context("Failed to record update check timestamp")?;
    Ok(())
}

fn get_cache_dir() -> Result<PathBuf> {
    let dir = if let Ok(xdg_cache) = std::env::var("XDG_CACHE_HOME") {
        PathBuf::from(xdg_cache).join("vtcode")
    } else {
        let home = dirs::home_dir().context("Cannot determine home directory")?;
        home.join(".cache/vtcode")
    };

    ensure_dir_exists_sync(&dir).context("Failed to create cache directory")?;
    Ok(dir)
}
