mod archive;
mod release;
mod state;

use std::path::PathBuf;

use crate::tools::ast_grep_binary::{
    AST_GREP_NO_INSTALL_ENV, managed_ast_grep_bin_dir, missing_ast_grep_message,
    resolve_ast_grep_binary_from_env_and_fs,
};
use crate::tools::editing::patch::resolve_ast_grep_binary_path;
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
            Err(err) => Self::Error { reason: err.to_string() },
        }
    }

    pub async fn install() -> Result<AstGrepInstallOutcome> {
        let paths = InstallPaths::discover()?;
        let _lock = InstallLockGuard::acquire(&paths)?;

        if !InstallationCache::is_stale(&paths)
            && let Ok(cache) = InstallationCache::load(&paths)
            && cache.status == "failed"
            && !cache.failure_reason.as_deref().is_some_and(should_retry_without_cooldown)
        {
            let reason = cache.failure_reason.as_deref().unwrap_or("unknown reason");
            bail!(
                "Previous ast-grep installation attempt failed ({reason}). Not retrying for 24 hours."
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

    /// Resolve the ast-grep binary path, auto-installing on first use when the
    /// binary is missing. Used by `structural` and `outline` search actions so
    /// the model does not need to run `vtcode dependencies install ast-grep`
    /// manually before the first structural/outline call.
    ///
    /// Skips auto-install (returns the standard "not available" error) when:
    /// - the test override forces `Missing` (unit tests),
    /// - `VTCODE_AST_GREP_NO_INSTALL` is set (user opt-out).
    ///
    /// On install failure, returns `"ast-grep is not available; auto-install
    /// failed: {error}"` so the structural grep-fallback substring check
    /// (`msg.contains("ast-grep") && msg.contains("not available")`) still
    /// triggers for query/count workflows.
    pub async fn resolve_or_install() -> std::result::Result<PathBuf, String> {
        // Use an already-resolved binary regardless of the opt-out env var:
        // `VTCODE_AST_GREP_NO_INSTALL` only disables auto-install, not usage of
        // an already-present binary.
        if let Ok(path) = resolve_ast_grep_binary_path() {
            return Ok(path);
        }

        // No binary available. If the user opted out of auto-install, surface
        // the missing-binary error immediately without attempting a download.
        if std::env::var_os(AST_GREP_NO_INSTALL_ENV).is_some() {
            return Err(missing_ast_grep_message(
                "Auto-install disabled via VTCODE_AST_GREP_NO_INSTALL.",
            ));
        }

        // Skip auto-install in test mode when either override registry forces
        // the binary to be missing. Both are checked because
        // `resolve_ast_grep_binary_path` consults the patch (semantic) registry
        // first and the `ast_grep_binary` registry second.
        if crate::tools::ast_grep_binary::is_binary_override_missing()
            || crate::tools::editing::patch::is_binary_override_missing()
        {
            return Err(missing_ast_grep_message(
                "Auto-install skipped: test override forces missing binary.",
            ));
        }

        match Self::install().await {
            Ok(_) => match resolve_ast_grep_binary_from_env_and_fs() {
                Some(path) => Ok(path),
                None => Err(missing_ast_grep_message(
                    "Auto-install reported success but the binary could not be resolved.",
                )),
            },
            Err(err) => Err(format!(
                "ast-grep is not available; auto-install failed: {err}. Run `{}` manually.",
                crate::tools::ast_grep_binary::AST_GREP_INSTALL_COMMAND
            )),
        }
    }
}

fn should_retry_without_cooldown(failure_reason: &str) -> bool {
    failure_reason.contains("No ast-grep release asset matched the current platform")
        || failure_reason.contains("Unsupported platform for VT Code-managed ast-grep install")
}

#[cfg(test)]
mod tests {
    use super::{AstGrepStatus, should_retry_without_cooldown};
    use crate::tools::ast_grep_binary::AST_GREP_INSTALL_COMMAND;
    use crate::tools::ast_grep_binary::set_ast_grep_binary_override_for_tests;
    use serial_test::serial;
    use vtcode_commons::env_lock;

    #[test]
    fn platform_mismatch_failures_do_not_enter_cooldown() {
        assert!(should_retry_without_cooldown(
            "No ast-grep release asset matched the current platform (aarch64-apple-darwin)"
        ));
        assert!(should_retry_without_cooldown(
            "Unsupported platform for VT Code-managed ast-grep install"
        ));
    }

    #[test]
    fn unrelated_failures_still_use_cooldown() {
        assert!(!should_retry_without_cooldown("Failed to fetch ast-grep release metadata"));
    }

    #[tokio::test]
    #[serial]
    async fn resolve_or_install_skips_install_when_test_override_missing() {
        let _guard = set_ast_grep_binary_override_for_tests(None);
        let err = AstGrepStatus::resolve_or_install()
            .await
            .expect_err("should not install in test mode");
        let text = err.to_string();
        assert!(text.contains("ast-grep"), "{text}");
        assert!(text.contains("not available"), "{text}");
        assert!(text.contains(AST_GREP_INSTALL_COMMAND), "{text}");
    }

    #[tokio::test]
    #[serial]
    async fn resolve_or_install_mentions_env_optout_when_set() {
        // Force the binary to be missing so `resolve_ast_grep_binary_path`
        // fails and execution reaches the env-var check. The test override is
        // checked AFTER the env var, so the env-var error wins. This keeps the
        // test portable across machines that happen to have ast-grep installed.
        let _override = set_ast_grep_binary_override_for_tests(None);
        let env = env_lock::lock();
        env.set_var("VTCODE_AST_GREP_NO_INSTALL", "1");
        let err = AstGrepStatus::resolve_or_install()
            .await
            .expect_err("should not install when env opt-out is set");
        env.remove_var("VTCODE_AST_GREP_NO_INSTALL");
        assert!(
            err.contains("VTCODE_AST_GREP_NO_INSTALL"),
            "error should mention the env var: {err}"
        );
    }
}
