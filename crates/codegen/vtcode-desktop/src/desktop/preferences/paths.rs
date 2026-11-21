//! Width clamping and test preferences-path helpers extracted from the store.

use super::*;

pub(in crate::desktop) fn clamp_width(width: f32) -> f32 {
    width.clamp(MIN_PANE_WIDTH, MAX_PANE_WIDTH)
}

#[cfg(test)]
std::thread_local! {
    pub(in crate::desktop) static TEST_PREFERENCES_PATH: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// Drop any leftover file from a dead process so the new test starts from
/// defaults. Test paths embed pid+sequence, which collide after pid reuse.
#[cfg(test)]
pub(in crate::desktop) fn remove_stale_test_file(path: &Path) {
    drop(std::fs::remove_file(path));
}

/// Redirect preference I/O for one GUI-test setup without touching the real
/// user home. Each call receives a fresh file, even when the test runner
/// reuses a worker thread for another test.
#[cfg(test)]
pub(in crate::desktop) fn install_test_preferences_path() -> PathBuf {
    static NEXT_TEST_PATH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let sequence = NEXT_TEST_PATH.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("vtcode-desktop-test-prefs-{}-{sequence}.json", std::process::id()));
    remove_stale_test_file(&path);
    set_test_preferences_path(path.clone());
    path
}

#[cfg(test)]
pub(in crate::desktop) fn set_test_preferences_path(path: PathBuf) {
    TEST_PREFERENCES_PATH.with(|slot| *slot.borrow_mut() = Some(path));
}
