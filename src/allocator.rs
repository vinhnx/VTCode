//! Global allocator selection, shared via `mod allocator;` in `src/main.rs`.
//!
//! The default allocator is `mimalloc`. `allocator-jemalloc` opts into
//! `tikv-jemalloc` (background-thread purging), which returns memory to the OS
//! between bursty/sparse workload idle gaps — beneficial on Linux servers, where
//! `background_thread` is supported. On macOS `background_thread` is unsupported,
//! so jemalloc pins like mimalloc; mimalloc is kept as the default there. The Linux
//! jemalloc config also disables THP (`thp:never`) so RSS stays comparable with
//! jemalloc's resident metric; see `docs/development/allocator_memory.md`.

// `hotpath::main` installs its own allocator when profiling is enabled. Keep
// this module's allocator definitions mutually exclusive with that feature so
// `--all-features` remains a valid build configuration; profiling takes
// precedence over the selectable production allocators.
#[cfg(all(feature = "allocator-jemalloc", not(feature = "profiling"), unix))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

#[cfg(all(feature = "allocator-jemalloc", not(feature = "profiling"), unix))]
#[allow(
    unsafe_code,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
mod jemalloc_malloc_conf {
    /// jemalloc looks up `extern const char *malloc_conf` — a thin pointer,
    /// not a Rust `&[u8]` fat pointer.
    #[repr(transparent)]
    struct MallocConfPtr(*const u8);
    // SAFETY: `MallocConfPtr` is `#[repr(transparent)]` over a `*const u8` that
    // points exclusively at a process-lifetime byte-string literal — statically
    // backed, immutable data with no interior mutability. Sharing `&MallocConfPtr`
    // across threads only copies the pointer value itself (a `Copy` type) and,
    // when dereferenced, reads read-only bytes. There is no mutable aliasing of
    // the pointed-to data and the literal lives for the entire program duration,
    // so `Sync` is sound.
    unsafe impl Sync for MallocConfPtr {}
    // `thp:never` keeps jemalloc's mappings off transparent huge pages. With THP
    // set to `always` at the host level, small allocations can be promoted to 2 MiB
    // pages that jemalloc does not count as resident, so RSS diverges from every
    // allocator metric and never falls after a burst. Linux-only: jemalloc rejects
    // the `thp` option outright on platforms without MADV_HUGEPAGE/memcntl (e.g.
    // macOS), where the prefixed `_rjem_malloc_conf` symbol is used instead.
    #[cfg(target_os = "linux")]
    static CONF: &[u8] = b"prof:true,prof_active:false,lg_prof_sample:19,prof_final:false,thp:never\0";
    #[cfg(not(target_os = "linux"))]
    static CONF: &[u8] = b"prof:true,prof_active:false,lg_prof_sample:19,prof_final:false\0";
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    #[used]
    #[allow(
        unsafe_code,
        reason = "Intentional compatibility, platform, or test-only suppression."
    )]
    #[unsafe(export_name = "malloc_conf")]
    static MALLOC_CONF: MallocConfPtr = MallocConfPtr(CONF.as_ptr());
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    #[used]
    #[allow(
        unsafe_code,
        reason = "Intentional compatibility, platform, or test-only suppression."
    )]
    #[unsafe(export_name = "_rjem_malloc_conf")]
    static MALLOC_CONF: MallocConfPtr = MallocConfPtr(CONF.as_ptr());
}

#[cfg(all(not(feature = "allocator-jemalloc"), not(feature = "profiling")))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
