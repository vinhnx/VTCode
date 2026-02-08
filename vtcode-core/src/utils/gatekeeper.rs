use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use once_cell::sync::OnceCell;
use tracing::{debug, warn};

use crate::config::GatekeeperConfig;

#[derive(Debug, Clone)]
pub struct GatekeeperPolicy {
    warn_on_quarantine: bool,
    auto_clear_quarantine: bool,
    auto_clear_paths: Vec<PathBuf>,
    cache: Arc<Mutex<HashMap<PathBuf, GatekeeperCacheEntry>>>,
}

#[derive(Debug, Clone)]
struct GatekeeperCacheEntry {
    quarantined: bool,
    warned: bool,
}

static GATEKEEPER_POLICY: OnceCell<GatekeeperPolicy> = OnceCell::new();

pub fn initialize_gatekeeper(config: &GatekeeperConfig, workspace_root: Option<&Path>) {
    let policy = GatekeeperPolicy::from_config(config, workspace_root);
    let _ = GATEKEEPER_POLICY.set(policy);
}

impl GatekeeperPolicy {
    fn from_config(config: &GatekeeperConfig, workspace_root: Option<&Path>) -> Self {
        let auto_clear_paths = config
            .auto_clear_paths
            .iter()
            .filter_map(|raw| resolve_path(raw, workspace_root))
            .collect();

        Self {
            warn_on_quarantine: config.warn_on_quarantine,
            auto_clear_quarantine: config.auto_clear_quarantine,
            auto_clear_paths,
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn should_auto_clear(&self, target: &Path) -> bool {
        self.auto_clear_paths
            .iter()
            .any(|base| target.starts_with(base))
    }

    fn cache_entry(&self, path: &Path) -> Option<GatekeeperCacheEntry> {
        self.cache
            .lock()
            .ok()
            .and_then(|cache| cache.get(path).cloned())
    }

    fn update_cache(&self, path: PathBuf, entry: GatekeeperCacheEntry) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(path, entry);
        }
    }
}

pub fn check_quarantine_for_program(program: &str) {
    if program.contains(std::path::MAIN_SEPARATOR) || program.contains('/') {
        check_quarantine(Path::new(program));
    }
}

pub fn check_quarantine(path: &Path) {
    let Some(policy) = GATEKEEPER_POLICY.get() else {
        return;
    };

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (policy, path);
        return;
    }

    #[cfg(target_os = "macos")]
    {
        if !path.exists() {
            return;
        }

        let canonical = match path.canonicalize() {
            Ok(canonical) => canonical,
            Err(err) => {
                warn!(path = %path.display(), error = %err, "Failed to canonicalize path");
                return;
            }
        };

        if let Some(entry) = policy.cache_entry(&canonical) {
            if !entry.quarantined {
                return;
            }
            if entry.warned || !policy.warn_on_quarantine {
                return;
            }
        }

        match read_quarantine_xattr(&canonical) {
            Ok(Some(_)) => {
                let should_auto_clear =
                    policy.auto_clear_quarantine && policy.should_auto_clear(&canonical);

                if policy.warn_on_quarantine {
                    warn!(
                        path = %canonical.display(),
                        auto_clear = should_auto_clear,
                        "Gatekeeper quarantine detected for executable"
                    );
                }

                let warned = policy.warn_on_quarantine;

                if should_auto_clear {
                    match clear_quarantine_xattr(&canonical) {
                        Ok(()) => {
                            debug!(
                                path = %canonical.display(),
                                "Cleared Gatekeeper quarantine attribute"
                            );
                            policy.update_cache(
                                canonical,
                                GatekeeperCacheEntry {
                                    quarantined: false,
                                    warned: false,
                                },
                            );
                            return;
                        }
                        Err(err) => {
                            warn!(
                                path = %canonical.display(),
                                error = %err,
                                "Failed to clear Gatekeeper quarantine"
                            );
                        }
                    }
                }

                policy.update_cache(
                    canonical,
                    GatekeeperCacheEntry {
                        quarantined: true,
                        warned,
                    },
                );
            }
            Ok(None) => {
                policy.update_cache(
                    canonical,
                    GatekeeperCacheEntry {
                        quarantined: false,
                        warned: false,
                    },
                );
            }
            Err(err) => {
                debug!(
                    path = %canonical.display(),
                    error = %err,
                    "Gatekeeper check failed"
                );
            }
        }
    }
}

fn resolve_path(raw: &str, workspace_root: Option<&Path>) -> Option<PathBuf> {
    if raw.trim().is_empty() {
        return None;
    }

    let expanded = if raw.starts_with("~/") {
        dirs::home_dir().map(|home| home.join(raw.trim_start_matches("~/")))
    } else {
        Some(PathBuf::from(raw))
    }?;

    if expanded.is_absolute() {
        Some(expanded)
    } else {
        let relative = expanded.clone();
        workspace_root
            .map(|root| root.join(expanded))
            .or_else(|| std::env::current_dir().ok().map(|cwd| cwd.join(relative)))
    }
}

#[cfg(target_os = "macos")]
fn read_quarantine_xattr(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes())?;
    let name = CString::new("com.apple.quarantine")?;

    unsafe {
        let size = libc::getxattr(
            c_path.as_ptr(),
            name.as_ptr(),
            std::ptr::null_mut(),
            0,
            0,
            0,
        );
        if size < 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::ENOATTR) {
                return Ok(None);
            }
            return Err(err);
        }
        let mut buffer = vec![0u8; size as usize];
        let read = libc::getxattr(
            c_path.as_ptr(),
            name.as_ptr(),
            buffer.as_mut_ptr() as *mut _,
            buffer.len(),
            0,
            0,
        );
        if read < 0 {
            return Err(std::io::Error::last_os_error());
        }
        buffer.truncate(read as usize);
        Ok(Some(buffer))
    }
}

#[cfg(target_os = "macos")]
fn clear_quarantine_xattr(path: &Path) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes())?;
    let name = CString::new("com.apple.quarantine")?;

    unsafe {
        let result = libc::removexattr(c_path.as_ptr(), name.as_ptr(), 0);
        if result != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_clear_resolves_workspace_paths() {
        let config = GatekeeperConfig {
            warn_on_quarantine: true,
            auto_clear_quarantine: true,
            auto_clear_paths: vec![".vtcode/bin".to_string()],
        };
        let policy = GatekeeperPolicy::from_config(&config, Some(Path::new("/tmp/workspace")));

        let target = Path::new("/tmp/workspace/.vtcode/bin/tool");
        assert!(policy.should_auto_clear(target));
    }
}
