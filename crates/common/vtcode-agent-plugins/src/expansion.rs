use std::path::{Path, PathBuf};

/// Single-pass, non-recursive placeholder expansion.
///
/// Replaces every exact occurrence of `${PLUGIN_ROOT}` and `${PLUGIN_DATA}`
/// without rescanning replacement text, per Agent Plugins §9.2. Unrecognized
/// placeholder-like text is left literal and no other expansion is performed.
/// Expansion applies only to `args`, `env` values, and `cwd`; callers must not
/// apply it to `command`, `env` keys, URLs, or headers.
pub fn expand_placeholders(value: &str, plugin_root: &Path, plugin_data: &Path) -> String {
    let root = plugin_root.display().to_string();
    let data = plugin_data.display().to_string();
    let mut out = String::with_capacity(value.len() + root.len());
    let mut idx = 0;
    while idx < value.len() {
        let Some(rest) = value.get(idx..) else {
            break;
        };
        if rest.starts_with("${PLUGIN_ROOT}") {
            out.push_str(&root);
            idx += "${PLUGIN_ROOT}".len();
        } else if rest.starts_with("${PLUGIN_DATA}") {
            out.push_str(&data);
            idx += "${PLUGIN_DATA}".len();
        } else {
            // Advance by one char to preserve UTF-8 boundaries.
            let ch = rest.chars().next().unwrap_or_default();
            out.push(ch);
            idx += ch.len_utf8();
        }
    }
    out
}

/// Returns true for the two reserved env names that a plugin must not set.
/// Such an entry makes the server configuration invalid (§9.2); the client
/// supplies them last instead of overlaying.
pub fn is_reserved_env_key(key: &str) -> bool {
    // Platform env-name semantics differ (Windows is case-insensitive); the
    // portable check rejects the exact reserved spellings. Callers on Windows
    // should compare case-insensitively before invoking.
    key == "PLUGIN_ROOT" || key == "PLUGIN_DATA"
}

/// Shell-control bytes that must never appear in a stdio `command` token.
/// Spawn is argv-based (no shell), so these are inert rather than exploitable;
/// rejecting them keeps both command branches consistent and prevents a shell
/// command string from slipping through as one token.
const SHELL_CONTROL_CHARS: &[char] = &[';', '|', '&', '>', '<', '`', '\n', '\r', '\0'];

fn contains_shell_control(value: &str) -> bool {
    value.chars().any(|c| SHELL_CONTROL_CHARS.contains(&c))
}

/// Validate a stdio `command` as a single executable token.
///
/// Accepts either a bare executable name (no `/`, `\`, or whitespace, resolved
/// via platform search) or a plugin-relative `./` path (resolved against the
/// plugin root; spaces allowed since spawn is argv-based). Rejects empty
/// values, bare directory tokens (`./`, `./.`), NUL/newlines, and
/// shell-control characters so a shell command string can never slip through
/// as one token (Agent Plugins §7.2.1: single token, bare or `./`).
pub fn validate_command_token(command: &str) -> Result<(), String> {
    if command.is_empty() {
        return Err("command must not be empty".into());
    }
    if command == "./" || command == "./." {
        return Err("command must name an executable, not a directory".into());
    }
    if contains_shell_control(command) {
        return Err("command must be a single executable token, not a shell string".into());
    }
    if command.starts_with("./") {
        Ok(())
    } else {
        if command.contains('/') || command.contains('\\') {
            return Err("command with a path separator must start with ./".into());
        }
        if command.chars().any(|c| c.is_whitespace()) {
            return Err("command must be a single executable token, not a shell string".into());
        }
        Ok(())
    }
}

/// Validate the syntactic form of an explicit `cwd` (§7.2.1).
///
/// Must be `./`-relative, exactly `${PLUGIN_ROOT}` (or prefixed with
/// `${PLUGIN_ROOT}/`), or exactly `${PLUGIN_DATA}` (or prefixed with
/// `${PLUGIN_DATA}/`). Any other form makes the server entry invalid.
pub fn validate_cwd_form(cwd: &str) -> Result<(), String> {
    if cwd.starts_with("./")
        || cwd == "${PLUGIN_ROOT}"
        || cwd.starts_with("${PLUGIN_ROOT}/")
        || cwd == "${PLUGIN_DATA}"
        || cwd.starts_with("${PLUGIN_DATA}/")
    {
        Ok(())
    } else {
        Err(format!("cwd has unsupported form: {cwd}"))
    }
}

/// Client-managed persistent data directory for one installed plugin instance.
///
/// Uses `VtCodePaths::data_dir()/plugin-data/<name>` so state survives plugin
/// updates (§9.1). Fallbacks (in order): platform data dir
/// (`dirs::data_dir()/vtcode/plugin-data/<name>`), then the process temp dir.
/// Callers must create it before spawn and preserve it across updates; it may
/// be deleted on uninstall. Plugin names are charset-constrained
/// (`manifest.rs` rejects `/`, `\`, `..`), so `join` cannot escape.
pub fn default_plugin_data_dir(plugin_name: &str) -> PathBuf {
    if let Ok(paths) = vtcode_commons::VtCodePaths::resolve() {
        return paths.data_dir().join("plugin-data").join(plugin_name);
    }
    if let Some(data_dir) = dirs::data_dir() {
        return data_dir.join("vtcode").join("plugin-data").join(plugin_name);
    }
    std::env::temp_dir().join("vtcode-plugin-data").join(plugin_name)
}

/// Resolve an explicit or default `cwd` after placeholder expansion and
/// enforce post-resolution containment.
///
/// * `None` → canonical plugin root (spec default).
/// * `./...` or `${PLUGIN_ROOT}[/...]` → must stay within the canonical
///   plugin root.
/// * `${PLUGIN_DATA}[/...]` → must stay within the canonical plugin data dir.
pub fn resolve_cwd(cwd: Option<&str>, plugin_root: &Path, plugin_data: &Path) -> Result<PathBuf, crate::PluginError> {
    let canonical_root = vtcode_commons::canonicalize(plugin_root)
        .map_err(|e| crate::PluginError::PathEscape(format!("plugin root could not be resolved: {e}")))?;
    // PLUGIN_DATA may not exist yet; canonicalize ancestors when missing so
    // the containment check still applies to the existing prefix.
    let canonical_data = canonicalize_allow_missing(plugin_data).unwrap_or_else(|_| plugin_data.to_path_buf());

    let Some(raw) = cwd else {
        return Ok(canonical_root);
    };
    validate_cwd_form(raw).map_err(crate::PluginError::PathEscape)?;

    let expanded = expand_placeholders(raw, plugin_root, plugin_data);
    if raw.starts_with("./") {
        // `./` form may itself contain placeholders; after expansion it either
        // remains `./`-relative (validate normally) or became absolute via an
        // injected placeholder (check containment directly).
        if expanded.starts_with("./") {
            return validate_plugin_relative(&expanded, plugin_root);
        }
        let candidate = PathBuf::from(&expanded);
        let resolved = ensure_within(&canonical_root, &candidate)
            .map_err(|e| crate::PluginError::PathEscape(format!("cwd escapes plugin root: {e}")))?;
        return Ok(resolved);
    }
    if raw == "${PLUGIN_ROOT}" || raw.starts_with("${PLUGIN_ROOT}/") {
        let candidate = PathBuf::from(&expanded);
        let resolved = ensure_within(&canonical_root, &candidate)
            .map_err(|e| crate::PluginError::PathEscape(format!("cwd escapes plugin root: {e}")))?;
        return Ok(resolved);
    }
    // Remaining valid form is ${PLUGIN_DATA}[/...].
    let candidate = PathBuf::from(&expanded);
    let resolved = ensure_within(&canonical_data, &candidate)
        .map_err(|e| crate::PluginError::PathEscape(format!("cwd escapes plugin data dir: {e}")))?;
    Ok(resolved)
}

pub(crate) fn canonicalize_allow_missing(path: &Path) -> std::io::Result<PathBuf> {
    if let Ok(canonical) = vtcode_commons::canonicalize(path) {
        return Ok(canonical);
    }
    // Walk to the deepest existing ancestor, canonicalize it, re-append suffix.
    // Any `..` in the missing suffix is rejected: re-appended `..` would make
    // `starts_with` checks lexically true while escaping at runtime.
    let mut probe = path;
    let mut suffix: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if let Ok(existing) = vtcode_commons::canonicalize(probe) {
            for part in suffix.iter().rev() {
                if *part == ".." {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("path escapes its root: {}", path.display()),
                    ));
                }
            }
            let mut resolved = existing;
            for part in suffix.iter().rev() {
                resolved.push(part);
            }
            return Ok(resolved);
        }
        let Some(name) = probe.file_name() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("path could not be resolved: {}", path.display()),
            ));
        };
        suffix.push(name.to_os_string());
        let Some(parent) = probe.parent() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("path could not be resolved: {}", path.display()),
            ));
        };
        probe = parent;
    }
}

pub(crate) fn ensure_within(canonical_root: &Path, candidate: &Path) -> Result<PathBuf, String> {
    let resolved = canonicalize_allow_missing(candidate).map_err(|e| format!("{} ({e})", candidate.display()))?;
    // A surviving `..` means the tail could not be normalized; fail closed
    // since lexical `starts_with` cannot be trusted in that state.
    if resolved.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(format!("{} escapes {}", candidate.display(), canonical_root.display()));
    }
    if resolved.starts_with(canonical_root) {
        Ok(resolved)
    } else {
        Err(format!("{} escapes {}", candidate.display(), canonical_root.display()))
    }
}

/// Validate that a plugin-relative path stays inside the plugin root.
///
/// Uses `canonicalize` (which resolves symlinks) so a symlink inside the
/// plugin pointing outside (e.g. `bin/link -> /etc`) cannot smuggle a path out
/// of the sandbox. The candidate must start with `./` per the Agent Plugins
/// spec.
///
/// The target need not exist yet (a plugin may spawn a binary built at
/// runtime), so when it is absent we canonicalize the deepest *existing*
/// ancestor and verify the remaining suffix stays lexically inside the
/// canonicalized root.
pub fn validate_plugin_relative(value: &str, root: &Path) -> Result<PathBuf, crate::PluginError> {
    if !value.starts_with("./") {
        return Err(crate::PluginError::PathEscape(format!("path must start with ./: {}", value)));
    }

    let canonical_root = vtcode_commons::canonicalize(root)
        .map_err(|e| crate::PluginError::PathEscape(format!("plugin root could not be resolved: {e}")))?;

    let candidate = root.join(value.get(2..).unwrap_or_default());

    // Reject obvious lexical escapes before touching the filesystem.
    let lexical_ok = candidate
        .components()
        .all(|component| !matches!(component, std::path::Component::ParentDir));
    if !lexical_ok {
        return Err(crate::PluginError::PathEscape(format!("path escapes plugin root: {}", value)));
    }

    // Walk up to the deepest existing ancestor and canonicalize it; the
    // remaining suffix is appended lexically.
    let mut probe = candidate.as_path();
    let mut suffix: Vec<PathBuf> = Vec::new();
    loop {
        match vtcode_commons::canonicalize(probe) {
            Ok(existing) => {
                let mut resolved = existing;
                for part in suffix.iter().rev() {
                    resolved.push(part);
                }
                if !resolved.starts_with(&canonical_root) {
                    return Err(crate::PluginError::PathEscape(format!("path escapes plugin root: {}", value)));
                }
                return Ok(resolved);
            }
            Err(_) => {
                let Some(name) = probe.file_name() else {
                    return Err(crate::PluginError::PathEscape(format!("path could not be resolved: {}", value)));
                };
                suffix.push(PathBuf::from(name));
                let Some(parent) = probe.parent() else {
                    return Err(crate::PluginError::PathEscape(format!("path could not be resolved: {}", value)));
                };
                probe = parent;
            }
        }
    }
}
