use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

pub(crate) const ZSH_EXEC_BRIDGE_WRAPPER_SOCKET_ENV_VAR: &str =
    "VTCODE_ZSH_EXEC_BRIDGE_WRAPPER_SOCKET";
pub(crate) const ZSH_EXEC_WRAPPER_MODE_ENV_VAR: &str = "VTCODE_ZSH_EXEC_WRAPPER_MODE";
pub(crate) const EXEC_WRAPPER_ENV_VAR: &str = "EXEC_WRAPPER";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WrapperExecRequest {
    request_id: String,
    file: String,
    argv: Vec<String>,
    cwd: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WrapperExecAction {
    Allow,
    Deny,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WrapperExecResponse {
    request_id: String,
    action: WrapperExecAction,
    reason: Option<String>,
}

#[cfg(unix)]
mod unix_impl {
    use super::{
        EXEC_WRAPPER_ENV_VAR, WrapperExecAction, WrapperExecRequest, WrapperExecResponse,
        ZSH_EXEC_BRIDGE_WRAPPER_SOCKET_ENV_VAR, ZSH_EXEC_WRAPPER_MODE_ENV_VAR,
    };
    use anyhow::{Context, Result, bail};
    use std::collections::HashMap;
    use parking_lot::Mutex;
    use std::fs;
    use std::io::{ErrorKind, Read, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use std::thread::{self, JoinHandle};
    use std::time::Duration;
    use tracing::warn;
    use uuid::Uuid;

    const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(20);

    pub struct ZshExecBridgeSession {
        socket_path: PathBuf,
        stop: Arc<AtomicBool>,
        worker: Mutex<Option<JoinHandle<()>>>,
    }

    impl ZshExecBridgeSession {
        pub fn spawn(allow_confirmed_dangerous: bool) -> Result<Self> {
            let socket_path = std::env::temp_dir().join(format!(
                "vtcode-zsh-exec-bridge-{}.sock",
                Uuid::new_v4()
            ));

            if socket_path.exists() {
                fs::remove_file(&socket_path).with_context(|| {
                    format!(
                        "remove pre-existing zsh bridge socket at {}",
                        socket_path.display()
                    )
                })?;
            }

            let listener = UnixListener::bind(&socket_path).with_context(|| {
                format!(
                    "bind zsh exec bridge socket listener at {}",
                    socket_path.display()
                )
            })?;
            listener
                .set_nonblocking(true)
                .context("set zsh exec bridge listener to nonblocking")?;

            let stop = Arc::new(AtomicBool::new(false));
            let stop_clone = Arc::clone(&stop);
            let cleanup_path = socket_path.clone();
            let worker = thread::Builder::new()
                .name("vtcode-zsh-exec-bridge".to_string())
                .spawn(move || {
                    run_bridge_loop(listener, stop_clone, allow_confirmed_dangerous);
                    let _ = fs::remove_file(&cleanup_path);
                })
                .context("spawn zsh exec bridge listener thread")?;

            Ok(Self {
                socket_path,
                stop,
                worker: Mutex::new(Some(worker)),
            })
        }

        pub(crate) fn env_vars(&self, wrapper_executable: &Path) -> HashMap<String, String> {
            HashMap::from([
                (
                    ZSH_EXEC_BRIDGE_WRAPPER_SOCKET_ENV_VAR.to_string(),
                    self.socket_path.to_string_lossy().to_string(),
                ),
                (ZSH_EXEC_WRAPPER_MODE_ENV_VAR.to_string(), "1".to_string()),
                (
                    EXEC_WRAPPER_ENV_VAR.to_string(),
                    wrapper_executable.to_string_lossy().to_string(),
                ),
            ])
        }
    }

    impl Drop for ZshExecBridgeSession {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(worker) = self.worker.lock().take()
                && worker.join().is_err()
            {
                warn!("zsh exec bridge worker thread panicked during cleanup");
            }
            let _ = fs::remove_file(&self.socket_path);
        }
    }

    fn run_bridge_loop(
        listener: UnixListener,
        stop: Arc<AtomicBool>,
        allow_confirmed_dangerous: bool,
    ) {
        while !stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    if let Err(err) = handle_wrapper_request(&mut stream, allow_confirmed_dangerous)
                    {
                        warn!(error = %err, "zsh exec bridge request failed");
                    }
                }
                Err(err) if err.kind() == ErrorKind::WouldBlock => {
                    thread::sleep(ACCEPT_POLL_INTERVAL);
                }
                Err(err) => {
                    warn!(error = %err, "zsh exec bridge listener failed");
                    break;
                }
            }
        }
    }

    fn handle_wrapper_request(
        stream: &mut UnixStream,
        allow_confirmed_dangerous: bool,
    ) -> Result<()> {
        let mut payload = String::new();
        stream
            .read_to_string(&mut payload)
            .context("read wrapper request payload")?;
        let request: WrapperExecRequest =
            serde_json::from_str(payload.trim()).context("parse wrapper request payload")?;

        let (action, reason) =
            evaluate_wrapper_exec_request(&request, allow_confirmed_dangerous);
        let response = WrapperExecResponse {
            request_id: request.request_id.clone(),
            action,
            reason,
        };
        let encoded = serde_json::to_string(&response).context("serialize wrapper response")?;
        stream
            .write_all(encoded.as_bytes())
            .context("write wrapper response payload")?;
        stream
            .write_all(b"\n")
            .context("write wrapper response newline")?;
        stream.flush().context("flush wrapper response")?;
        Ok(())
    }

    fn evaluate_wrapper_exec_request(
        request: &WrapperExecRequest,
        allow_confirmed_dangerous: bool,
    ) -> (WrapperExecAction, Option<String>) {
        let command = if request.argv.is_empty() {
            vec![request.file.clone()]
        } else {
            request.argv.clone()
        };

        if command.is_empty() {
            return (
                WrapperExecAction::Deny,
                Some("Rejected empty wrapped command".to_string()),
            );
        }

        if allow_confirmed_dangerous {
            return (WrapperExecAction::Allow, None);
        }

        let display = shell_words::join(command.iter().map(String::as_str));
        if let Err(err) = crate::tools::validation::commands::validate_command_safety(&display) {
            return (
                WrapperExecAction::Deny,
                Some(format!("Rejected by command safety validation: {err}")),
            );
        }
        if crate::command_safety::command_might_be_dangerous(&command) {
            return (
                WrapperExecAction::Deny,
                Some("Rejected dangerous subcommand".to_string()),
            );
        }

        (WrapperExecAction::Allow, None)
    }

    pub(crate) fn maybe_run_zsh_exec_wrapper_mode() -> Result<bool> {
        let wrapper_mode = std::env::var(ZSH_EXEC_WRAPPER_MODE_ENV_VAR).ok();
        if wrapper_mode.as_deref() != Some("1") {
            return Ok(false);
        }

        run_zsh_exec_wrapper_mode()?;
        Ok(true)
    }

    fn run_zsh_exec_wrapper_mode() -> Result<()> {
        let args: Vec<String> = std::env::args().collect();
        if args.len() < 2 {
            bail!("zsh exec wrapper mode requires target executable path");
        }

        let file = args[1].clone();
        let argv = if args.len() > 2 {
            args[2..].to_vec()
        } else {
            vec![file.clone()]
        };
        let cwd = std::env::current_dir()
            .context("resolve wrapper cwd")?
            .to_string_lossy()
            .to_string();
        let socket_path = std::env::var(ZSH_EXEC_BRIDGE_WRAPPER_SOCKET_ENV_VAR)
            .context("missing wrapper socket path env var")?;

        let request_id = Uuid::new_v4().to_string();
        let request = WrapperExecRequest {
            request_id: request_id.clone(),
            file: file.clone(),
            argv: argv.clone(),
            cwd,
        };

        let mut stream = UnixStream::connect(&socket_path)
            .with_context(|| format!("connect to wrapper socket at {socket_path}"))?;
        let encoded = serde_json::to_string(&request).context("serialize wrapper request")?;
        stream
            .write_all(encoded.as_bytes())
            .context("write wrapper request payload")?;
        stream
            .write_all(b"\n")
            .context("write wrapper request newline")?;
        stream
            .shutdown(std::net::Shutdown::Write)
            .context("shutdown wrapper request writer")?;

        let mut response_buf = String::new();
        stream
            .read_to_string(&mut response_buf)
            .context("read wrapper response payload")?;
        let response: WrapperExecResponse =
            serde_json::from_str(response_buf.trim()).context("parse wrapper response payload")?;

        if response.request_id != request_id {
            bail!(
                "wrapper response request_id mismatch: expected {request_id}, got {}",
                response.request_id
            );
        }

        if response.action == WrapperExecAction::Deny {
            if let Some(reason) = response.reason {
                warn!("zsh exec bridge denied execution: {reason}");
            } else {
                warn!("zsh exec bridge denied execution");
            }
            std::process::exit(1);
        }

        let mut command = std::process::Command::new(&file);
        if argv.len() > 1 {
            command.args(&argv[1..]);
        }
        command.env_remove(ZSH_EXEC_WRAPPER_MODE_ENV_VAR);
        command.env_remove(ZSH_EXEC_BRIDGE_WRAPPER_SOCKET_ENV_VAR);
        command.env_remove(EXEC_WRAPPER_ENV_VAR);
        let status = command.status().context("spawn wrapped executable")?;
        std::process::exit(status.code().unwrap_or(1));
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn request(command: &[&str]) -> WrapperExecRequest {
            let file = command.first().unwrap_or(&"/usr/bin/true").to_string();
            WrapperExecRequest {
                request_id: "test-request".to_string(),
                file: file.clone(),
                argv: command.iter().map(|s| s.to_string()).collect(),
                cwd: "/tmp".to_string(),
            }
        }

        #[test]
        fn evaluate_request_denies_dangerous_when_unconfirmed() {
            let request = request(&["rm", "-rf", "/tmp/demo"]);
            let (action, reason) = evaluate_wrapper_exec_request(&request, false);
            assert_eq!(action, WrapperExecAction::Deny);
            assert!(reason.is_some());
        }

        #[test]
        fn evaluate_request_allows_safe_when_unconfirmed() {
            let request = request(&["/usr/bin/true"]);
            let (action, reason) = evaluate_wrapper_exec_request(&request, false);
            assert_eq!(action, WrapperExecAction::Allow);
            assert!(reason.is_none());
        }

        #[test]
        fn evaluate_request_allows_dangerous_when_confirmed() {
            let request = request(&["rm", "-rf", "/tmp/demo"]);
            let (action, reason) = evaluate_wrapper_exec_request(&request, true);
            assert_eq!(action, WrapperExecAction::Allow);
            assert!(reason.is_none());
        }
    }
}

#[cfg(unix)]
pub use unix_impl::ZshExecBridgeSession;

#[cfg(unix)]
pub fn maybe_run_zsh_exec_wrapper_mode() -> Result<bool> {
    unix_impl::maybe_run_zsh_exec_wrapper_mode()
}

#[cfg(not(unix))]
pub struct ZshExecBridgeSession;

#[cfg(not(unix))]
impl ZshExecBridgeSession {
    pub fn spawn(_allow_confirmed_dangerous: bool) -> Result<Self> {
        Err(anyhow!(
            "zsh exec bridge is only supported on Unix platforms"
        ))
    }

    pub fn env_vars(&self, _wrapper_executable: &Path) -> HashMap<String, String> {
        HashMap::new()
    }
}

#[cfg(not(unix))]
pub fn maybe_run_zsh_exec_wrapper_mode() -> Result<bool> {
    Ok(false)
}
