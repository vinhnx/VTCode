//! Lock-free concurrent map built on [`left_right`].
//!
//! [`LrMap`] keeps two copies of a `HashMap` — readers see one copy while the
//! writer mutates the other. On publish the copies swap, giving readers a
//! consistent, wait-free snapshot.
//!
//! Best for read-heavy workloads with infrequent writes (caches, registries).

use hashbrown::HashMap;
use left_right::{Absorb, ReadHandleFactory, WriteHandle};
use std::hash::Hash;
use std::sync::Mutex;

enum MapOp<K, V> {
    Insert(K, V),
    Clear,
}

impl<K: Eq + Hash + Clone, V: Clone> Absorb<MapOp<K, V>> for HashMap<K, V> {
    fn absorb_first(&mut self, operation: &mut MapOp<K, V>, _other: &Self) {
        match operation {
            MapOp::Insert(k, v) => {
                self.insert(k.clone(), v.clone());
            }
            MapOp::Clear => self.clear(),
        }
    }

    fn sync_with(&mut self, first: &Self) {
        self.clone_from(first);
    }
}

/// A concurrent map optimized for read-heavy workloads.
///
/// Readers never block — not even while a write is in progress. Writers are
/// serialized through an internal [`Mutex`].
///
/// Trade-off: doubled memory (two copies of the map).
pub struct LrMap<K: Eq + Hash + Clone, V: Clone> {
    reader_factory: ReadHandleFactory<HashMap<K, V>>,
    writer: Mutex<WriteHandle<HashMap<K, V>, MapOp<K, V>>>,
}

impl<K, V> LrMap<K, V>
where
    K: Eq + Hash + Clone + Send + Sync,
    V: Clone + Send + Sync,
{
    pub fn new() -> Self {
        let (writer, reader) = left_right::new_from_empty(HashMap::new());
        let factory = reader.factory();
        Self {
            reader_factory: factory,
            writer: Mutex::new(writer),
        }
    }

    /// Lock-free lookup returning a clone of the value.
    pub fn get<Q>(&self, key: &Q) -> Option<V>
    where
        K: std::borrow::Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let reader = self.reader_factory.handle();
        reader.enter().and_then(|map| map.get(key).cloned())
    }

    pub fn insert(&self, key: K, value: V) {
        if let Ok(mut w) = self.writer.lock() {
            w.append(MapOp::Insert(key, value));
            w.publish();
        }
    }

    pub fn clear(&self) {
        if let Ok(mut w) = self.writer.lock() {
            w.append(MapOp::Clear);
            w.publish();
        }
    }

    pub fn len(&self) -> usize {
        let reader = self.reader_factory.handle();
        reader.enter().map(|m| m.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<K, V> Default for LrMap<K, V>
where
    K: Eq + Hash + Clone + Send + Sync,
    V: Clone + Send + Sync,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn insert_and_get() {
        let map: LrMap<String, i32> = LrMap::new();
        map.insert("a".into(), 1);
        assert_eq!(map.get("a"), Some(1));
        assert_eq!(map.get("b"), None);
    }

    #[test]
    fn overwrite_key() {
        let map: LrMap<String, i32> = LrMap::new();
        map.insert("a".into(), 1);
        map.insert("a".into(), 2);
        assert_eq!(map.get("a"), Some(2));
    }

    #[test]
    fn clear_removes_all() {
        let map: LrMap<String, i32> = LrMap::new();
        map.insert("a".into(), 1);
        map.insert("b".into(), 2);
        map.clear();
        assert!(map.is_empty());
    }

    #[test]
    fn concurrent_reads() {
        let map: Arc<LrMap<String, i32>> = Arc::new(LrMap::new());
        map.insert("key".into(), 42);

        let handles: Vec<_> = (0..4)
            .map(|_| {
                let m = Arc::clone(&map);
                std::thread::spawn(move || {
                    for _ in 0..100 {
                        assert_eq!(m.get("key"), Some(42));
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().expect("reader thread panicked");
        }
    }
}
