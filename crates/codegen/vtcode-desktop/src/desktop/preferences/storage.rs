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
/// legacy file is never renamed or overwritten here.
pub(super) fn load<T>(canonical: &Path, legacy: Option<&Path>) -> T
where
    T: DeserializeOwned + Default,
{
    if let Ok(content) = std::fs::read_to_string(canonical) {
        return parse_or_quarantine(canonical, content);
    }
    legacy
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
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
