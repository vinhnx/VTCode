use std::path::Path;
use std::time::SystemTime;

pub(super) fn generate_session_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("vt-{}-{nanos}", std::process::id())
}

pub(super) fn path_to_string(path: &Path) -> Option<String> {
    Some(path.to_string_lossy().into_owned())
}
