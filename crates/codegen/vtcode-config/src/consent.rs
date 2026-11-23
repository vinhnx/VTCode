//! Shared ask-first consent model for home-directory installs.
//!
//! Single source of truth used by the CLI first-run wizard and the Desktop
//! consent card so both surfaces ask the same question, persist the same
//! answer (`skills.bundled.enabled` in the user layer), and enforce the same
//! gate. Install sites must consult [`bundled_skills_install_permitted`]:
//! nothing under the user's home is created for bundled skills until the
//! answer is [`ConsentAnswer::Granted`] — or the cache already exists from a
//! previous release (grandfathered), in which case only a refresh of VT Code's
//! own cache directory is allowed.
//!
//! [`ConsentAnswer`] deliberately has three states: an explicit grant,
//! an explicit decline (never ask again, actively remove our cache), and
//! unasked (fail closed: no writes, surfaces may prompt once).

use crate::loader::ConfigManager;

/// Dotted setting path that carries the bundled-skills consent answer.
///
/// Persisted to the user layer by both the CLI wizard and the Desktop card.
/// Workspace and project layers may still override the effective value for
/// their own scope through the normal layer merge.
pub const BUNDLED_SKILLS_SETTING_PATH: &str = "skills.bundled.enabled";

/// Consent answer for installing bundled skills into the user's home.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentAnswer {
    /// The user explicitly allowed the install.
    Granted,
    /// The user explicitly declined. Never install, never prompt again for
    /// the same scope; our own cache directory is removed when present.
    Declined,
    /// No answer recorded in any enabled layer. Installs are forbidden
    /// unless grandfathered (a cache from a previous release exists).
    Unasked,
}

/// Resolve the bundled-skills consent answer from a loaded manager.
///
/// Presence is checked against every enabled layer (system, user, project,
/// workspace); the effective value decides between grant and decline. A
/// missing key is [`ConsentAnswer::Unasked`] even though the effective
/// default is `false` — absence means "never asked", not "declined".
#[must_use]
pub fn bundled_skills_answer(manager: &ConfigManager) -> ConsentAnswer {
    if !manager.has_explicit_dotted_key(BUNDLED_SKILLS_SETTING_PATH) {
        return ConsentAnswer::Unasked;
    }
    if manager.config().skills.bundled.enabled {
        ConsentAnswer::Granted
    } else {
        ConsentAnswer::Declined
    }
}

/// Decide whether bundled skills may be installed or refreshed.
///
/// `grandfathered` must be true only when a marker written by a previous
/// VT Code release proves the cache predates ask-first consent. The
/// predicate is intentionally pure so unit tests pin the same truth table
/// the install sites enforce.
#[must_use]
pub const fn bundled_skills_install_permitted(answer: ConsentAnswer, grandfathered: bool) -> bool {
    match answer {
        ConsentAnswer::Granted => true,
        ConsentAnswer::Declined => false,
        ConsentAnswer::Unasked => grandfathered,
    }
}

/// Title shared by the CLI wizard step and the Desktop consent card.
pub const CONSENT_TITLE: &str = "Bundled skills";

/// Question shared by the CLI wizard step and the Desktop consent card.
pub const CONSENT_QUESTION: &str = "Install VT Code's bundled skills (cmd-review, skill-creator, skill-installer)?";

/// What the install contains. Sizes stay small and no network is involved.
pub const CONSENT_BODY_WHAT: &str = "Three built-in skills are extracted from the VT Code binary: cmd-review (powers /review), skill-creator, and skill-installer.";

pub const CONSENT_BODY_REVERSIBLE: &str = "Fully reversible: declining (or disabling later) removes the cache and VT Code keeps working; skill features report how to enable them.";

pub const CONSENT_ALLOW_LABEL: &str = "Allow (recommended)";

pub const CONSENT_ALLOW_DESCRIPTION: &str = "Install the three skills to the VT Code data directory.";

pub const CONSENT_DECLINE_LABEL: &str = "Decline";

pub const CONSENT_DECLINE_DESCRIPTION: &str = "Skip the install. VT Code works without these skills.";

pub const CONSENT_MCP_DISCLOSURE: &str =
    "MCP servers are never installed automatically. Each one is configured explicitly by you.";

pub const CONSENT_REOPEN_HINT: &str =
    "You can change this anytime in /config (skills.bundled.enabled) or your user vtcode.toml.";

/// Describe where the install lands, for wizard and card copy.
#[must_use]
pub fn install_location_description(data_dir: &std::path::Path) -> String {
    format!(
        "Installs to {} (offline, from the running binary).",
        data_dir.join("skills").join(".system").display()
    )
}

/// Actionable hint appended when a bundled skill is requested but unavailable.
#[must_use]
pub fn bundled_skill_missing_hint(name: &str) -> String {
    format!(
        "Skill '{name}' is a bundled skill that is not installed (declined or never consented). Allow it with /config (skills.bundled.enabled) or your user vtcode.toml, then start a new session."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::defaults::{self, WorkspacePathsDefaults};
    use serial_test::serial;
    use std::fs;
    use std::sync::Arc;
    use vtcode_commons::reference::StaticWorkspacePaths;

    fn with_hermetic_manager<R>(
        user_toml: Option<&str>,
        workspace_toml: Option<&str>,
        action: impl FnOnce(&ConfigManager) -> R,
    ) -> R {
        let workspace = assert_fs::TempDir::new().expect("workspace tempdir");
        let home_paths = match user_toml {
            Some(content) => {
                let home_dir = workspace.path().join("home");
                fs::create_dir_all(&home_dir).expect("home dir");
                let user_config = home_dir.join("vtcode.toml");
                fs::write(&user_config, content).expect("user config");
                vec![user_config]
            }
            None => Vec::new(),
        };
        if let Some(content) = workspace_toml {
            fs::write(workspace.path().join("vtcode.toml"), content).expect("workspace config");
        }
        let paths = StaticWorkspacePaths::new(workspace.path(), workspace.path().join(".vtcode"));
        let provider = WorkspacePathsDefaults::new(Arc::new(paths)).with_home_paths(home_paths);
        defaults::provider::with_config_defaults_provider_for_test(Arc::new(provider), || {
            let manager = ConfigManager::load_from_workspace(workspace.path()).expect("load hermetic config");
            action(&manager)
        })
    }

    fn answer_for_configs(user_toml: Option<&str>, workspace_toml: Option<&str>) -> ConsentAnswer {
        with_hermetic_manager(user_toml, workspace_toml, bundled_skills_answer)
    }

    #[test]
    #[serial]
    fn answer_distinguishes_grant_decline_and_unasked() {
        assert_eq!(answer_for_configs(None, None), ConsentAnswer::Unasked);
        assert_eq!(answer_for_configs(Some("[skills.bundled]\nenabled = true\n"), None), ConsentAnswer::Granted);
        assert_eq!(answer_for_configs(Some("[skills.bundled]\nenabled = false\n"), None), ConsentAnswer::Declined);
        // Workspace layers answer too: an explicit workspace grant counts even
        // when the user layer is silent.
        assert_eq!(answer_for_configs(None, Some("[skills.bundled]\nenabled = true\n")), ConsentAnswer::Granted);
        // A neighboring skills key does not answer the consent question.
        assert_eq!(answer_for_configs(Some("[skills]\nenable-auto-trigger = false\n"), None), ConsentAnswer::Unasked);
    }

    #[test]
    #[serial]
    fn dotted_presence_handles_prefixes_and_malformed_paths() {
        with_hermetic_manager(Some("[skills.bundled]\nenabled = true\n"), None, |manager| {
            assert!(manager.has_explicit_dotted_key(BUNDLED_SKILLS_SETTING_PATH));
            assert!(manager.has_explicit_dotted_key("skills.bundled"));
            assert!(manager.has_explicit_dotted_key("skills"));
            assert!(!manager.has_explicit_dotted_key("skills.bundled.missing"));
            assert!(!manager.has_explicit_dotted_key("skills.bundled.enabled.extra"));
            for malformed in ["", ".", "skills..enabled", ".skills", "skills."] {
                assert!(!manager.has_explicit_dotted_key(malformed), "malformed path must not match: {malformed:?}");
            }
        });
    }

    #[test]
    fn install_permitted_truth_table() {
        use ConsentAnswer::{Declined, Granted, Unasked};
        assert!(bundled_skills_install_permitted(Granted, false));
        assert!(bundled_skills_install_permitted(Granted, true));
        assert!(!bundled_skills_install_permitted(Declined, false));
        assert!(!bundled_skills_install_permitted(Declined, true));
        assert!(!bundled_skills_install_permitted(Unasked, false));
        assert!(bundled_skills_install_permitted(Unasked, true));
    }

    #[test]
    fn missing_hint_names_skill_and_recovery() {
        let hint = bundled_skill_missing_hint("cmd-review");
        assert!(hint.contains("cmd-review"), "hint: {hint}");
        assert!(hint.contains("skills.bundled.enabled"), "hint: {hint}");
        assert!(hint.contains("new session"), "hint: {hint}");
    }

    #[test]
    fn install_location_points_at_system_cache() {
        let description = install_location_description(std::path::Path::new("/data"));
        assert!(description.contains("skills"), "description: {description}");
        assert!(description.contains(".system"), "description: {description}");
    }
}
