//! Download manager for update files

use super::config::UpdateConfig;
use anyhow::{Context, Result};
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;

/// Handles downloading update files
pub struct UpdateDownloader {
    config: UpdateConfig,
    client: reqwest::Client,
}

impl UpdateDownloader {
    pub fn new(config: UpdateConfig) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.download_timeout_secs))
            .user_agent(format!("vtcode/{}", super::CURRENT_VERSION))
            .build()?;

        Ok(Self { config, client })
    }

    /// Download an update from the given URL
    pub async fn download(&self, url: &str) -> Result<PathBuf> {
        self.config.ensure_directories()?;

        // Extract filename from URL
        let filename = url.split('/').last().context("Invalid download URL")?;

        let download_path = self.config.update_dir.join(filename);

        // Download the file
        tracing::info!("Downloading update from: {}", url);

        let response = self
            .client
            .get(url)
            .send()
            .await
            .context("Failed to initiate download")?;

        if !response.status().is_success() {
            anyhow::bail!("Download failed with status: {}", response.status());
        }

        let total_size = response.content_length().unwrap_or(0);
        tracing::info!("Download size: {} bytes", total_size);

        // Stream the response to a file
        let mut file = tokio::fs::File::create(&download_path)
            .await
            .context("Failed to create download file")?;

        let mut downloaded: u64 = 0;
        let mut stream = response.bytes_stream();

        use futures::StreamExt;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("Failed to read download chunk")?;
            file.write_all(&chunk)
                .await
                .context("Failed to write download chunk")?;

            downloaded += chunk.len() as u64;

            if total_size > 0 {
                let progress = (downloaded as f64 / total_size as f64) * 100.0;
                tracing::debug!("Download progress: {:.1}%", progress);
            }
        }

        file.flush()
            .await
            .context("Failed to flush download file")?;

        tracing::info!("Download completed: {:?}", download_path);

        // Download checksum file if available
        if self.config.verify_checksums {
            let checksum_url = format!("{}.sha256", url);
            if let Ok(checksum_path) = self.download_checksum(&checksum_url).await {
                tracing::info!("Downloaded checksum file: {:?}", checksum_path);
            }
        }

        // Download signature file if available
        if self.config.verify_signatures {
            let signature_url = format!("{}.sig", url);
            if let Ok(signature_path) = self.download_signature(&signature_url).await {
                tracing::info!("Downloaded signature file: {:?}", signature_path);
            }
        }

        Ok(download_path)
    }

    /// Download checksum file
    async fn download_checksum(&self, url: &str) -> Result<PathBuf> {
        let filename = url.split('/').last().context("Invalid checksum URL")?;

        let checksum_path = self.config.update_dir.join(filename);

        let response = self
            .client
            .get(url)
            .send()
            .await
            .context("Failed to download checksum")?;

        if !response.status().is_success() {
            anyhow::bail!("Checksum download failed");
        }

        let content = response
            .bytes()
            .await
            .context("Failed to read checksum content")?;

        tokio::fs::write(&checksum_path, content)
            .await
            .context("Failed to write checksum file")?;

        Ok(checksum_path)
    }

    /// Download signature file
    async fn download_signature(&self, url: &str) -> Result<PathBuf> {
        let filename = url.split('/').last().context("Invalid signature URL")?;

        let signature_path = self.config.update_dir.join(filename);

        let response = self
            .client
            .get(url)
            .send()
            .await
            .context("Failed to download signature")?;

        if !response.status().is_success() {
            anyhow::bail!("Signature download failed");
        }

        let content = response
            .bytes()
            .await
            .context("Failed to read signature content")?;

        tokio::fs::write(&signature_path, content)
            .await
            .context("Failed to write signature file")?;

        Ok(signature_path)
    }

    /// Clean up downloaded files
    pub fn cleanup(&self, path: &PathBuf) -> Result<()> {
        if path.exists() {
            std::fs::remove_file(path).context("Failed to remove download file")?;
        }

        // Also remove checksum and signature files
        let checksum_path = path.with_extension("sha256");
        if checksum_path.exists() {
            std::fs::remove_file(checksum_path).ok();
        }

        let signature_path = path.with_extension("sig");
        if signature_path.exists() {
            std::fs::remove_file(signature_path).ok();
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_downloader_creation() {
        let config = UpdateConfig::default();
        let downloader = UpdateDownloader::new(config);
        assert!(downloader.is_ok());
    }
}
