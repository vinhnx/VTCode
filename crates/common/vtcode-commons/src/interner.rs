//! Arena-based string interner for memory-efficient string deduplication.
//!
//! Stores all strings in a single contiguous buffer to minimize allocations
//! and improve cache locality. Uses a hash-based lookup for O(1) interning.
//!
//! # Example
//!
//! ```
//! use vtcode_commons::interner::StringInterner;
//!
//! let mut interner = StringInterner::new();
//! let id1 = interner.intern("src/lib.rs");
//! let id2 = interner.intern("src/lib.rs");
//! assert_eq!(id1, id2);
//! assert_eq!(interner.get(id1), Some("src/lib.rs"));
//! ```

use std::hash::{Hash, Hasher};

use hashbrown::HashMap;
use rustc_hash::FxHasher;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

/// Type alias for HashMap with u64 keys that are already hashed.
type U64NoHashMap<V> = HashMap<u64, V, rustc_hash::FxBuildHasher>;

/// A compact identifier for an interned string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub struct StringId(u32);

impl StringId {
    /// Create a new StringId from a raw u32 value.
    #[inline]
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    /// Get the raw u32 value.
    #[inline]
    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

/// Arena-based string interner for efficient string deduplication.
#[derive(Debug, Clone, Default)]
pub struct StringInterner {
    arena: Vec<u8>,
    lookup: U64NoHashMap<SmallVec<[StringId; 1]>>,
    offsets: Vec<(u32, u16)>,
}

impl StringInterner {
    /// Create a new empty interner.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create an interner with pre-allocated capacity.
    #[must_use]
    pub fn with_capacity(string_bytes: usize, num_strings: usize) -> Self {
        Self {
            arena: Vec::with_capacity(string_bytes),
            lookup: U64NoHashMap::with_capacity_and_hasher(num_strings, rustc_hash::FxBuildHasher),
            offsets: Vec::with_capacity(num_strings),
        }
    }

    /// Intern a byte string, returning its StringId.
    pub fn intern_bytes(&mut self, s: &[u8]) -> StringId {
        let hash = Self::hash_bytes(s);

        if let Some(ids) = self.lookup.get(&hash) {
            for &id in ids {
                if self.get_bytes(id) == Some(s) {
                    return id;
                }
            }
        }

        let start = self.arena.len() as u32;
        let len = s.len() as u16;
        self.arena.extend_from_slice(s);
        let id = StringId::new(self.offsets.len() as u32);
        self.offsets.push((start, len));
        self.lookup.entry(hash).or_default().push(id);
        id
    }

    /// Intern a UTF-8 string. Convenience wrapper around `intern_bytes`.
    #[inline]
    pub fn intern(&mut self, s: &str) -> StringId {
        self.intern_bytes(s.as_bytes())
    }

    /// Get the StringId for a byte string without interning it.
    pub fn get_bytes_id(&self, s: &[u8]) -> Option<StringId> {
        let hash = Self::hash_bytes(s);
        if let Some(ids) = self.lookup.get(&hash) {
            for &id in ids {
                if self.get_bytes(id) == Some(s) {
                    return Some(id);
                }
            }
        }
        None
    }

    /// Get the StringId for a UTF-8 string without interning it.
    #[inline]
    pub fn get_id(&self, s: &str) -> Option<StringId> {
        self.get_bytes_id(s.as_bytes())
    }

    /// Get the raw bytes for a StringId.
    pub fn get_bytes(&self, id: StringId) -> Option<&[u8]> {
        let (start, len) = *self.offsets.get(id.0 as usize)?;
        self.arena.get(start as usize..(start as usize + len as usize))
    }

    /// Get the string for a StringId, if it's valid UTF-8.
    pub fn get(&self, id: StringId) -> Option<&str> {
        self.get_bytes(id).and_then(|b| std::str::from_utf8(b).ok())
    }

    /// Number of interned strings.
    #[inline]
    pub fn len(&self) -> usize {
        self.offsets.len()
    }

    /// Check if the interner is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.offsets.is_empty()
    }

    /// Compute FxHash of a byte slice.
    #[inline]
    fn hash_bytes(s: &[u8]) -> u64 {
        let mut hasher = FxHasher::default();
        s.hash(&mut hasher);
        hasher.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_interning() {
        let mut interner = StringInterner::new();
        let id1 = interner.intern("src");
        let id2 = interner.intern("lib");
        let id3 = interner.intern("src");
        assert_eq!(id1, id3);
        assert_ne!(id1, id2);
        assert_eq!(interner.get(id1), Some("src"));
        assert_eq!(interner.len(), 2);
    }

    #[test]
    fn test_bytes_interning() {
        let mut interner = StringInterner::new();
        let id1 = interner.intern_bytes(b"hello");
        assert_eq!(interner.get(id1), Some("hello"));
    }

    #[test]
    fn test_get_id() {
        let mut interner = StringInterner::new();
        let id_src = interner.intern("src");
        assert_eq!(interner.get_id("src"), Some(id_src));
        assert_eq!(interner.get_id("nonexistent"), None);
    }
}
