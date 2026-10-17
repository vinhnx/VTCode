use anyhow::{Context, Result, anyhow};
use std::fs;
use std::path::{Path, PathBuf};

use super::{LAUNCHD_LABEL, SERVICE_NAME, atomic_write};
use vtcode_commons::VtCodePaths;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceManager {
    Launchd,
    SystemdUser,
}

impl ServiceManager {
    #[must_use]
    pub fn current() -> Option<Self> {
        #[cfg(target_os = "macos")]
        {
            return Some(Self::Launchd);
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            return Some(Self::SystemdUser);
        }
        #[cfg_attr(
            unix,
            allow(
                unreachable_code,
                reason = "Platform-specific returns make the fallback unreachable on Unix."
            )
        )]
        None
    }
}

#[derive(Debug, Clone)]
pub struct ServiceInstallPlan {
    pub manager: ServiceManager,
    pub path: PathBuf,
    pub contents: String,
}

pub fn render_service_install_plan(executable_path: &Path) -> Result<ServiceInstallPlan> {
    let manager = ServiceManager::current()
        .ok_or_else(|| anyhow!("Durable scheduler services are unsupported on this platform"))?;
    let path = match manager {
        ServiceManager::Launchd => dirs::home_dir()
            .ok_or_else(|| anyhow!("Failed to resolve home directory"))?
            .join("Library/LaunchAgents")
            .join(format!("{LAUNCHD_LABEL}.plist")),
        ServiceManager::SystemdUser => dirs::home_dir()
            .ok_or_else(|| anyhow!("Failed to resolve home directory"))?
            .join(".config/systemd/user")
            .join(format!("{SERVICE_NAME}.service")),
    };
    let contents = match manager {
        ServiceManager::Launchd => render_launchd_plist(executable_path),
        ServiceManager::SystemdUser => render_systemd_unit(executable_path),
    };
    Ok(ServiceInstallPlan { manager, path, contents })
}

pub fn install_service_file(executable_path: &Path) -> Result<ServiceInstallPlan> {
    let plan = render_service_install_plan(executable_path)?;
    if let Some(parent) = plan.path.parent() {
        VtCodePaths::ensure_user_dir(parent).with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    atomic_write(&plan.path, plan.contents.as_bytes())?;
    Ok(plan)
}

pub fn uninstall_service_file() -> Result<Option<(ServiceManager, PathBuf, bool)>> {
    let Some(manager) = ServiceManager::current() else {
        return Ok(None);
    };
    let path = render_service_install_plan(Path::new("/tmp/vtcode"))?.path;
    if path.exists() {
        fs::remove_file(&path).with_context(|| format!("Failed to remove {}", path.display()))?;
        return Ok(Some((manager, path, true)));
    }
    Ok(Some((manager, path, false)))
}

#[must_use]
pub fn render_launchd_plist(executable_path: &Path) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
  <dict>
    <key>Label</key>
    <string>{LAUNCHD_LABEL}</string>
    <key>ProgramArguments</key>
    <array>
      <string>{}</string>
      <string>schedule</string>
      <string>serve</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
  </dict>
</plist>
"#,
        xml_escape(executable_path.display().to_string())
    )
}

#[must_use]
pub fn render_systemd_unit(executable_path: &Path) -> String {
    format!(
        "[Unit]\nDescription=VT Code scheduler\n\n[Service]\nType=simple\nExecStart={} schedule serve\nRestart=always\nRestartSec=5\n\n[Install]\nWantedBy=default.target\n",
        shell_words::quote(executable_path.to_string_lossy().as_ref())
    )
}

fn xml_escape(value: String) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
