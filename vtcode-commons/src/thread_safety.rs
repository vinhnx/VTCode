//! # Thread Safety Primitives
//!
//! Based on "Formal methods for the unsafe side of the Force" (Antithesis, 2026).
//! Provides rigorously defined primitives for bridging FFI and multi-threaded boundaries.

use std::marker::PhantomData;
use std::sync::OnceLock;
use std::thread::{self, ThreadId};

/// Stores the `ThreadId` designated as the application's main thread.
///
/// Populated exactly once by [`designate_main_thread`]; subsequent calls are no-ops
/// so that callers can re-assert designation from defensive initialization paths
/// without panicking.
static MAIN_THREAD_ID: OnceLock<ThreadId> = OnceLock::new();

/// Designate the calling thread as the application's main thread.
///
/// Should be invoked once, early in `main`, before spawning any worker threads
/// that may try to obtain a [`MainThreadToken`]. Subsequent calls have no effect.
pub fn designate_main_thread() {
    let _ = MAIN_THREAD_ID.set(thread::current().id());
}

/// Returns the `ThreadId` previously designated as the main thread, if any.
pub fn main_thread_id() -> Option<ThreadId> {
    MAIN_THREAD_ID.get().copied()
}

/// A witness of execution that exists solely on a designated "Main Thread".
///
/// In FFI contexts, many libraries (especially legacy C++ or UI frameworks)
/// are not thread-safe and must only be initialized, called, or dropped from
/// the same thread that originally created them.
///
/// `MainThreadToken` is a zero-sized proof carrier. Possessing it proves
/// (at a type-system level) that the holder previously executed on the
/// designated main thread. The `PhantomData<*mut ()>` makes the token
/// `!Send + !Sync`, so a token obtained on the main thread cannot leak to
/// another thread through ordinary safe code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MainThreadToken(PhantomData<*mut ()>);

impl MainThreadToken {
    /// Create a new `MainThreadToken` without verifying the current thread.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that:
    /// 1. They are executing on the thread that was (or will be) passed to
    ///    [`designate_main_thread`], and
    /// 2. The resulting token will not be transmitted to another thread
    ///    through `unsafe` channels (the type is `!Send + !Sync`, which
    ///    prevents safe channels from doing so).
    #[expect(
        unsafe_code,
        reason = "phantom data marker; !Send + !Sync prevents token leakage"
    )]
    pub unsafe fn new_unchecked() -> Self {
        Self(PhantomData)
    }

    /// Obtain a token if the current thread matches the one previously passed
    /// to [`designate_main_thread`].
    ///
    /// Returns `None` if [`designate_main_thread`] has never been called, or
    /// if the current thread is not the designated main thread.
    pub fn try_new() -> Option<Self> {
        let designated = MAIN_THREAD_ID.get()?;
        if *designated == thread::current().id() {
            Some(Self(PhantomData))
        } else {
            None
        }
    }
}

/// A wrapper that allows sending non-`Send` types across thread boundaries.
///
/// Re-exported from the `send_wrapper` crate. It implements `Send` and `Sync`
/// regardless of whether the wrapped type is thread-safe. However, it will
/// panic at runtime if the wrapped value is accessed from any thread other
/// than the one that created it.
pub use send_wrapper::SendWrapper;

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn worker_thread_never_obtains_token() {
        // A spawned worker thread is never the designated main thread, even if
        // some other test in this process has called `designate_main_thread`
        // on a different thread. The token type is `!Send`, so we materialize
        // it inside the worker and return only its presence as a `bool`.
        let on_worker = thread::spawn(|| MainThreadToken::try_new().is_some())
            .join()
            .expect("worker thread");
        assert!(!on_worker);
    }

    #[test]
    fn try_new_returns_some_after_designation_on_same_thread() {
        designate_main_thread();
        // If this test happens to run on the same thread that another test
        // designated, we still get a token; if a different thread was
        // designated first, `try_new` correctly returns `None`.
        match main_thread_id() {
            Some(id) if id == thread::current().id() => {
                assert!(MainThreadToken::try_new().is_some());
            }
            _ => {
                assert!(MainThreadToken::try_new().is_none());
            }
        }
    }
}
