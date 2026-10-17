//! Deterministic offline workloads for production runtime entrypoints.
#![allow(
    missing_docs,
    clippy::expect_used,
    reason = "Benchmark fixture failures must stop the measurement."
)]

#[path = "runtime_paths/support.rs"]
mod runtime_paths;

use criterion::{Criterion, criterion_group};

fn workloads(c: &mut Criterion) {
    runtime_paths::benchmarks(c);
}

criterion_group!(benches, workloads);

fn main() {
    // Nextest's all-targets discovery must not start benchmark peers.
    if std::env::args().any(|argument| argument == "--list") {
        return;
    }
    #[cfg(feature = "profiling")]
    let _profiler = hotpath::HotpathGuardBuilder::new("runtime_paths").build();
    benches();
    Criterion::default().configure_from_args().final_summary();
}
