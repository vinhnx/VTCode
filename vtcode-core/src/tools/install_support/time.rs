use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn unix_timestamp_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub(super) fn modified_age_secs(path: &Path) -> Option<u64> {
    let metadata = fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?;
    Some(
        SystemTime::now()
            .duration_since(modified)
            .unwrap_or_default()
            .as_secs(),
    )
}
