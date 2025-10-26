//! Installation manager for updates

use super::config::UpdateConfig;
use anyhow::{Context, Result};
use std::path::Path;

/// Handles installation of downloaded updates
pub struct UpdateInstaller {
    config: UpdateConfig,
}

impl UpdateInstaller {
    pub fn new(config: UpdateConfig) -> Result<Self> {
        Ok(Self { config })
    }

    /// Install an update from the given path
    pub async fn install(&self, update_path: &Path) -> Result<()> {
        tracing::info!("Installing update from: {:?}", update_path);

        // Always handle as a file path since the downloader has already downloaded the file
        // Use self_update's binary replacement functionality for cross-platform compatibility
        match self_update::self_replace::self_replace(update_path) {
            Ok(_) => {
                tracing::info!("Successfully replaced binary with new version");
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to use self_replace, falling back to manual replacement: {}",
                    e
                );
                // Fallback to original file-based installation
                self.install_from_file(update_path).await?;
            }
        }

        tracing::info!("Update installation completed successfully");

        Ok(())
    }

    /// Internal method to install from a local file path (original logic)
    async fn install_from_file(&self, update_path: &Path) -> Result<()> {
        // Get the current executable path
        let current_exe =
            std::env::current_exe().context("Failed to get current executable path")?;

        tracing::info!("Current executable: {:?}", current_exe);

        // Extract the update if it's an archive
        let binary_path = if self.is_archive(update_path) {
            self.extract_archive(update_path).await?
        } else {
            update_path.to_path_buf()
        };

        // Set executable permissions on Unix
        #[cfg(unix)]
        self.set_executable_permissions(&binary_path)?;

        // Replace the current executable
        self.replace_executable(&binary_path, &current_exe)
            .await
            .context("Failed to replace executable")?;

        Ok(())
    }

    /// Check if the file is an archive
    fn is_archive(&self, path: &Path) -> bool {
        if let Some(ext) = path.extension() {
            let ext = ext.to_string_lossy().to_lowercase();
            matches!(ext.as_str(), "tar" | "gz" | "tgz" | "zip" | "bz2" | "xz")
        } else {
            false
        }
    }

    /// Extract an archive and return the path to the binary
    async fn extract_archive(&self, archive_path: &Path) -> Result<std::path::PathBuf> {
        let extract_dir = self.config.update_dir.join("extracted");
        tokio::fs::create_dir_all(&extract_dir)
            .await
            .context("Failed to create extraction directory")?;

        tracing::info!("Extracting archive to: {:?}", extract_dir);

        // Determine archive type and extract
        if let Some(ext) = archive_path.extension() {
            let ext = ext.to_string_lossy().to_lowercase();

            match ext.as_str() {
                "zip" => self.extract_zip(archive_path, &extract_dir).await?,
                "tar" | "tgz" | "gz" | "bz2" | "xz" => {
                    self.extract_tar(archive_path, &extract_dir).await?
                }
                _ => anyhow::bail!("Unsupported archive format: {}", ext),
            }
        } else {
            anyhow::bail!("Archive has no extension");
        }

        // Find the binary in the extracted files
        self.find_binary_in_dir(&extract_dir)
            .await
            .context("Failed to find binary in extracted archive")
    }

    /// Extract a ZIP archive
    async fn extract_zip(&self, archive_path: &Path, extract_dir: &Path) -> Result<()> {
        let file = std::fs::File::open(archive_path).context("Failed to open ZIP archive")?;
        let mut archive = zip::ZipArchive::new(file).context("Failed to read ZIP archive")?;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i).context("Failed to read ZIP entry")?;
            let outpath = extract_dir.join(file.name());

            if file.is_dir() {
                tokio::fs::create_dir_all(&outpath)
                    .await
                    .context("Failed to create directory")?;
            } else {
                if let Some(parent) = outpath.parent() {
                    tokio::fs::create_dir_all(parent)
                        .await
                        .context("Failed to create parent directory")?;
                }

                let mut outfile =
                    std::fs::File::create(&outpath).context("Failed to create output file")?;
                std::io::copy(&mut file, &mut outfile).context("Failed to extract file")?;
            }

            // Set permissions on Unix
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Some(mode) = file.unix_mode() {
                    std::fs::set_permissions(&outpath, std::fs::Permissions::from_mode(mode)).ok();
                }
            }
        }

        Ok(())
    }

    /// Extract a TAR archive (including compressed variants)
    async fn extract_tar(&self, archive_path: &Path, extract_dir: &Path) -> Result<()> {
        let file = std::fs::File::open(archive_path).context("Failed to open TAR archive")?;

        // Determine if the archive is compressed
        let ext = archive_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        let decoder: Box<dyn std::io::Read> = match ext {
            "gz" | "tgz" => Box::new(flate2::read::GzDecoder::new(file)),
            "bz2" => Box::new(bzip2::read::BzDecoder::new(file)),
            "xz" => Box::new(xz2::read::XzDecoder::new(file)),
            _ => Box::new(file),
        };

        let mut archive = tar::Archive::new(decoder);
        archive
            .unpack(extract_dir)
            .context("Failed to extract TAR archive")?;

        Ok(())
    }

    /// Find the binary executable in a directory
    async fn find_binary_in_dir(&self, dir: &Path) -> Result<std::path::PathBuf> {
        Box::pin(self.find_binary_in_dir_impl(dir)).await
    }

    /// Implementation of find_binary_in_dir with proper boxing for recursion
    fn find_binary_in_dir_impl<'a>(
        &'a self,
        dir: &'a Path,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<std::path::PathBuf>> + 'a>> {
        Box::pin(async move {
            let mut entries = tokio::fs::read_dir(dir)
                .await
                .context("Failed to read extraction directory")?;

            while let Some(entry) = entries
                .next_entry()
                .await
                .context("Failed to read directory entry")?
            {
                let path = entry.path();

                if path.is_file() {
                    // Check if it's an executable
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let metadata = tokio::fs::metadata(&path).await?;
                        if metadata.permissions().mode() & 0o111 != 0 {
                            // Check if filename matches "vtcode"
                            if let Some(name) = path.file_name() {
                                if name.to_string_lossy().starts_with("vtcode") {
                                    return Ok(path);
                                }
                            }
                        }
                    }

                    #[cfg(windows)]
                    {
                        if let Some(name) = path.file_name() {
                            let name = name.to_string_lossy();
                            if name.starts_with("vtcode") && name.ends_with(".exe") {
                                return Ok(path);
                            }
                        }
                    }
                } else if path.is_dir() {
                    // Recursively search subdirectories
                    if let Ok(binary) = self.find_binary_in_dir_impl(&path).await {
                        return Ok(binary);
                    }
                }
            }

            anyhow::bail!("Binary not found in extracted archive")
        })
    }

    /// Set executable permissions (Unix only)
    #[cfg(unix)]
    fn set_executable_permissions(&self, path: &Path) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let metadata = std::fs::metadata(path)?;
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions)?;

        tracing::info!("Set executable permissions: {:?}", path);

        Ok(())
    }

    /// Replace the current executable with the new one
    async fn replace_executable(&self, new_binary: &Path, current_exe: &Path) -> Result<()> {
        // On Windows, we can't replace a running executable directly
        // We need to use a different approach
        #[cfg(windows)]
        {
            self.replace_executable_windows(new_binary, current_exe)
                .await
        }

        // On Unix, we can replace the executable directly
        #[cfg(unix)]
        {
            self.replace_executable_unix(new_binary, current_exe).await
        }
    }

    /// Replace executable on Unix systems
    #[cfg(unix)]
    async fn replace_executable_unix(&self, new_binary: &Path, current_exe: &Path) -> Result<()> {
        // Copy the new binary over the current one
        tokio::fs::copy(new_binary, current_exe)
            .await
            .context("Failed to copy new binary")?;

        tracing::info!("Replaced executable: {:?}", current_exe);

        Ok(())
    }

    /// Replace executable on Windows systems
    #[cfg(windows)]
    async fn replace_executable_windows(
        &self,
        new_binary: &Path,
        current_exe: &Path,
    ) -> Result<()> {
        // On Windows, we need to rename the current executable and then copy the new one
        let backup_path = current_exe.with_extension("exe.old");

        // Remove old backup if it exists
        if backup_path.exists() {
            tokio::fs::remove_file(&backup_path).await.ok();
        }

        // Rename current executable
        tokio::fs::rename(current_exe, &backup_path)
            .await
            .context("Failed to rename current executable")?;

        // Copy new binary
        match tokio::fs::copy(new_binary, current_exe).await {
            Ok(_) => {
                tracing::info!("Replaced executable: {:?}", current_exe);
                Ok(())
            }
            Err(e) => {
                // Restore backup on failure
                tokio::fs::rename(&backup_path, current_exe).await.ok();
                Err(e).context("Failed to copy new binary")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_archive() {
        let config = UpdateConfig::default();
        let installer = UpdateInstaller::new(config).unwrap();

        assert!(installer.is_archive(Path::new("file.tar.gz")));
        assert!(installer.is_archive(Path::new("file.zip")));
        assert!(installer.is_archive(Path::new("file.tgz")));
        assert!(!installer.is_archive(Path::new("file.bin")));
        assert!(!installer.is_archive(Path::new("file")));
    }

    #[test]
    fn test_installer_creation() {
        let config = UpdateConfig::default();
        let installer = UpdateInstaller::new(config);
        assert!(installer.is_ok());
    }
}
