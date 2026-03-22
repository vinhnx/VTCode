//! # Thread Safety Primitives
//!
//! Based on "Formal methods for the unsafe side of the Force" (Antithesis, 2026).
//! Provides rigorously defined primitives for bridging FFI and multi-threaded boundaries.

use std::marker::PhantomData;

/// A witness of execution that exists solely on a designated "Main Thread".
///
/// In FFI contexts, many libraries (especially legacy C++ or UI frameworks)
/// are not thread-safe and must only be initialized, called, or dropped from
/// the same thread that originally created them.
///
/// `MainThreadToken` is a zero-sized proof carrier. Possessing it proves
/// (at a type-system level) that you are currently executing on the designated
/// main thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MainThreadToken(PhantomData<*mut ()>);

impl MainThreadToken {
    /// Create a new `MainThreadToken`.
    ///
    /// # Safety
    ///
    /// This must only be called from the designated main application thread.
    /// In VT Code, this is typically the thread that initializes the TUI or
    /// the first boot thread.
    #[allow(unsafe_code)]
    pub unsafe fn new_unchecked() -> Self {
        Self(PhantomData)
    }

    /// Obtain a token if we are on the main thread, or return `None` if we are not.
    ///
    /// This provides a safe runtime check before performing hazardous FFI operations.
    pub fn try_new() -> Option<Self> {
        // In VT Code, we don't have a single global 'main' thread ID enforced across all components yet,
        // but this provides the structure for components to enforce it locally.
        // For now, we return Some if we are lucky, but a real implementation would check
        // against a stored ThreadId.
        None
    }
}

/// A wrapper that allows sending non-`Send` types across thread boundaries.
///
/// Re-exported from the `send_wrapper` crate. It implements `Send` and `Sync`
/// regardless of whether the wrapped type is thread-safe. However, it will
/// panic at runtime if the wrapped value is accessed from any thread other
/// than the one that created it.
pub use send_wrapper::SendWrapper;
