mod archive;
mod release;
mod state;

use std::path::PathBuf;

use crate::tools::ast_grep_binary::{
    managed_ast_grep_bin_dir, resolve_ast_grep_binary_from_env_and_fs,
};
use anyhow::{Context, Result, bail};

use self::archive::{ast_grep_version, install_archive};
use self::release::{
    current_platform_asset_spec, download_release_asset, fetch_latest_release,
    select_release_asset, verify_checksum_if_available,
};
use self::state::{InstallLockGuard, InstallPaths, InstallationCache};

#[derive(Debug, Clone, PartialEq)]
pub enum AstGrepStatus {
    Available {
        version: String,
        binary: PathBuf,
        managed: bool,
    },
    NotFound,
    Error {
        reason: String,
    },
}

#[derive(Debug, Clone)]
pub struct AstGrepInstallOutcome {
    pub version: String,
    pub binary_path: PathBuf,
    pub alias_path: Option<PathBuf>,
    pub managed_bin_dir: PathBuf,
    pub warning: Option<String>,
}

impl AstGrepStatus {
    pub fn check() -> Self {
        let Some(binary) = resolve_ast_grep_binary_from_env_and_fs() else {
            return Self::NotFound;
        };

        match ast_grep_version(&binary) {
            Ok(version) => Self::Available {
                version,
                managed: binary.starts_with(managed_ast_grep_bin_dir()),
                binary,
            },
            Err(err) => Self::Error {
                reason: err.to_string(),
            },
        }
    }

    pub async fn install() -> Result<AstGrepInstallOutcome> {
        let paths = InstallPaths::discover()?;
        let _lock = InstallLockGuard::acquire(&paths)?;

        if !InstallationCache::is_stale(&paths)
            && let Ok(cache) = InstallationCache::load(&paths)
            && cache.status == "failed"
        {
            let reason = cache.failure_reason.as_deref().unwrap_or("unknown reason");
            bail!(
                "Previous ast-grep installation attempt failed ({}). Not retrying for 24 hours.",
                reason
            );
        }

        let install_result: Result<AstGrepInstallOutcome> = async {
            let client = reqwest::Client::builder()
                .user_agent("vtcode-ast-grep-installer")
                .build()
                .context("Failed to create HTTP client")?;

            let release = fetch_latest_release(&client).await?;
            let platform = current_platform_asset_spec()?;
            let selected_asset = select_release_asset(&release, &platform)?;
            let archive_bytes = download_release_asset(&client, &selected_asset.asset).await?;
            let warning =
                verify_checksum_if_available(&client, &release, &selected_asset, &archive_bytes)
                    .await?;

            install_archive(&paths, &selected_asset.asset.name, &archive_bytes)?;
            let version = ast_grep_version(&paths.binary_path)
                .context("Installed ast-grep failed version check")?;
            InstallationCache::mark_success(&paths, &selected_asset.tag_name);

            Ok(AstGrepInstallOutcome {
                version,
                binary_path: paths.binary_path.clone(),
                alias_path: paths.alias_path.clone(),
                managed_bin_dir: paths.bin_dir.clone(),
                warning,
            })
        }
        .await;

        match install_result {
            Ok(outcome) => Ok(outcome),
            Err(err) => {
                InstallationCache::mark_failure(&paths, &err.to_string());
                Err(err)
            }
        }
    }
}
