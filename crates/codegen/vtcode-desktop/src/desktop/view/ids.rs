//! Stable identity helpers for view elements: opaque path-derived IDs and
//! process-unique ephemeral IDs.

use std::hash::{Hash, Hasher};

pub(super) fn uuid_like_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    NEXT_ID.fetch_add(1, Ordering::Relaxed).to_string()
}

/// Keep local paths out of element IDs while preserving path-based identity across sorting.
pub(super) fn opaque_path_element_id(prefix: &str, path: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    format!("{prefix}-{:016x}", hasher.finish())
}
