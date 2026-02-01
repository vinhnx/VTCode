//! Validation utilities for common operations

use anyhow::{Result, bail};
use std::path::Path;

/// Validate that a string is non-empty
pub fn validate_non_empty(value: &str, field_name: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{} cannot be empty", field_name);
    }
    Ok(())
}

/// Validate and return non-empty string
pub fn validate_non_empty_string(value: String, field_name: &str) -> Result<String> {
    if value.trim().is_empty() {
        bail!("{} cannot be empty", field_name);
    }
    Ok(value)
}

/// Validate optional non-empty string
pub fn validate_optional_non_empty(value: &Option<String>, field_name: &str) -> Result<()> {
    if let Some(v) = value {
        validate_non_empty(v, field_name)?;
    }
    Ok(())
}

/// Validate collection is not empty
pub fn validate_non_empty_collection<T>(collection: &[T], field_name: &str) -> Result<()> {
    if collection.is_empty() {
        bail!("{} collection cannot be empty", field_name);
    }
    Ok(())
}

/// Validate that all strings in a slice are non-empty
pub fn validate_all_non_empty(values: &[String], field_name: &str) -> Result<()> {
    for (i, value) in values.iter().enumerate() {
        if value.trim().is_empty() {
            bail!("{}[{}] cannot be empty", field_name, i);
        }
    }
    Ok(())
}

/// Validate path exists
pub fn validate_path_exists(path: &Path, field_name: &str) -> Result<()> {
    if !path.exists() {
        bail!("{} path does not exist: {}", field_name, path.display());
    }
    Ok(())
}

/// Validate path is a file
pub fn validate_is_file(path: &Path, field_name: &str) -> Result<()> {
    validate_path_exists(path, field_name)?;
    if !path.is_file() {
        bail!("{} is not a file: {}", field_name, path.display());
    }
    Ok(())
}

/// Validate path is a directory
pub fn validate_is_directory(path: &Path, field_name: &str) -> Result<()> {
    validate_path_exists(path, field_name)?;
    if !path.is_dir() {
        bail!("{} is not a directory: {}", field_name, path.display());
    }
    Ok(())
}

/// Basic URL format validation
pub fn validate_url_format(url: &str, field_name: &str) -> Result<()> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        bail!(
            "{} must be a valid URL starting with http:// or https://",
            field_name
        );
    }
    Ok(())
}

/// Validate alphanumeric identifier
pub fn validate_identifier(id: &str, field_name: &str) -> Result<()> {
    if id.is_empty() {
        bail!("{} cannot be empty", field_name);
    }
    if !id
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        bail!("{} must be alphanumeric (can include _ or -)", field_name);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_non_empty() {
        assert!(validate_non_empty("test", "field").is_ok());
        assert!(validate_non_empty("", "field").is_err());
        assert!(validate_non_empty("   ", "field").is_err());
    }

    #[test]
    fn test_validate_all_non_empty() {
        assert!(validate_all_non_empty(&["a".to_string(), "b".to_string()], "field").is_ok());
        assert!(validate_all_non_empty(&["a".to_string(), "".to_string()], "field").is_err());
        assert!(validate_all_non_empty(&[], "field").is_ok());
    }
}
