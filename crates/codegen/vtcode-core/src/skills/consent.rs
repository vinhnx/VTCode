//! Workspace-level composition of ask-first consent for bundled skills.
//!
//! Combines the shared [`vtcode_config::consent`] answer with the pre-consent
//! cache probe. Every install site funnels through
//! [`bundled_skills_install_permitted_for_workspace`]; the function is
//! fail-closed, so any configuration-load or path-resolution error means no
//! install and no system discovery root.

use std::path::Path;

use crate::config::ConfigManager;
use crate::config::consent::{bundled_skills_answer, bundled_skills_install_permitted};
use crate::skills::system::bundled_skills_grandfathered;

/// Whether bundled system skills may be installed or refreshed for `workspace`.
///
/// True only for an explicit grant, or while unasked when a pre-consent
/// cache grandfathers the refresh. Never true for an explicit decline —
/// even when a stale cache is still on disk.
#[must_use]
pub fn bundled_skills_install_permitted_for_workspace(workspace: &Path) -> bool {
    let Ok(manager) = ConfigManager::load_from_workspace(workspace) else {
        return false;
    };
    bundled_skills_install_permitted(bundled_skills_answer(&manager), bundled_skills_grandfathered())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::system::{install_system_skills, system_cache_root_dir};
    use serial_test::serial;
    use std::fs;
    use std::sync::Arc;
    use tempfile::TempDir;

    struct ConsentHarness {
        _root: TempDir,
        workspace: std::path::PathBuf,
        data: std::path::PathBuf,
        codex: std::path::PathBuf,
        previous_provider: Arc<dyn crate::config::ConfigDefaultsProvider>,
        previous_data: Option<std::ffi::OsString>,
        previous_codex: Option<std::ffi::OsString>,
    }

    impl ConsentHarness {
        fn new(user_toml: Option<&str>) -> Self {
            let root = TempDir::new().expect("hermetic root");
            let data = root.path().join("data");
            let codex = root.path().join("codex");
            let workspace = root.path().join("workspace");
            for dir in [&data, &codex, &workspace] {
                fs::create_dir_all(dir).expect("hermetic dir");
            }
            let home_paths = match user_toml {
                Some(content) => {
                    let home_dir = root.path().join("home");
                    fs::create_dir_all(&home_dir).expect("home dir");
                    let user_config = home_dir.join("vtcode.toml");
                    fs::write(&user_config, content).expect("user config");
                    vec![user_config]
                }
                None => Vec::new(),
            };
            let static_paths =
                vtcode_commons::reference::StaticWorkspacePaths::new(&workspace, workspace.join(".vtcode"));
            let provider = crate::config::defaults::WorkspacePathsDefaults::new(Arc::new(static_paths))
                .with_home_paths(home_paths);
            let previous_provider = crate::config::install_config_defaults_provider(Arc::new(provider));
            let previous_data = std::env::var_os("VTCODE_DATA");
            let previous_codex = std::env::var_os("CODEX_HOME");
            let guard = vtcode_commons::env_lock::lock();
            guard.set_var("VTCODE_DATA", &data);
            guard.set_var("CODEX_HOME", &codex);
            Self {
                _root: root,
                workspace,
                data,
                codex,
                previous_provider,
                previous_data,
                previous_codex,
            }
        }

        fn permitted(&self) -> bool {
            bundled_skills_install_permitted_for_workspace(&self.workspace)
        }

        fn install_unified_cache(&self) {
            install_system_skills(&self.data).expect("unified cache install");
        }

        fn install_legacy_cache(&self) {
            install_system_skills(&self.codex).expect("legacy cache install");
        }
    }

    impl Drop for ConsentHarness {
        fn drop(&mut self) {
            // Restore globals even on assertion failure: the provider and env
            // outlive the temp dirs, and a leaked override would poison every
            // later `load_from_workspace` in this test binary. Neither call
            // panics (poisoned locks recover), so teardown is abort-safe.
            let _ = crate::config::install_config_defaults_provider(self.previous_provider.clone());
            let guard = vtcode_commons::env_lock::lock();
            guard.restore_var("VTCODE_DATA", self.previous_data.as_deref());
            guard.restore_var("CODEX_HOME", self.previous_codex.as_deref());
        }
    }

    #[test]
    #[serial]
    fn grant_permits_without_cache() {
        let harness = ConsentHarness::new(Some("[skills.bundled]\nenabled = true\n"));
        assert!(!system_cache_root_dir(&harness.data).exists());
        assert!(harness.permitted());
    }

    #[test]
    #[serial]
    fn decline_forbids_despite_stale_cache() {
        let harness = ConsentHarness::new(Some("[skills.bundled]\nenabled = false\n"));
        harness.install_unified_cache();
        assert!(!harness.permitted());
    }

    #[test]
    #[serial]
    fn unasked_without_cache_forbids() {
        let harness = ConsentHarness::new(None);
        assert!(!harness.permitted());
    }

    #[test]
    #[serial]
    fn unasked_with_unified_cache_permits() {
        let harness = ConsentHarness::new(None);
        harness.install_unified_cache();
        assert!(harness.permitted());
    }

    #[test]
    #[serial]
    fn unasked_with_legacy_only_cache_permits() {
        let harness = ConsentHarness::new(None);
        assert!(!system_cache_root_dir(&harness.data).exists());
        harness.install_legacy_cache();
        assert!(harness.permitted());
    }
}
