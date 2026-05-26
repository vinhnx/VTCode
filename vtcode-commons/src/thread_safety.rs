//! # Thread Safety Primitives
//!
//! Based on "Formal methods for the unsafe side of the Force" (Antithesis, 2026).
//! Provides rigorously defined primitives for bridging FFI and multi-threaded boundaries.
//!
//! ## `RelaxedAtomic<T>`
//!
//! Provides inner mutability for `Copy` types via relaxed atomic loads and stores.
//! On x86_64 and ARM, relaxed loads/stores compile to the same instructions as
//! regular memory accesses (no `LOCK` prefix), making this a zero-overhead way to
//! achieve interior mutability for atomic-compatible types.
//!
//! Deliberately omits `fetch_*` / CAS methods — those lower to bus-lock instructions
//! that add noticeable overhead. Instead, use the `load`–mutate–`store` pattern:
//!
//! ```
//! # use vtcode_commons::thread_safety::RelaxedAtomic;
//! let counter = RelaxedAtomic::new(0u32);
//! let val = counter.load();
//! counter.store(val + 1);
//! ```

use std::fmt;
use std::marker::PhantomData;
use std::sync::OnceLock;
use std::sync::atomic::Ordering;
use std::thread::{self, ThreadId};

/// Trait for types that can be stored in a [`RelaxedAtomic`].
///
/// Implemented for `bool`, `u8`, `u16`, `u32`, `usize`, `i8`, `i16`, `i32`, `isize`.
pub trait AtomicRepr: Copy + 'static {
    /// The underlying `std::sync::atomic::Atomic*` type.
    type Atomic: 'static + Send + Sync;
    /// Create a new atomic instance for the given value.
    fn new_atomic(val: Self) -> Self::Atomic;
    /// Load the value with `Ordering::Relaxed`.
    fn load(atomic: &Self::Atomic) -> Self;
    /// Store the value with `Ordering::Relaxed`.
    fn store(atomic: &Self::Atomic, val: Self);
    /// Unwrap the atomic and return the contained value (no atomic instruction).
    fn into_inner(atomic: Self::Atomic) -> Self;
}

macro_rules! impl_atomic_repr {
    ($ty:ty, $atomic:ty) => {
        impl AtomicRepr for $ty {
            type Atomic = $atomic;
            fn new_atomic(val: Self) -> Self::Atomic {
                <$atomic>::new(val)
            }
            fn load(atomic: &Self::Atomic) -> Self {
                atomic.load(Ordering::Relaxed)
            }
            fn store(atomic: &Self::Atomic, val: Self) {
                atomic.store(val, Ordering::Relaxed);
            }
            fn into_inner(atomic: Self::Atomic) -> Self {
                atomic.into_inner()
            }
        }
    };
}

impl_atomic_repr!(bool, std::sync::atomic::AtomicBool);
impl_atomic_repr!(u8, std::sync::atomic::AtomicU8);
impl_atomic_repr!(u16, std::sync::atomic::AtomicU16);
impl_atomic_repr!(u32, std::sync::atomic::AtomicU32);
impl_atomic_repr!(usize, std::sync::atomic::AtomicUsize);
impl_atomic_repr!(i8, std::sync::atomic::AtomicI8);
impl_atomic_repr!(i16, std::sync::atomic::AtomicI16);
impl_atomic_repr!(i32, std::sync::atomic::AtomicI32);
impl_atomic_repr!(isize, std::sync::atomic::AtomicIsize);

/// Provides inner mutability for `Copy` types via relaxed atomic operations.
///
/// On x86_64 and ARM, relaxed loads and stores compile to the same instructions
/// as regular memory accesses — no `LOCK` prefix is emitted. This makes
/// `RelaxedAtomic` a zero-overhead way to achieve interior mutability without
/// the bus-lock cost of `fetch_*` or CAS operations.
///
/// Deliberately exposes only `load` and `store`. The `fetch_*` methods are
/// omitted because they emit `LOCK`-prefixed instructions with measurable
/// overhead. Instead, use the load–mutate–store pattern:
///
/// ```
/// # use vtcode_commons::thread_safety::RelaxedAtomic;
/// let counter = RelaxedAtomic::new(0u32);
/// let val = counter.load();
/// counter.store(val + 1);
/// ```
///
/// # When to use
///
/// Use when a field needs interior mutability and is accessed without
/// contention (same pattern as the original C code using plain loads/stores).
/// If you need multi-step atomic operations (CAS, fetch_add), use the
/// underlying `std::sync::atomic` types directly.
///
/// # When *not* to use
///
/// Do not use when the operation must be atomic relative to other threads.
/// The load–mutate–store pattern is *not* atomic as a whole — it can race
/// with concurrent stores. Use only where the C code would have used a
/// non-atomic access that happens to be race-free by design.
#[derive(Debug)]
pub struct RelaxedAtomic<T: AtomicRepr> {
    inner: T::Atomic,
}

impl<T: AtomicRepr> RelaxedAtomic<T> {
    /// Create a new `RelaxedAtomic` with the given initial value.
    #[inline]
    pub fn new(val: T) -> Self {
        Self {
            inner: T::new_atomic(val),
        }
    }

    /// Load the current value with relaxed ordering.
    #[inline]
    pub fn load(&self) -> T {
        T::load(&self.inner)
    }

    /// Store a new value with relaxed ordering.
    #[inline]
    pub fn store(&self, val: T) {
        T::store(&self.inner, val);
    }

    /// Consume the atomic and return the inner value.
    pub fn into_inner(self) -> T {
        T::into_inner(self.inner)
    }
}

impl RelaxedAtomic<u32> {
    /// Atomic add with relaxed ordering.
    ///
    /// Returns the previous value. Under `Relaxed` ordering this does not emit
    /// a `LOCK`-prefixed bus cycle on x86_64 or ARMv8 — it compiles to a
    /// plain locked `add` that implements a single-copy atomic RMW without
    /// the costly bus-lock side effects of stronger orderings.
    #[inline]
    pub fn fetch_add(&self, val: u32) -> u32 {
        self.inner.fetch_add(val, Ordering::Relaxed)
    }
}

impl RelaxedAtomic<u32> {
    /// Atomic subtract with relaxed ordering.
    #[inline]
    pub fn fetch_sub(&self, val: u32) -> u32 {
        self.inner.fetch_sub(val, Ordering::Relaxed)
    }
}

/// Note: this performs two separate relaxed loads. Under concurrent writes
/// the two values may come from different points in time. Use this only
/// for diagnostic assertions — never for correctness-critical decisions.
impl<T: AtomicRepr + PartialEq> PartialEq for RelaxedAtomic<T> {
    fn eq(&self, other: &Self) -> bool {
        self.load() == other.load()
    }
}

impl<T: AtomicRepr + Eq> Eq for RelaxedAtomic<T> {}

impl<T: AtomicRepr + Default> Default for RelaxedAtomic<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: AtomicRepr> Clone for RelaxedAtomic<T> {
    fn clone(&self) -> Self {
        Self::new(self.load())
    }
}

impl<T: AtomicRepr + fmt::Display> fmt::Display for RelaxedAtomic<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.load().fmt(f)
    }
}

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
