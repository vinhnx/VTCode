use anyhow::Result;
use vtcode_commons::paths::validate_path_safety as common_validate_path_safety;

/// Validates that a path is safe to use.
/// Re-exported from vtcode-commons for tool-specific use cases.
pub fn validate_path_safety(path: &str) -> Result<()> {
    common_validate_path_safety(path)
}

#[cfg(test)]
mod tests {
    use super::validate_path_safety;

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
}
