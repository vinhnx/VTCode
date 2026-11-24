//! Poison-tolerant locking for `std::sync::Mutex`.
//!
//! `std::sync::Mutex` poisoning is advisory bookkeeping: a guard dropped while
//! panicking flags the mutex, but the guarded value is often still consistent.
//! Panicking on every subsequent `lock()` turns one localized failure into a
//! process-wide cascade, especially for process-global state and cleanup paths.
//!
//! VT Code's policy:
//! - short synchronous critical sections use `parking_lot::Mutex` (no poison);
//! - where `std::sync::Mutex` is required, use [`lock_or_recover`] so poison is
//!   recovered instead of propagated;
//! - callers with a genuine mid-critical-section invariant should model poison
//!   explicitly rather than relying on the standard library's boolean flag.

use std::sync::{Mutex, MutexGuard, PoisonError};

/// Lock `mutex`, returning the guard even if a previous holder panicked.
///
/// Accepts `&Mutex<T>` and, through deref coercion, `&Arc<Mutex<T>>`. On stable
/// Rust this is the non-panicking alternative to `mutex.lock().unwrap()`;
/// panic-freedom matters most in globals, shutdown, and panic-hook paths where
/// a second panic aborts the process.
#[inline]
pub fn lock_or_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::lock_or_recover;
    use std::sync::{Arc, Mutex};

    #[test]
    fn lock_or_recover_returns_the_guard_for_a_healthy_mutex() {
        let mutex = Mutex::new(7);
        assert_eq!(*lock_or_recover(&mutex), 7);
        assert!(!mutex.is_poisoned());
    }

    #[test]
    fn lock_or_recover_recovers_and_allows_mutation_after_poison() {
        let mutex = Arc::new(Mutex::new(vec![1, 2, 3]));
        let poisoner = Arc::clone(&mutex);
        let result = std::thread::spawn(move || {
            let _guard = poisoner.lock().expect("poisoner acquires the lock");
            panic!("deliberate panic while holding the lock");
        })
        .join();
        assert!(result.is_err(), "the helper thread must panic while locked");
        assert!(mutex.is_poisoned());

        {
            let mut guard = lock_or_recover(&mutex);
            guard.push(4);
        }
        assert_eq!(*lock_or_recover(&mutex), vec![1, 2, 3, 4]);
    }
}
