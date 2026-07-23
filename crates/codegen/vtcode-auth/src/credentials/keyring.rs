//! OS keyring integration — entry creation, liveness probe, and disable detection.
//!
//! All keyring access goes through [`entry`] so that [`is_disabled`] and
//! [`ensure_native_store`] are always applied consistently.

/// Check if the OS keyring is functional by attempting a test operation.
///
/// On macOS, this skips the write/read test — every Keychain operation triggers
/// an authorization popup. Instead, it just checks that the keyring crate can
/// create an Entry handle (which does not touch the keychain yet). The first
/// real `set_password`/`get_password` call will prompt the user once per binary,
/// which is acceptable since this check is only reached when the user has
/// explicitly opted into keyring via `Auto` or `Keyring` mode.
///
/// On other platforms, this creates a test entry, verifies it can be written
/// and read, then deletes it. This is more reliable than just checking if
/// Entry creation succeeds.
///
/// The result is cached after the first call so that repeated checks (e.g. from
/// `Auto` mode resolution) do not trigger additional OS keyring round trips.
pub(crate) fn is_functional() -> bool {
    use std::sync::OnceLock;

    static FUNCTIONAL: OnceLock<bool> = OnceLock::new();

    *FUNCTIONAL.get_or_init(|| {
        #[cfg(target_os = "macos")]
        {
            keyring_core::Entry::new("vtcode", "_probe").is_ok()
        }

        #[cfg(not(target_os = "macos"))]
        {
            let test_user = format!("test_{}", std::process::id());
            let entry = match entry("vtcode", &test_user) {
                Ok(e) => e,
                Err(_) => return false,
            };

            if entry.set_password("test").is_err() {
                return false;
            }

            let functional = entry.get_password().is_ok();

            let _ = entry.delete_credential();

            functional
        }
    })
}

/// Returns `true` when access to the OS keyring should be skipped.
///
/// The native keyring (e.g. macOS Keychain) prompts the user for authorization
/// the first time each distinct binary touches it. Debug and test binaries are
/// recompiled with a new code signature on every build, so they would prompt on
/// every run. To avoid this, keyring access is disabled in debug builds, during
/// tests, and whenever the `VTCODE_DISABLE_KEYRING` or `CI` environment
/// variables are set. Callers fall back to encrypted-file storage in that case.
///
/// The result is cached after the first call so that repeated checks (e.g. from
/// [`entry`]) do not re-read environment variables on every invocation.
pub(crate) fn is_disabled() -> bool {
    use std::sync::OnceLock;

    static DISABLED: OnceLock<bool> = OnceLock::new();

    *DISABLED.get_or_init(|| {
        if cfg!(debug_assertions) {
            if let Ok(value) = std::env::var("VTCODE_DISABLE_KEYRING") {
                if matches!(value.trim().to_ascii_lowercase().as_str(), "" | "0" | "false" | "no" | "off") {
                    return false;
                }
            }
            return true;
        }

        if cfg!(test) {
            return true;
        }

        if let Ok(value) = std::env::var("VTCODE_DISABLE_KEYRING") {
            return !matches!(value.trim().to_ascii_lowercase().as_str(), "" | "0" | "false" | "no" | "off");
        }

        std::env::var_os("CI").is_some()
    })
}

/// Create a keyring entry for the given service and user, respecting the
/// disabled check and ensuring a native store is configured.
pub(crate) fn entry(service: &str, user: &str) -> keyring_core::Result<keyring_core::Entry> {
    if is_disabled() {
        return Err(keyring_core::Error::NotSupportedByStore(
            "VT Code keyring access is disabled (test run or VTCODE_DISABLE_KEYRING/CI set)".to_string(),
        ));
    }

    if keyring_core::get_default_store().is_none() {
        ensure_native_store()?;
    }

    keyring_core::Entry::new(service, user)
}

fn ensure_native_store() -> keyring_core::Result<()> {
    if keyring_core::get_default_store().is_some() {
        return Ok(());
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    let store = dbus_secret_service_keyring_store::Store::new_with_configuration(&std::collections::HashMap::new())?;

    #[cfg(target_os = "macos")]
    let store = apple_native_keyring_store::keychain::Store::new_with_configuration(&std::collections::HashMap::new())?;

    #[cfg(target_os = "windows")]
    let store = windows_native_keyring_store::Store::new_with_configuration(&std::collections::HashMap::new())?;

    #[cfg(not(any(
        target_os = "linux",
        target_os = "freebsd",
        target_os = "macos",
        target_os = "windows"
    )))]
    {
        return Err(keyring_core::Error::NotSupportedByStore(
            "VT Code does not have a native keyring store configured for this platform".to_string(),
        ));
    }

    keyring_core::set_default_store(store);
    Ok(())
}
