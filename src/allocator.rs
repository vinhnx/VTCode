//! Global allocator selection, shared via `mod allocator;` in `src/main.rs`.
//!
//! The default allocator is `mimalloc`. `allocator-jemalloc` opts into
//! `tikv-jemalloc` (background-thread purging), which returns memory to the OS
//! between bursty/sparse workload idle gaps — beneficial on Linux servers, where
//! `background_thread` is supported. On macOS `background_thread` is unsupported,
//! so jemalloc pins like mimalloc; mimalloc is kept as the default there.

#[cfg(feature = "allocator-jemalloc")]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

#[cfg(not(feature = "allocator-jemalloc"))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
