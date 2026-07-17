use anyhow::{Result, anyhow};
use vtcode_commons::paths::validate_path_safety as common_validate_path_safety;

/// Validates that a path is safe to use.
/// Re-exported from vtcode-commons for tool-specific use cases.
pub fn validate_path_safety(path: &str) -> Result<()> {
    common_validate_path_safety(path)
}

/// Validates that a directory-listing request targets a safe path.
/// Blocks empty paths, absent paths, and absolute filesystem root `/`,
/// but allows `.` (workspace root) — loop detection and `max_items`
/// caps handle the "infinite loops" concern that originally motivated
/// blocking root-listing.
pub fn validate_non_root_listing_path(path: Option<&str>) -> Result<()> {
    let raw = path.unwrap_or_default();
    let normalized = raw.trim_start_matches("./").trim_start_matches('/');
    if normalized.is_empty() && raw != "." && raw != "./" {
        return Err(anyhow!(
            "Error: directory-listing path is empty. Please specify a subdirectory like 'src/', 'vtcode-core/src/', or 'tests/'."
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{validate_non_root_listing_path, validate_path_safety};

    #[test]
    fn allows_macos_temp_paths_under_var_folders() {
        validate_path_safety("/var/folders/ab/cd/tmp123/file.txt").unwrap();
    }

    #[test]
    fn still_blocks_sensitive_var_paths() {
        assert!(validate_path_safety("/var/db/shadow").is_err());
        assert!(validate_path_safety("/var").is_err());
    }

    #[test]
    fn allows_non_critical_prefix_matches() {
        validate_path_safety("/varnish/cache/file").unwrap();
    }

    #[test]
    fn allows_var_tmp_paths() {
        validate_path_safety("/var/tmp/vtcode/run.log").unwrap();
    }

    #[test]
    fn blocks_empty_or_missing_paths() {
        for candidate in [None, Some(""), Some("/")] {
            assert!(
                validate_non_root_listing_path(candidate).is_err(),
                "should block {candidate:?}"
            );
        }
    }

    #[test]
    fn allows_root_listing_path() {
        for candidate in [Some("."), Some("./")] {
            validate_non_root_listing_path(candidate)
                .unwrap_or_else(|_| panic!("should allow {candidate:?}"));
        }
    }

    #[test]
    fn allows_subdirectory_listing_requests() {
        for candidate in [Some("src"), Some("./src"), Some("/workspace/src")] {
            validate_non_root_listing_path(candidate).unwrap();
        }
    }
}
