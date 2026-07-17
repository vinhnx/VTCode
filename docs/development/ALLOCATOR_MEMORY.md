# Allocator & Memory Behavior

VT Code runs a bursty, sparse workload: `Semaphore`-capped concurrency with
`JoinSet` fans-out of many short-lived Tokio tasks, then workers go idle between
bursts (tool batches, LLM streaming, subagents, periodic health/metrics). This
pattern interacts badly with how some allocators return memory to the OS.

## The problem

For this workload, `mimalloc` (the default) and `glibc` hold RSS **flat near
peak** after a burst instead of returning memory. The mechanism: tasks allocated
on one worker thread are often stolen and dropped on another. In mimalloc v3,
cross-thread frees land on the owning page's `xthread_free` list and are only
reconciled by *future allocation activity*. When Tokio workers park between
bursts, that activity never arrives, so the freed chunks stay stranded and RSS
does not drop. See
<https://pranitha.dev/posts/rust-and-memory-allocators/>.

`jemalloc` avoids this only when its `background_thread` is active: it purges
dirty pages on a decay timer (`dirty_decay_ms`/`muzzy_decay_ms`) independent of
allocation activity.

## Allocator selection

The default allocator is `mimalloc`. `tikv-jemalloc` is opt-in via the
`allocator-jemalloc` feature:

```bash
# default: mimalloc
cargo run --release --bin vtcode -- bench-allocator

# jemalloc (background-thread purging)
cargo run --release --bin vtcode --features allocator-jemalloc -- bench-allocator
```

- **Linux (containers / long-lived servers):** build with `--features
  allocator-jemalloc`. `background_thread` is supported there, so memory returns
  to the OS between bursts. Tune via `MALLOC_CONF` (`background_thread:true,
  dirty_decay_ms:10000, muzzy_decay_ms:0`) before the first allocation.
- **macOS (dev):** keep the `mimalloc` default — it is lower-latency, and
  jemalloc's `background_thread` is unsupported on this platform (it prints
  `background_thread currently supports pthread only` and pins like mimalloc).

## Allocation throughput trade-off (measured)

`jemalloc` trades allocation speed for better memory behavior. Measured on macOS
(`cargo bench --bench allocator_throughput`, `release-fast`):

| Benchmark | mimalloc | jemalloc | Delta |
|---|---|---|---|
| `alloc_free_64B` (200k iters) | 583 us | 2.65 ms | jemalloc ~4.7x slower |
| `alloc_free_4KB` (20k iters) | 763 us | 881 us | jemalloc ~17% slower |
| `event_burst` 200x1000 | 23.0 ms | 30.0 ms | jemalloc ~30% slower |

So on macOS, switching to jemalloc would *hurt* allocation latency with no memory
benefit (background_thread unsupported) — which is why `mimalloc` stays the
default there. On Linux, jemalloc's memory reclamation is the win; the latency
cost is the accepted trade for long-lived servers.

## Measuring it

Two tools, no provider/API key needed:

- **RSS / memory pinning** — `vtcode bench-allocator` prints the active
  allocator and a per-burst RSS trajectory (`peak_rss_mb`, `post_burst_mb`,
  `post_idle_mb`, `retained_after_idle_mb`). The diagnostic signature is
  `post_idle_mb` staying near `peak_rss_mb` (allocator not returning memory).
- **Allocation throughput** — `cargo bench --bench allocator_throughput`
  (optionally `--features allocator-jemalloc`) compares allocation speed of the
  two allocators for small, large, and mixed event-shaped allocations.

Measured on macOS (dev machine), identical `bench-allocator` workload (2 bursts x
20 events x 200 tasks, 2s idle):

| Allocator | Baseline MB | Final MB | Retained | Note |
|---|---|---|---|---|
| mimalloc (default) | 25.2 | 55.0 | +118% | pins |
| jemalloc | 24.8 | 44.9 | +81% | pins on macOS only |

The jemalloc row was measured on macOS where `background_thread` is unsupported;
on Linux the same build returns memory between bursts (per the article's
container findings).

## Implementation

- Allocator selection lives in `src/allocator.rs` (a `mod allocator;` in
  `src/main.rs`). `mimalloc` is the default; `allocator-jemalloc` swaps in
  `tikv_jemallocator` (with the `background_threads` feature). The throughput
  benchmark (`benches/allocator_throughput.rs`) uses the same selection so it
  compares the active allocator, not the system default.
- **Rust 1.93.0**: global allocators written in Rust can now safely use
  `thread_local!` and `std::thread::current` without re-entrancy concerns. This
  removes a previous limitation that could cause issues with Rust-based allocators
  using thread-local storage.
- RSS sampling lives in `vtcode-commons::memory` (real values on macOS via
  `mach_task_basic_info`, on Linux via `/proc/self/statm`) — unlike
  `performance_profiler::get_memory_usage_mb`, which is Linux-only with a fake
  macOS fallback.
- The `bench-allocator` command is implemented in `src/cli/bench_allocator.rs`
  and wired through `vtcode_core::cli::args::BenchAllocatorArgs`.
