//! Workspace path helpers: normalization, canonicalization, containment,
//! safety validation, workspace scopes, and ergonomic extension traits.
//!
//! The implementation lives in focused submodules; everything is re-exported
//! here so the existing `paths::` API is unchanged.

mod canonicalize;
mod containment;
mod normalize;
mod safety;
mod scopes;

pub use canonicalize::{canonicalize, canonicalize_allow_missing, canonicalize_async, canonicalize_workspace};
pub use containment::{
    ensure_path_within_workspace, ensure_path_within_workspace_resolved, resolve_workspace_path, secure_path,
    validate_no_escaping_symlink_ancestors, workspace_relative_display,
};
pub use normalize::{expand_tilde, file_name_from_path, normalize_ascii_identifier, normalize_path};
pub use safety::{is_safe_relative_path, safe_archive_relative_path, validate_path_safety};
pub use scopes::{PathExt, PathResolver, PathScope, StrPathExt, WorkspacePaths};
