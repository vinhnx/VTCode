use anyhow::{Result, anyhow};
use vtcode_commons::paths::validate_path_safety as common_validate_path_safety;

const ROOT_DIRECTORY_LISTING_BLOCKED_MESSAGE: &str = "Error: root directory listing is blocked to prevent infinite loops. Please specify a subdirectory like 'src/', 'vtcode-core/src/', or 'tests/' instead of listing the whole workspace.";

/// Validates that a path is safe to use.
/// Re-exported from vtcode-commons for tool-specific use cases.
pub fn validate_path_safety(path: &str) -> Result<()> {
    common_validate_path_safety(path)
}

/// Validates that a directory-listing request targets a concrete subdirectory.
pub fn validate_non_root_listing_path(path: Option<&str>) -> Result<()> {
    let normalized = path
        .unwrap_or_default()
        .trim_start_matches("./")
        .trim_start_matches('/');
    if normalized.is_empty() || normalized == "." {
        return Err(anyhow!(ROOT_DIRECTORY_LISTING_BLOCKED_MESSAGE));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{validate_non_root_listing_path, validate_path_safety};

    #[test]
    fn allows_macos_temp_paths_under_var_folders() {
        assert!(validate_path_safety("/var/folders/ab/cd/tmp123/file.txt").is_ok());
    }

    #[test]
    fn still_blocks_sensitive_var_paths() {
        assert!(validate_path_safety("/var/db/shadow").is_err());
        assert!(validate_path_safety("/var").is_err());
    }

    #[test]
    fn allows_non_critical_prefix_matches() {
        assert!(validate_path_safety("/varnish/cache/file").is_ok());
    }

    #[test]
    fn allows_var_tmp_paths() {
        assert!(validate_path_safety("/var/tmp/vtcode/run.log").is_ok());
    }

    #[test]
    fn blocks_root_directory_listing_requests() {
        for candidate in [None, Some("."), Some(""), Some("./"), Some("/")] {
            assert!(validate_non_root_listing_path(candidate).is_err());
        }
    }

    #[test]
    fn allows_subdirectory_listing_requests() {
        for candidate in [Some("src"), Some("./src"), Some("/workspace/src")] {
            assert!(validate_non_root_listing_path(candidate).is_ok());
        }
    }
}
