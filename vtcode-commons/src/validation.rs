//! Validation utilities for common operations

use anyhow::{Result, bail};
use std::path::Path;

/// Validate that a string is non-empty
pub fn validate_non_empty(value: &str, field_name: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{field_name} cannot be empty");
    }
    Ok(())
}

/// Validate and return non-empty string
pub fn validate_non_empty_string(value: String, field_name: &str) -> Result<String> {
    if value.trim().is_empty() {
        bail!("{field_name} cannot be empty");
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
        bail!("{field_name} collection cannot be empty");
    }
    Ok(())
}

/// Validate that all strings in a slice are non-empty
pub fn validate_all_non_empty(values: &[String], field_name: &str) -> Result<()> {
    for (i, value) in values.iter().enumerate() {
        if value.trim().is_empty() {
            bail!("{field_name}[{i}] cannot be empty");
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
        bail!("{field_name} must be a valid URL starting with http:// or https://");
    }
    Ok(())
}

/// Validate alphanumeric identifier
pub fn validate_identifier(id: &str, field_name: &str) -> Result<()> {
    if id.is_empty() {
        bail!("{field_name} cannot be empty");
    }
    if !id
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        bail!("{field_name} must be alphanumeric (can include _ or -)");
    }
    Ok(())
}

/// A validated string that is guaranteed to be non-empty after trimming.
///
/// Follows the **"Parse Don't Validate"** pattern (Ch 15): the constraint is
/// enforced at construction time via [`TryFrom`], so downstream code never
/// needs to re-check.
///
/// ```rust
/// use vtcode_commons::validation::NonEmptyString;
///
/// let name = NonEmptyString::try_from("hello").unwrap();
/// assert_eq!(name.as_str(), "hello");
///
/// assert!(NonEmptyString::try_from("").is_err());
/// assert!(NonEmptyString::try_from("   ").is_err());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NonEmptyString(String);

impl NonEmptyString {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl std::ops::Deref for NonEmptyString {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::borrow::Borrow<str> for NonEmptyString {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for NonEmptyString {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for NonEmptyString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl TryFrom<String> for NonEmptyString {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.trim().is_empty() {
            Err("string must be non-empty".to_string())
        } else {
            Ok(Self(value))
        }
    }
}

impl TryFrom<&str> for NonEmptyString {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if value.trim().is_empty() {
            Err("string must be non-empty".to_string())
        } else {
            Ok(Self(value.to_string()))
        }
    }
}

impl From<NonEmptyString> for String {
    fn from(value: NonEmptyString) -> Self {
        value.0
    }
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

    #[test]
    fn non_empty_string_accepts_valid() {
        let s = NonEmptyString::try_from("hello").unwrap();
        assert_eq!(s.as_str(), "hello");
        assert_eq!(s.len(), 5);
    }

    #[test]
    fn non_empty_string_rejects_empty() {
        assert!(NonEmptyString::try_from("").is_err());
        assert!(NonEmptyString::try_from("   ").is_err());
        assert!(NonEmptyString::try_from("\t\n").is_err());
    }

    #[test]
    fn non_empty_string_from_owned() {
        let s = NonEmptyString::try_from("test".to_string()).unwrap();
        assert_eq!(s.into_inner(), "test");
    }

    #[test]
    fn non_empty_string_deref() {
        let s = NonEmptyString::try_from("hello").unwrap();
        assert!(s.starts_with("hel"));
        assert_eq!(&*s, "hello");
    }
}
