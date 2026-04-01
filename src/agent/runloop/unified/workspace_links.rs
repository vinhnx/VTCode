use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

#[derive(Clone)]
pub(crate) struct LinkedDirectory {
    pub(crate) original: PathBuf,
    pub(crate) link_path: PathBuf,
    pub(crate) display_path: String,
}

pub(crate) async fn remove_directory_symlink(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        if let Err(err) = tokio::fs::remove_file(path).await
            && err.kind() != ErrorKind::NotFound
        {
            return Err(err)
                .with_context(|| format!("failed to remove directory link {}", path.display()));
        }
    }

    #[cfg(windows)]
    {
        if let Err(err) = tokio::fs::remove_dir(path).await {
            if err.kind() != ErrorKind::NotFound {
                return Err(err).with_context(|| {
                    format!("failed to remove directory link {}", path.display())
                });
            }
        }
    }

    Ok(())
}
