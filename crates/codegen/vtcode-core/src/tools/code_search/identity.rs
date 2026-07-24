use std::path::{Path, PathBuf};

use crate::tools::code_search::CodeSearchRequest;
use vtcode_commons::canonicalize;

fn normalised_identity_value(
    args: &serde_json::Value,
    include_max_results: bool,
) -> Option<serde_json::Value> {
    let request = serde_json::from_value::<CodeSearchRequest>(args.clone()).ok()?;
    let mut normalised = request.normalise().ok()?;
    normalised.filters.file_types.sort_unstable();
    let mut identity = serde_json::json!({
        "query": normalised.query,
        "path": normalised.filters.path,
        "file_types": normalised.filters.file_types,
        "result_types": normalised.filters.result_types,
    });
    if include_max_results {
        identity["max_results"] = normalised.filters.max_results.into();
    }
    Some(identity)
}

/// Normalised identity for result caching and replay.
///
/// The effective result limit affects both the returned results and echoed
/// filters, so it is part of this identity.
pub fn normalised_identity(args: &serde_json::Value) -> Option<String> {
    serde_json::to_string(&normalised_identity_value(args, true)?).ok()
}

/// Normalised identity for detecting repeated search behaviour.
///
/// Loop detection deliberately ignores the result limit: changing only the
/// requested extent does not make an otherwise repeated search distinct.
pub fn normalised_loop_identity(args: &serde_json::Value) -> Option<String> {
    serde_json::to_string(&normalised_identity_value(args, false)?).ok()
}

/// Return whether a mutated path lies within the file or directory scope of a
/// valid `code_search` request.
///
/// Both paths are resolved against a canonical workspace root so relative and
/// absolute tool arguments share one component-aware comparison. Existing
/// prefixes are canonicalised while missing suffixes remain lexical, which
/// keeps deleted and newly-created targets comparable.
pub fn scope_contains_mutated_path(args: &serde_json::Value, mutated_path: &Path, workspace_root: &Path) -> bool {
    let Ok(request) = serde_json::from_value::<CodeSearchRequest>(args.clone()) else {
        return false;
    };
    let Ok(request) = request.normalise() else {
        return false;
    };
    let workspace_root = canonicalize_existing_prefix(workspace_root);
    let resolve = |path: &Path| {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            workspace_root.join(path)
        };
        canonicalize_existing_prefix(&absolute)
    };
    let scope = resolve(Path::new(request.filters.path.as_str()));
    let mutated_path = resolve(mutated_path);
    mutated_path.starts_with(&scope) || scope.starts_with(&mutated_path)
}

/// Canonicalise the longest existing prefix, then append any missing suffix.
/// This resolves symlinked workspace roots and existing directory aliases while
/// retaining deleted or newly-created mutation targets for replay checks.
pub(super) fn canonicalize_existing_prefix(path: &Path) -> PathBuf {
    let normalised = crate::utils::path::normalize_path(path);
    let mut existing_prefix = normalised.as_path();
    let mut missing_components = Vec::new();

    loop {
        if let Ok(canonical) = canonicalize(existing_prefix) {
            return missing_components.iter().rev().fold(canonical, |mut resolved, component| {
                resolved.push(component);
                resolved
            });
        }
        let Some(component) = existing_prefix.file_name() else {
            return normalised;
        };
        missing_components.push(component.to_os_string());
        let Some(parent) = existing_prefix.parent() else {
            return normalised;
        };
        existing_prefix = parent;
    }
}
