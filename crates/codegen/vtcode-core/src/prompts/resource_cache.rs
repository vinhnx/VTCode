//! Shared cache primitives for prompt resources loaded from the filesystem.

use std::collections::HashMap;
use std::fs;
use std::hash::Hash;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use tracing::warn;

/// How long a parsed prompt resource remains eligible for reuse.
const RESOURCE_CACHE_TTL: Duration = Duration::from_secs(5 * 60);

/// Minimum interval between source metadata scans for a cache entry.
const RESOURCE_METADATA_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Maximum number of source roots cached by one prompt-resource cache.
const RESOURCE_CACHE_MAX_ENTRIES: usize = 32;

/// Metadata needed to detect a source-file change without reading its body.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct FileMetadataFingerprint {
    source_index: usize,
    relative_path: PathBuf,
    size: u64,
    modified_nanos: u128,
}

impl FileMetadataFingerprint {
    fn from_metadata(source_index: usize, relative_path: PathBuf, metadata: &fs::Metadata) -> Self {
        let modified_nanos = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map_or(0, |duration| duration.as_nanos());

        Self {
            source_index,
            relative_path,
            size: metadata.len(),
            modified_nanos,
        }
    }
}

/// Compute a deterministic fingerprint for regular markdown files below each
/// source directory. The directory scan is intentionally synchronous so
/// callers can run it inside `spawn_blocking` alongside parsing.
pub(crate) fn fingerprint_markdown_directories(directories: &[&Path]) -> Vec<FileMetadataFingerprint> {
    let mut fingerprint = Vec::new();

    for (source_index, directory) in directories.iter().enumerate() {
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                warn!(path = %directory.display(), %error, "prompt resource metadata scan failed");
                continue;
            }
        };

        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let is_markdown = path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("md"));
            if !is_markdown {
                continue;
            }

            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if !metadata.is_file() {
                continue;
            }

            let relative_path = path.trim_prefix(directory).to_path_buf();
            fingerprint.push(FileMetadataFingerprint::from_metadata(source_index, relative_path, &metadata));
        }
    }

    fingerprint.sort_unstable_by(|left, right| {
        left.source_index
            .cmp(&right.source_index)
            .then_with(|| left.relative_path.cmp(&right.relative_path))
    });
    fingerprint
}

/// Compute a deterministic fingerprint for a set of individual source files.
pub(crate) fn fingerprint_files(files: &[&Path]) -> Vec<FileMetadataFingerprint> {
    let mut fingerprint = Vec::new();

    for (source_index, file) in files.iter().enumerate() {
        let Ok(metadata) = fs::metadata(file) else {
            continue;
        };
        if metadata.is_file() {
            let relative_path = file.file_name().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("resource"));
            fingerprint.push(FileMetadataFingerprint::from_metadata(source_index, relative_path, &metadata));
        }
    }

    fingerprint
}

/// Canonicalize a cache key path while retaining a useful path for sources
/// that have not been created yet.
pub(crate) fn canonical_cache_path(path: &Path) -> PathBuf {
    struct CanonicalPathEntry {
        canonical: PathBuf,
        checked_at: Instant,
    }

    static CANONICAL_PATHS: std::sync::OnceLock<Mutex<HashMap<PathBuf, CanonicalPathEntry>>> =
        std::sync::OnceLock::new();

    let cache = CANONICAL_PATHS.get_or_init(|| Mutex::new(HashMap::new()));
    let source_path = path.to_path_buf();
    {
        let entries = cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(entry) = entries.get(&source_path)
            && entry.checked_at.elapsed() <= RESOURCE_METADATA_POLL_INTERVAL
        {
            return entry.canonical.clone();
        }
    }

    let canonical = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut entries = cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if entries.len() >= RESOURCE_CACHE_MAX_ENTRIES && !entries.contains_key(&source_path) {
        // This is only a small canonicalization memo, not the parsed-resource
        // cache. Any existing alias may be evicted because a miss simply
        // recomputes the canonical path.
        if let Some(evicted_path) = entries.keys().next().cloned() {
            entries.remove(&evicted_path);
        }
    }
    entries.insert(
        source_path,
        CanonicalPathEntry {
            canonical: canonical.clone(),
            checked_at: Instant::now(),
        },
    );
    canonical
}

struct CacheEntry<V> {
    value: Arc<V>,
    loaded_at: Instant,
    metadata_checked_at: Instant,
    fingerprint: Vec<FileMetadataFingerprint>,
}

struct CacheState<K, V> {
    entries: HashMap<K, CacheEntry<V>>,
}

/// A small, concurrency-safe, source-aware resource cache.
///
/// The cache uses a short metadata polling interval so warm prompt assembly
/// does not scan source directories on every turn. A cache miss is serialized
/// by [`Self::with_load_gate`] so concurrent first requests do not stampede the
/// filesystem or parser.
pub(crate) struct ResourceCache<K, V> {
    state: Mutex<CacheState<K, V>>,
    load_gate: Mutex<()>,
}

impl<K, V> Default for ResourceCache<K, V> {
    fn default() -> Self {
        Self {
            state: Mutex::new(CacheState { entries: HashMap::new() }),
            load_gate: Mutex::new(()),
        }
    }
}

impl<K, V> ResourceCache<K, V>
where
    K: Eq + Hash + Clone,
{
    /// Return a value without touching the filesystem when the metadata poll
    /// interval has not elapsed.
    pub(crate) fn fast_get(&self, key: &K) -> Option<Arc<V>> {
        let state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let entry = state.entries.get(key)?;
        if entry.loaded_at.elapsed() > RESOURCE_CACHE_TTL
            || entry.metadata_checked_at.elapsed() > RESOURCE_METADATA_POLL_INTERVAL
        {
            return None;
        }
        Some(Arc::clone(&entry.value))
    }

    /// Reuse a value after a metadata scan confirms that its sources are
    /// unchanged. This also advances the next metadata polling deadline.
    pub(crate) fn get_if_unchanged(&self, key: &K, fingerprint: &[FileMetadataFingerprint]) -> Option<Arc<V>> {
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let entry = state.entries.get_mut(key)?;
        if entry.loaded_at.elapsed() > RESOURCE_CACHE_TTL || entry.fingerprint != fingerprint {
            return None;
        }
        entry.metadata_checked_at = Instant::now();
        Some(Arc::clone(&entry.value))
    }

    /// Insert or replace a parsed resource and evict the oldest entry when
    /// the bounded cache is full.
    pub(crate) fn insert(&self, key: K, value: V, fingerprint: Vec<FileMetadataFingerprint>) {
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        state.entries.retain(|_, entry| entry.loaded_at.elapsed() <= RESOURCE_CACHE_TTL);

        if state.entries.len() >= RESOURCE_CACHE_MAX_ENTRIES && !state.entries.contains_key(&key) {
            let oldest_key = state
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.loaded_at)
                .map(|(entry_key, _)| entry_key.clone());
            if let Some(oldest_key) = oldest_key {
                state.entries.remove(&oldest_key);
            }
        }

        let now = Instant::now();
        state.entries.insert(
            key,
            CacheEntry {
                value: Arc::new(value),
                loaded_at: now,
                metadata_checked_at: now,
                fingerprint,
            },
        );
    }

    /// Serialize only cache-miss work. The closure must not hold any other
    /// lock across an async boundary; async callers invoke it inside a
    /// blocking task after the fast cache path has failed.
    pub(crate) fn with_load_gate<R>(&self, load: impl FnOnce() -> R) -> R {
        let _guard = self.load_gate.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        load()
    }

    #[cfg(test)]
    pub(crate) fn clear(&self) {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entries
            .clear();
    }

    #[cfg(test)]
    pub(crate) fn force_metadata_poll(&self) {
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        for entry in state.entries.values_mut() {
            entry.metadata_checked_at = Instant::now()
                .checked_sub(RESOURCE_METADATA_POLL_INTERVAL + Duration::from_millis(1))
                .expect("test timestamp should have a representable history");
        }
    }

    #[cfg(test)]
    pub(crate) fn force_expiration(&self) {
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        for entry in state.entries.values_mut() {
            entry.loaded_at = Instant::now()
                .checked_sub(RESOURCE_CACHE_TTL + Duration::from_millis(1))
                .expect("test timestamp should have a representable history");
        }
    }
}
