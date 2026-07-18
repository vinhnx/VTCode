//! Process-environment mutation lock.
//!
//! Rust 2024 marks [`std::env::set_var`] and [`std::env::remove_var`] as
//! `unsafe` because POSIX `setenv`/`getenv` are not thread-safe. Each call site
//! that needs to mutate the environment previously rolled its own
//! `OnceLock<Mutex<()>>` plus duplicated `set_env_var` / `remove_env_var`
//! helpers carrying their own `SAFETY:` comments. This module consolidates that
//! invariant into a single sound wrapper.
//!
//! # Usage
//!
//! Acquire the guard once, then mutate the environment through its methods.
//! The guard holds a process-wide [`Mutex`] so concurrent callers serialize
//! automatically.
//!
//! ```no_run
//! use vtcode_commons::env_lock;
//!
//! let env = env_lock::lock();
//! env.set_var("MY_TEST_VAR", "1");
//! // ... run code that reads MY_TEST_VAR ...
//! env.remove_var("MY_TEST_VAR");
//! ```
//!
//! All in-process code that mutates the environment **must** go through this
//! module; direct `std::env::set_var` / `std::env::remove_var` calls bypass the
//! lock and re-introduce the data race the wrapper is here to prevent.

use std::ffi::OsStr;
use std::sync::{Mutex, MutexGuard, OnceLock};

static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn raw_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// RAII guard that proves ownership of the process-wide environment lock.
///
/// Obtain one with [`lock`]; while it is alive, no other thread can enter the
/// safe mutation methods on this type. Dropping the guard releases the lock.
#[must_use = "EnvGuard releases the lock when dropped; bind it to a local"]
pub struct EnvGuard(#[allow(dead_code)] MutexGuard<'static, ()>);

impl EnvGuard {
    /// Set a process environment variable.
    ///
    /// Safe because `self` proves the global env mutex is held, so no other
    /// caller routed through this module is reading or writing the environment
    /// concurrently.
    #[expect(unsafe_code, reason = "guard serializes all env mutators, so no concurrent access")]
    pub fn set_var(&self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) {
        // SAFETY: `self` is the unique holder of the process-wide env mutex
        // for the duration of this call; all set/remove calls in this module
        // route through the same mutex, so no concurrent env access occurs.
        unsafe {
            std::env::set_var(key, value);
        }
    }

    /// Remove a process environment variable.
    ///
    /// See [`Self::set_var`] for the safety argument.
    #[expect(unsafe_code, reason = "see set_var — guard serializes all env mutators")]
    pub fn remove_var(&self, key: impl AsRef<OsStr>) {
        // SAFETY: see `set_var` — the guard serializes all mutators.
        unsafe {
            std::env::remove_var(key);
        }
    }

    /// Restore a variable to its previous value, or remove it if there was none.
    ///
    /// Pairs with [`std::env::var_os`] snapshots taken before a mutation.
    pub fn restore_var<T: AsRef<OsStr>>(&self, key: &str, previous: Option<T>) {
        match previous {
            Some(value) => self.set_var(key, value),
            None => self.remove_var(key),
        }
    }
}

/// Acquire the process-wide environment lock.
///
/// Blocks until no other [`EnvGuard`] is alive. Poisoning is ignored — the
/// inner `()` cannot become corrupted, so callers can safely recover.
pub fn lock() -> EnvGuard {
    EnvGuard(raw_lock())
}

/// One-shot safe wrapper around [`std::env::set_var`].
///
/// Acquires the process-wide environment lock for the duration of the call.
/// Use [`lock`] when you need to perform multiple env operations atomically.
pub fn set_var(key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) {
    lock().set_var(key, value);
}

/// One-shot safe wrapper around [`std::env::remove_var`].
///
/// Acquires the process-wide environment lock for the duration of the call.
/// Use [`lock`] when you need to perform multiple env operations atomically.
pub fn remove_var(key: impl AsRef<OsStr>) {
    lock().remove_var(key);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_and_remove_roundtrip() {
        let env = lock();
        env.set_var("VTCODE_ENV_LOCK_TEST", "value-a");
        assert_eq!(std::env::var("VTCODE_ENV_LOCK_TEST").as_deref(), Ok("value-a"));
        env.remove_var("VTCODE_ENV_LOCK_TEST");
        assert!(std::env::var("VTCODE_ENV_LOCK_TEST").is_err());
    }

    #[test]
    fn restore_var_restores_previous_value() {
        let env = lock();
        env.set_var("VTCODE_ENV_LOCK_RESTORE", "original");
        let previous = std::env::var_os("VTCODE_ENV_LOCK_RESTORE");
        env.set_var("VTCODE_ENV_LOCK_RESTORE", "temporary");
        env.restore_var("VTCODE_ENV_LOCK_RESTORE", previous);
        assert_eq!(std::env::var("VTCODE_ENV_LOCK_RESTORE").as_deref(), Ok("original"));
        env.remove_var("VTCODE_ENV_LOCK_RESTORE");
    }

    #[test]
    fn restore_var_removes_when_previous_is_none() {
        let env = lock();
        env.set_var("VTCODE_ENV_LOCK_RESTORE_NONE", "temporary");
        env.restore_var::<&str>("VTCODE_ENV_LOCK_RESTORE_NONE", None);
        assert!(std::env::var("VTCODE_ENV_LOCK_RESTORE_NONE").is_err());
    }
}
