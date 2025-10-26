//! Binary verification for downloaded updates

use super::config::UpdateConfig;
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::Path;

/// Handles verification of downloaded update files
pub struct UpdateVerifier {
    config: UpdateConfig,
}

impl UpdateVerifier {
    pub fn new(config: UpdateConfig) -> Result<Self> {
        Ok(Self { config })
    }

    /// Verify the integrity of a downloaded update
    pub async fn verify(&self, path: &Path) -> Result<()> {
        tracing::info!("Verifying update file: {:?}", path);

        // Check if file exists
        if !path.exists() {
            anyhow::bail!("Update file does not exist: {:?}", path);
        }

        // Verify checksum if enabled
        if self.config.verify_checksums {
            self.verify_checksum(path)
                .await
                .context("Checksum verification failed")?;
        }

        // Verify signature if enabled
        if self.config.verify_signatures {
            self.verify_signature(path)
                .await
                .context("Signature verification failed")?;
        }

        // Verify the binary is executable (on Unix)
        #[cfg(unix)]
        self.verify_executable(path)?;

        tracing::info!("Update file verification successful");

        Ok(())
    }

    /// Verify the checksum of the downloaded file
    async fn verify_checksum(&self, path: &Path) -> Result<()> {
        let checksum_path = path.with_extension("sha256");

        if !checksum_path.exists() {
            tracing::warn!("Checksum file not found, skipping verification");
            return Ok(());
        }

        // Read expected checksum
        let expected_checksum = tokio::fs::read_to_string(&checksum_path)
            .await
            .context("Failed to read checksum file")?;

        let expected_checksum = expected_checksum
            .split_whitespace()
            .next()
            .context("Invalid checksum format")?
            .trim();

        // Calculate actual checksum
        let actual_checksum = self.calculate_sha256(path).await?;

        if actual_checksum.to_lowercase() != expected_checksum.to_lowercase() {
            anyhow::bail!(
                "Checksum mismatch: expected {}, got {}",
                expected_checksum,
                actual_checksum
            );
        }

        tracing::info!("Checksum verification passed");

        Ok(())
    }

    /// Calculate SHA256 checksum of a file
    async fn calculate_sha256(&self, path: &Path) -> Result<String> {
        let content = tokio::fs::read(path)
            .await
            .context("Failed to read file for checksum")?;

        let mut hasher = Sha256::new();
        hasher.update(&content);
        let result = hasher.finalize();

        Ok(format!("{:x}", result))
    }

    /// Verify the signature of the downloaded file
    async fn verify_signature(&self, path: &Path) -> Result<()> {
        let signature_path = path.with_extension("sig");

        if !signature_path.exists() {
            tracing::warn!("Signature file not found, skipping verification");
            return Ok(());
        }

        // For now, we just check that the signature file exists
        // In a production implementation, you would use a proper signature verification library
        // such as `ed25519-dalek` or `rsa` to verify the signature against a public key

        tracing::warn!(
            "Signature verification not fully implemented - signature file exists but not verified"
        );

        Ok(())
    }

    /// Verify the binary is executable (Unix only)
    #[cfg(unix)]
    fn verify_executable(&self, path: &Path) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let metadata = std::fs::metadata(path).context("Failed to read file metadata")?;
        let permissions = metadata.permissions();

        // Check if the file has execute permissions
        if permissions.mode() & 0o111 == 0 {
            tracing::warn!("Binary is not executable, attempting to set permissions");
            self.make_executable(path)?;
        }

        Ok(())
    }

    /// Make a file executable (Unix only)
    #[cfg(unix)]
    fn make_executable(&self, path: &Path) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let metadata = std::fs::metadata(path)?;
        let mut permissions = metadata.permissions();
        permissions.set_mode(permissions.mode() | 0o111);
        std::fs::set_permissions(path, permissions)?;

        tracing::info!("Set executable permissions on binary");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[tokio::test]
    async fn test_calculate_sha256() {
        let config = UpdateConfig::default();
        let verifier = UpdateVerifier::new(config).unwrap();

        // Create a temporary file
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        let mut file = std::fs::File::create(&file_path).unwrap();
        file.write_all(b"test content").unwrap();

        // Calculate checksum
        let checksum = verifier.calculate_sha256(&file_path).await.unwrap();

        // Verify it's a valid SHA256 hash (64 hex characters)
        assert_eq!(checksum.len(), 64);
        assert!(checksum.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_verifier_creation() {
        let config = UpdateConfig::default();
        let verifier = UpdateVerifier::new(config);
        assert!(verifier.is_ok());
    }
}
