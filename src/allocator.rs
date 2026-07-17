//! Global allocator selection, shared via `mod allocator;` in `src/main.rs`.
//!
//! The default allocator is `mimalloc`. `allocator-jemalloc` opts into
//! `tikv-jemalloc` (background-thread purging), which returns memory to the OS
//! between bursty/sparse workload idle gaps — beneficial on Linux servers, where
//! `background_thread` is supported. On macOS `background_thread` is unsupported,
//! so jemalloc pins like mimalloc; mimalloc is kept as the default there.

#[cfg(all(feature = "allocator-jemalloc", unix))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

#[cfg(all(feature = "allocator-jemalloc", unix))]
#[allow(unsafe_code)]
mod jemalloc_malloc_conf {
    /// jemalloc looks up `extern const char *malloc_conf` — a thin pointer,
    /// not a Rust `&[u8]` fat pointer.
    #[repr(transparent)]
    struct MallocConfPtr(*const u8);
    #[allow(clippy::undocumented_unsafe_blocks)]
    unsafe impl Sync for MallocConfPtr {}
    static CONF: [u8; 63] = *b"prof:true,prof_active:false,lg_prof_sample:19,prof_final:false\0";
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    #[used]
    #[allow(unsafe_code)]
    #[unsafe(export_name = "malloc_conf")]
    static MALLOC_CONF: MallocConfPtr = MallocConfPtr(CONF.as_ptr());
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    #[used]
    #[allow(unsafe_code)]
    #[unsafe(export_name = "_rjem_malloc_conf")]
    static MALLOC_CONF: MallocConfPtr = MallocConfPtr(CONF.as_ptr());
}

#[cfg(not(feature = "allocator-jemalloc"))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
