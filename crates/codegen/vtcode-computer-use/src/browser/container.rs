//! Approved-container lifecycle for the isolated Chromium runner.
//!
//! The caller approves the image and the static build. The launcher enforces
//! pinning, directory shape, and mount-spec safety, starts the container with
//! a fixed hardening flag set over `argv` (never a shell), and tears down the
//! exact container it started.

use std::{ffi::OsString, path::Path, time::Duration};

use anyhow::{Context, Result, bail, ensure};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

const DOCKER_TIMEOUT: Duration = Duration::from_secs(30);
const AVAILABILITY_TIMEOUT: Duration = Duration::from_secs(10);
const IDENTIFY_TIMEOUT: Duration = Duration::from_secs(15);
const IDENTIFY_INTERVAL: Duration = Duration::from_millis(200);
const CONTAINER_NAME_PREFIX: &str = "vtcode-cu-";
const CONTAINER_ID_LEN: usize = 64;
const DIAGNOSTIC_MAX_BYTES: usize = 512;

/// A validated image and build pair for one approved container.
pub struct ContainerSpec {
    image: String,
    build_dir: String,
}

impl ContainerSpec {
    /// Approve an image and build directory for container launch.
    ///
    /// The image must carry an explicit tag or digest other than `latest`.
    /// The build directory must exist, resolve to a real directory, and be
    /// representable in a `--mount` spec.
    ///
    /// # Errors
    ///
    /// Returns an error for an unpinned image, a missing or non-directory
    /// build path, or a path that cannot appear in a mount spec.
    pub async fn approved(image: &str, build_dir: &Path) -> Result<Self> {
        ensure!(!image.is_empty() && image.len() <= 512, "invalid browser container image");
        ensure!(image.chars().all(|c| !c.is_control() && !c.is_whitespace()), "invalid browser container image");
        ensure!(
            (image.contains(':') || image.contains('@')) && !image.ends_with(":latest"),
            "browser container image must be pinned"
        );
        let canonical = vtcode_commons::fs::canonicalize_with_context_async(build_dir, "browser build").await?;
        ensure!(canonical.is_dir(), "browser build is not a directory");
        let Some(dir) = canonical.to_str() else {
            bail!("browser build path is not representable");
        };
        ensure!(mount_spec_safe(dir), "browser build path cannot appear in a mount spec");
        Ok(Self { image: image.to_owned(), build_dir: dir.to_owned() })
    }
}

/// Handle to the exact container started for an approved spec. Dropping the
/// handle kills the attached client; use [`ContainerHandle::terminate`] to
/// deterministically remove the exact container.
pub struct ContainerHandle {
    name: String,
    id: String,
    #[allow(dead_code, reason = "Owned for its kill-on-drop backstop; never read directly.")]
    child: Child,
}

impl ContainerHandle {
    /// Start the approved container and return its pipes for the driver.
    ///
    /// The tuple order matches [`crate::browser::BrowserDriver::connect`]:
    /// the handle, the reply stream, then the command stream.
    ///
    /// # Errors
    ///
    /// Returns an error if `docker` is unavailable, the container cannot be
    /// started, or its identity cannot be established.
    pub async fn launch(spec: &ContainerSpec) -> Result<(Self, ChildStdout, ChildStdin)> {
        let name = format!("{CONTAINER_NAME_PREFIX}{}", uuid::Uuid::new_v4().as_simple());
        let mut child = Command::new("docker")
            .args(docker_run_argv(spec, &name))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .context("could not start the approved browser container")?;
        let replies = child.stdout.take().context("browser container did not pipe its replies")?;
        let commands = child.stdin.take().context("browser container did not pipe its commands")?;
        let id = inspect_container_id(&name, &mut child).await?;
        Ok((Self { name, id, child }, replies, commands))
    }

    /// The unique container name assigned at launch.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The exact container identity established at launch.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Remove the exact container, succeeding when it is already gone.
    /// Call this on Stop, failure, and teardown paths; dropping the handle
    /// only stops the attached client.
    ///
    /// # Errors
    ///
    /// Returns an error if `docker` is unavailable, the removal times out,
    /// or the container cannot be removed.
    pub async fn terminate(&self) -> Result<()> {
        let output = run_docker(&[
            OsString::from("rm"),
            OsString::from("--force"),
            OsString::from(&self.id),
        ])
        .await?;
        if !output.status.success() {
            let diagnostic = diagnostic_text(&output.stderr);
            ensure!(
                diagnostic.to_ascii_lowercase().contains("no such container"),
                "could not remove browser container {}: {diagnostic}",
                self.id
            );
        }
        Ok(())
    }
}

/// Whether a Docker daemon answers. Callers use this to skip container-backed
/// checks (and to refuse the browser target) without failing the run.
pub async fn docker_available() -> bool {
    let Ok(mut child) = Command::new("docker")
        .arg("info")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
    else {
        return false;
    };
    tokio::time::timeout(AVAILABILITY_TIMEOUT, child.wait())
        .await
        .is_ok_and(|status| status.is_ok_and(|status| status.success()))
}

/// Whether a directory can appear unquoted in a `--mount` spec: no separator,
/// quoting, or control characters. Fail-closed on anything ambiguous.
fn mount_spec_safe(dir: &str) -> bool {
    !dir.is_empty() && !dir.chars().any(|c| c.is_control() || matches!(c, ',' | '"' | '\\'))
}

fn docker_run_argv(spec: &ContainerSpec, name: &str) -> Vec<OsString> {
    [
        "run",
        "--rm",
        "--interactive",
        "--init",
        "--pull=never",
        "--network=none",
        "--read-only",
        "--cap-drop=ALL",
        "--security-opt=no-new-privileges",
        "--pids-limit=256",
        "--memory=1g",
        "--tmpfs=/tmp:rw,nosuid,nodev,size=512m",
        "--env=HOME=/tmp",
    ]
    .into_iter()
    .map(OsString::from)
    .chain([
        OsString::from(format!("--name={name}")),
        OsString::from(format!("--mount=type=bind,src={},dst=/build,readonly", spec.build_dir)),
        OsString::from(&spec.image),
    ])
    .collect()
}

async fn inspect_container_id(name: &str, child: &mut Child) -> Result<String> {
    let start = tokio::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().context("could not poll the browser container")? {
            bail!(
                "browser container exited before its identity could be established: {status}: {}",
                drain_stderr(child).await
            );
        }
        let output = run_docker(&[
            OsString::from("inspect"),
            OsString::from("--format={{.Id}}"),
            OsString::from(name),
        ])
        .await?;
        if output.status.success() {
            if let Ok(id) = parse_container_id(&String::from_utf8_lossy(&output.stdout)) {
                return Ok(id);
            }
        }
        ensure!(start.elapsed() < IDENTIFY_TIMEOUT, "could not identify the browser container");
        tokio::time::sleep(IDENTIFY_INTERVAL).await;
    }
}

async fn drain_stderr(child: &mut Child) -> String {
    use tokio::io::AsyncReadExt;
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        // Best effort: partial stderr is still useful diagnostics.
        let _drained = tokio::time::timeout(Duration::from_secs(2), pipe.read_to_string(&mut stderr)).await;
    }
    diagnostic_text(stderr.as_bytes())
}

fn parse_container_id(raw: &str) -> Result<String> {
    let id = raw.trim().to_owned();
    ensure!(
        id.len() == CONTAINER_ID_LEN && id.chars().all(|c| c.is_ascii_hexdigit()),
        "could not identify the browser container"
    );
    Ok(id)
}

async fn run_docker(args: &[OsString]) -> Result<std::process::Output> {
    let child = Command::new("docker")
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("could not run docker")?;
    // Dropping the timed-out future drops the child, and `kill_on_drop`
    // stops the docker client it was waiting on.
    match tokio::time::timeout(DOCKER_TIMEOUT, child.wait_with_output()).await {
        Ok(output) => output.context("could not read docker output"),
        Err(_) => bail!("docker command timed out"),
    }
}

fn diagnostic_text(output: &[u8]) -> String {
    let text = String::from_utf8_lossy(output);
    let end = text.floor_char_boundary(DIAGNOSTIC_MAX_BYTES.min(text.len()));
    text.get(..end).unwrap_or_default().to_owned()
}

#[cfg(test)]
mod tests;
