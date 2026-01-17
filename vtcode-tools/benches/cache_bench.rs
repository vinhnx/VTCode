use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use std::sync::Arc;
use std::time::Duration;
use vtcode_tools::cache::LruCache;

fn lru_insert_and_get(c: &mut Criterion) {
    // microbenchmark of insert/get using Arc to avoid clones
    c.bench_function("lru_insert_get_arc", |b| {
        let rt = tokio::runtime::Runtime::new().unwrap();
        b.iter(|| {
            let cache = LruCache::new(1000, Duration::from_secs(60));
            let k = "key".to_string();
            let v = Arc::new(black_box(vec!["value".to_string(); 10]));
            // Insert a bunch of entries
            for i in 0..100 {
                let key = format!("{}-{}", k, i);
                rt.block_on(cache.insert_arc(key, Arc::clone(&v)));
            }
            // Access some of them
            for i in 0..100 {
                let key = format!("{}-{}", k, i);
                let _ = rt.block_on(cache.get(&key));
            }
        })
    });
}

fn lru_get_owned_vs_arc(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let cache = LruCache::new(1000, Duration::from_secs(60));
    let k = "key".to_string();
    let v = Arc::new(black_box(vec!["value".to_string(); 50]));

    // Pre-populate cache
    for i in 0..1000 {
        let key = format!("{}-{}", k, i);
        rt.block_on(cache.insert_arc(key, Arc::clone(&v)));
    }

    // Compare clone-based get_owned versus get_arc (cheap Arc clone)
    c.bench_function("lru_get_owned_vs_arc", |b| {
        b.iter(|| {
            for i in 0..1000 {
                let key = format!("{}-{}", k, i);
                let _ = rt.block_on(cache.get_owned(&key));
                let _ = rt.block_on(cache.get(&key));
            }
        })
    });
}

criterion_group!(benches, lru_insert_and_get, lru_get_owned_vs_arc);
criterion_main!(benches);
