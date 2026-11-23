//! Ephemeral in-memory screenshot store for transcript previews.
//!
//! Screenshots stay out of every durable store (harness events, session
//! archives, logs, model history) by construction: this registry is the only
//! holder, it is memory-only, strictly bounded, never serialized, and it
//! exposes no `Debug` of its bytes. Transcript rows carry a
//! `computer-preview://` reference (row id plus byte metadata, no pixels;
//! see `vtcode_desktop_protocol::preview`); the desktop client resolves it
//! through `computer/preview/read` while the bytes are still retained.
//! Evicted or cleared entries read back as expired, which is the honest UI
//! state, not an error.
//!
//! Lifecycle: the dispatcher retains the latest screenshot of each completed
//! turn under its outer row id; entries evict oldest-first past the caps and
//! drop wholesale on session close. A process-global registry (rather than
//! context threading) fits because producer and consumer share one process,
//! the cache is non-authoritative, and every access is behind one mutex.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};

use vtcode_computer_use::Screenshot;

/// Retention caps: recent turns stay viewable while memory stays flat.
/// Screenshots arrive pre-bounded (8 MiB each), so the entry cap sets the
/// steady state while the byte cap bounds full-size bursts.
const MAX_RETAINED_PREVIEWS: usize = 16;
const MAX_RETAINED_BYTES: usize = 32 * 1024 * 1024;

/// One retained screenshot: validated PNG bytes plus their length for
/// metadata. `Screenshot` itself exposes no `Debug`/`Clone`, so the `Arc`
/// is the only sharing path and nothing can log the pixels by accident.
struct RetainedPreview {
    png: std::sync::Arc<Screenshot>,
    bytes: usize,
}

struct Registry {
    entries: HashMap<String, RetainedPreview>,
    order: VecDeque<String>,
    bytes: usize,
}

impl Registry {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            bytes: 0,
        }
    }
}

fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Registry::new()))
}

fn lock() -> std::sync::MutexGuard<'static, Registry> {
    registry().lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Retain a turn's latest screenshot under its outer row id, evicting
/// oldest-first past the caps. Re-retaining an id replaces it in place.
pub(crate) fn retain_preview(item_id: &str, shot: Screenshot) {
    let bytes = shot.as_png().len();
    let mut registry = lock();
    if let Some(previous) = registry.entries.remove(item_id) {
        registry.bytes = registry.bytes.saturating_sub(previous.bytes);
        registry.order.retain(|id| id != item_id);
    }
    while registry.entries.len() >= MAX_RETAINED_PREVIEWS || registry.bytes.saturating_add(bytes) > MAX_RETAINED_BYTES {
        let Some(oldest) = registry.order.pop_front() else {
            break;
        };
        if let Some(evicted) = registry.entries.remove(&oldest) {
            registry.bytes = registry.bytes.saturating_sub(evicted.bytes);
        }
    }
    registry.order.push_back(item_id.to_string());
    registry.bytes = registry.bytes.saturating_add(bytes);
    registry
        .entries
        .insert(item_id.to_string(), RetainedPreview { png: std::sync::Arc::new(shot), bytes });
}

/// Resolve a retained row id to its bytes. `None` means expired or never
/// retained; the caller reports expiry, never an error.
pub(crate) fn take_preview(item_id: &str) -> Option<std::sync::Arc<Screenshot>> {
    lock().entries.get(item_id).map(|entry| entry.png.clone())
}

/// Drop every retained screenshot. Called on session close so a new session
/// never resolves another session's pixels.
pub(crate) fn clear_previews() {
    let mut registry = lock();
    registry.entries.clear();
    registry.order.clear();
    registry.bytes = 0;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe_png(tag: &[u8]) -> Screenshot {
        Screenshot::png([b"\x89PNG\r\n\x1a\n".as_slice(), tag].concat()).unwrap()
    }

    #[test]
    #[serial_test::serial]
    fn retained_previews_resolve_until_cleared() {
        clear_previews();
        assert!(take_preview("row-1").is_none());
        retain_preview("row-1", probe_png(b"one"));
        assert_eq!(take_preview("row-1").as_ref().map(|shot| shot.as_png()), Some(b"\x89PNG\r\n\x1a\none".as_slice()));
        // Re-retaining replaces the bytes for the same row.
        retain_preview("row-1", probe_png(b"two"));
        assert_eq!(take_preview("row-1").as_ref().map(|shot| shot.as_png()), Some(b"\x89PNG\r\n\x1a\ntwo".as_slice()));
        clear_previews();
        assert!(take_preview("row-1").is_none());
    }

    #[test]
    #[serial_test::serial]
    fn retention_evicts_oldest_first_past_the_caps() {
        clear_previews();
        for index in 0..MAX_RETAINED_PREVIEWS {
            retain_preview(&format!("row-{index}"), probe_png(b"x"));
        }
        assert!(take_preview("row-0").is_some());
        retain_preview("row-new", probe_png(b"x"));
        assert!(take_preview("row-0").is_none(), "oldest entry must evict first");
        assert!(take_preview("row-new").is_some());
        clear_previews();
    }

    #[test]
    #[serial_test::serial]
    fn retention_bounds_total_bytes_across_full_size_shots() {
        // Four full-size screenshots sit exactly at the byte cap; one more
        // byte evicts the oldest instead of growing past it.
        clear_previews();
        let mut full = b"\x89PNG\r\n\x1a\n".to_vec();
        full.resize(8 * 1024 * 1024, 0xA5);
        for index in 0..4 {
            retain_preview(&format!("big-{index}"), Screenshot::png(full.clone()).unwrap());
        }
        assert!(take_preview("big-0").is_some());
        retain_preview("big-new", probe_png(b"x"));
        assert!(take_preview("big-0").is_none(), "byte cap must evict the oldest");
        assert!(take_preview("big-new").is_some());
        clear_previews();
    }
}
