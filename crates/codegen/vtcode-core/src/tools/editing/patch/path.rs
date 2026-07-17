use std::path::{Component, Path};

use super::error::PatchError;

pub(crate) fn validate_patch_path(
    operation: &'static str,
    raw_path: &str,
) -> Result<(), PatchError> {
    if raw_path.is_empty() {
        return Err(PatchError::InvalidPath {
            operation,
            path: raw_path.to_string(),
            reason: "path is empty".to_string(),
        });
    }

    if raw_path
        .chars()
        .any(|c| matches!(c, '\0' | '\r' | '\n' | '\t'))
    {
        return Err(PatchError::InvalidPath {
            operation,
            path: raw_path.to_string(),
            reason: "path contains control characters".to_string(),
        });
    }

    let candidate = Path::new(raw_path);
    if candidate.is_absolute() {
        return Err(PatchError::InvalidPath {
            operation,
            path: raw_path.to_string(),
            reason: "path must be relative".to_string(),
        });
    }

    for component in candidate.components() {
        match component {
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(PatchError::InvalidPath {
                    operation,
                    path: raw_path.to_string(),
                    reason: "path escapes workspace".to_string(),
                });
            }
            _ => {}
        }
    }

    if raw_path.contains("//") {
        return Err(PatchError::InvalidPath {
            operation,
            path: raw_path.to_string(),
            reason: "path contains consecutive separators".to_string(),
        });
    }

    Ok(())
}
