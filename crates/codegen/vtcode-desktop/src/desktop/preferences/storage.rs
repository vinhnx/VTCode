//! Path resolution and durable load helpers for desktop preferences.
//!
//! Kept separate from the preference model so the canonical/legacy contract is
//! reviewable on its own and the store file stays within its line budget.

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;

/// Canonical preference file under the resolved config home.
pub(super) fn canonical_preferences_path() -> Option<PathBuf> {
    vtcode_commons::VtCodePaths::resolve()
        .ok()
        .map(|paths| paths.config_dir().join("desktop").join("preferences.json"))
}

/// Pre-XDG preference file, read only when the canonical file is absent.
pub(super) fn legacy_preferences_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".vtcode").join("desktop").join("preferences.json"))
}

/// Read `canonical`, falling back to `legacy` only when the canonical file is
/// missing. A corrupt canonical file is quarantined and yields defaults; the
/// legacy file is read-only here (migration owns it) but a bad legacy file is
/// reported instead of silently degrading to defaults.
pub(super) fn load<T>(canonical: &Path, legacy: Option<&Path>) -> T
where
    T: DeserializeOwned + Default,
{
    match std::fs::read_to_string(canonical) {
        Ok(content) => parse_or_quarantine(canonical, content),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => load_legacy(legacy),
        Err(error) => {
            vtcode_commons::diagnostics::stderr(format_args!(
                "could not read desktop preferences ({}): {error}",
                canonical.display()
            ));
            T::default()
        }
    }
}

pub(super) fn load_legacy<T: DeserializeOwned + Default>(legacy: Option<&Path>) -> T {
    let Some(path) = legacy else {
        return T::default();
    };
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return T::default(),
        Err(error) => {
            vtcode_commons::diagnostics::stderr(format_args!(
                "could not read legacy desktop preferences ({}): {error}",
                path.display()
            ));
            return T::default();
        }
    };
    match serde_json::from_str(&content) {
        Ok(preferences) => preferences,
        Err(error) => {
            vtcode_commons::diagnostics::stderr(format_args!(
                "legacy desktop preferences corrupt ({error}): {}; using defaults",
                path.display()
            ));
            T::default()
        }
    }
}

fn parse_or_quarantine<T: DeserializeOwned + Default>(path: &Path, content: String) -> T {
    match serde_json::from_str(&content) {
        Ok(preferences) => preferences,
        Err(error) => {
            let quarantined = path.with_extension("json.corrupt");
            if let Err(rename_error) = std::fs::rename(path, &quarantined) {
                vtcode_commons::diagnostics::stderr(format_args!(
                    "failed to quarantine corrupt desktop preferences ({}): {rename_error}",
                    quarantined.display()
                ));
            } else {
                vtcode_commons::diagnostics::stderr(format_args!(
                    "desktop preferences corrupt ({error}); moved to {}",
                    quarantined.display()
                ));
            }
            T::default()
        }
    }
}
