//! Path utility functions
//!
//! Re-exports from vtcode-commons for backward compatibility.

pub use vtcode_commons::paths::{
    canonicalize_allow_missing, canonicalize_workspace, normalize_path,
};
pub use vtcode_commons::paths::{normalize_ascii_identifier, resolve_workspace_path};
