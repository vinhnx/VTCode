//! Allocation throughput benchmark comparing the active global allocator
//! (`mimalloc` vs `jemalloc`) for vtcode's allocation patterns.
//!
//! Run with the default allocator (mimalloc on macOS):
//!
//! ```bash
//! cargo bench --bench allocator_throughput
//! ```
//!
//! Run with jemalloc (any platform, via feature):
//!
//! ```bash
//! cargo bench --bench allocator_throughput --features allocator-jemalloc
//! ```
//!
//! Lower times = faster allocator. This measures allocation latency/throughput
//! only — it does NOT measure RSS reclamation (see `vtcode bench-allocator`).
//!
//! Build with the default allocator (mimalloc):
//!   cargo bench --bench allocator_throughput
//! Build with jemalloc:
//!   cargo bench --bench allocator_throughput --features allocator-jemalloc

// Use the exact same global allocator selection as the binary so the benchmark
// compares the active allocator, not the system default.
#[cfg(feature = "allocator-jemalloc")]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

#[cfg(not(feature = "allocator-jemalloc"))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

fn make_payload(n: usize) -> Vec<u8> {
    std::iter::repeat_n(0x5Au8, n).collect()
}

fn bench_small_churn(c: &mut Criterion) {
    let mut group = c.benchmark_group("alloc_small_churn");
    let count = 200_000usize;
    group.throughput(Throughput::Elements(count as u64));
    group.bench_function("alloc_free_64B", |b| {
        b.iter(|| {
            for _ in 0..count {
                let v: Vec<u8> = Vec::with_capacity(64);
                std::hint::black_box(&v);
            }
        });
    });
    group.finish();
}

fn bench_large_churn(c: &mut Criterion) {
    let mut group = c.benchmark_group("alloc_large_churn");
    let count = 20_000usize;
    group.throughput(Throughput::Bytes((count * 4096) as u64));
    group.bench_function("alloc_free_4KB", |b| {
        b.iter(|| {
            for _ in 0..count {
                let v = make_payload(4096);
                std::hint::black_box(&v);
            }
        });
    });
    group.finish();
}

/// Mirrors the bursty event workload: per event a 4KB payload plus a vector of
/// ~1KB token strings, all allocated and dropped together.
fn bench_event_burst(c: &mut Criterion) {
    let mut group = c.benchmark_group("alloc_event_burst");
    let events = 200usize;
    let tokens = 1000usize;
    group.throughput(Throughput::Elements((events * tokens) as u64));
    group.bench_function(BenchmarkId::new("payload_and_tokens", "{events}x{tokens}"), |b| {
        b.iter(|| {
            for _ in 0..events {
                let payload = make_payload(4096);
                let tokens_vec: Vec<String> =
                    (0..tokens).map(|i| format!("token-{i}-{i:064}")).collect();
                std::hint::black_box((&payload, &tokens_vec));
            }
        });
    });
    group.finish();
}

criterion_group!(benches, bench_small_churn, bench_large_churn, bench_event_burst);
criterion_main!(benches);
