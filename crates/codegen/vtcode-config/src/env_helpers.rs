//! Shared environment variable and serde-default helpers.
//!
//! Consolidates the `read_env_var` + test-override pattern and the trivial
//! `default_true` / `default_enabled` serde default functions that were
//! previously duplicated across the crate.

/// Read an environment variable, honoring the test-only override map.
pub(crate) fn read_env_var(name: &str) -> Option<String> {
    #[cfg(test)]
    if let Some(override_value) = test_env_overrides::get(name) {
        return override_value;
    }
    std::env::var(name).ok()
}

/// Parse a boolean environment variable, returning `default` when unset or
/// unrecognized. Accepted truthy values: `1`, `true`, `yes`, `on`. Accepted
/// falsy values: `0`, `false`, `no`, `off`. Matching is case-insensitive after
/// trimming whitespace.
pub(crate) fn parse_env_bool(name: &str, default: bool) -> bool {
    read_env_var(name)
        .and_then(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            match normalized.as_str() {
                "1" | "true" | "yes" | "on" => Some(true),
                "0" | "false" | "no" | "off" => Some(false),
                _ => None,
            }
        })
        .unwrap_or(default)
}

/// Serde default returning `true`. Use with `#[serde(default = "default_true")]`.
pub(crate) const fn default_true() -> bool {
    true
}

/// Serde default returning `true`. Alias of [`default_true`] for use with
/// `#[serde(default = "default_enabled")]`.
pub(crate) const fn default_enabled() -> bool {
    true
}

#[cfg(test)]
pub(crate) mod test_env_overrides {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    static ENV_OVERRIDES: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();

    fn overrides() -> &'static Mutex<HashMap<String, Option<String>>> {
        ENV_OVERRIDES.get_or_init(|| Mutex::new(HashMap::new()))
    }

    pub(crate) fn get(name: &str) -> Option<Option<String>> {
        overrides().lock().expect("env overrides lock poisoned").get(name).cloned()
    }

    pub(crate) fn set(name: &str, value: Option<&str>) {
        overrides()
            .lock()
            .expect("env overrides lock poisoned")
            .insert(name.to_string(), value.map(ToOwned::to_owned));
    }

    pub(crate) fn restore(name: &str, previous: Option<Option<String>>) {
        let mut guard = overrides().lock().expect("env overrides lock poisoned");
        match previous {
            Some(value) => {
                guard.insert(name.to_string(), value);
            }
            None => {
                guard.remove(name);
            }
        }
    }
}
