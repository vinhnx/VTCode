use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc::SyncSender;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, BufReader, BufWriter};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tokio::time::timeout;
use vtcode_desktop_protocol as protocol;
use vtcode_desktop_protocol::{InteractionRequest, RequestId, ThreadEventNotification};

const CLIENT_QUEUE_CAPACITY: usize = 128;
const MAX_PENDING_REQUESTS: usize = 128;
const STDERR_TAIL_BYTES: usize = 16 * 1024;
pub(super) const UI_EVENT_CAPACITY: usize = 256;
const BACKEND_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug)]
pub(crate) enum BackendCommand {
    Request { method: String, params: Value },
    Stop,
}

#[derive(Debug)]
pub(crate) enum BackendEvent {
    Response {
        method: String,
        result: Option<Value>,
        error: Option<String>,
    },
    ThreadEvent(ThreadEventNotification),
    Interaction(InteractionRequest),
    TurnStatus(Value),
    Exited {
        status: Option<i32>,
        authentication_required: bool,
    },
    LaunchFailed(String),
}

#[derive(Clone)]
pub(crate) struct BackendHandle {
    pub commands: mpsc::Sender<BackendCommand>,
}

pub(crate) fn launch_backend(workspace: PathBuf, ui_events: SyncSender<BackendEvent>) -> Result<BackendHandle, String> {
    let executable = resolve_backend_executable()?;
    let (commands, command_rx) = mpsc::channel(CLIENT_QUEUE_CAPACITY);

    let _transport_thread = std::thread::Builder::new()
        .name("vtcode-desktop-transport".to_string())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(_) => {
                    let _ = send_ui_event(
                        &ui_events,
                        BackendEvent::LaunchFailed(
                            "The VT Code desktop service could not initialize its runtime.".to_string(),
                        ),
                    );
                    return;
                }
            };
            runtime.block_on(run_client(executable, workspace, command_rx, ui_events));
        })
        .map_err(|_error| "Could not start the VT Code desktop transport thread.".to_string())?;

    Ok(BackendHandle { commands })
}

async fn run_client(
    executable: PathBuf,
    workspace: PathBuf,
    mut commands: mpsc::Receiver<BackendCommand>,
    ui_events: SyncSender<BackendEvent>,
) {
    let mut child = match spawn_backend(&executable, &workspace).await {
        Ok(child) => child,
        Err(error) => {
            let _ = send_ui_event(&ui_events, BackendEvent::LaunchFailed(error));
            return;
        }
    };
    let Some(stdout) = child.stdout.take() else {
        let _ = send_ui_event(
            &ui_events,
            BackendEvent::LaunchFailed("The VT Code service did not open its protocol output.".to_string()),
        );
        return;
    };
    let Some(stdin) = child.stdin.take() else {
        let _ = send_ui_event(
            &ui_events,
            BackendEvent::LaunchFailed("The VT Code service did not open its protocol input.".to_string()),
        );
        return;
    };
    let stderr_capture = child.stderr.take().map(|mut stderr| {
        tokio::spawn(async move {
            let mut buffer = [0_u8; 2048];
            let mut tail = Vec::with_capacity(STDERR_TAIL_BYTES);
            loop {
                match stderr.read(&mut buffer).await {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        let Some(chunk) = buffer.get(..read) else {
                            break;
                        };
                        tail.extend_from_slice(chunk);
                        let overflow = tail.len().saturating_sub(STDERR_TAIL_BYTES);
                        if overflow > 0 {
                            for _discarded_byte in tail.drain(..overflow) {}
                        }
                    }
                }
            }
            missing_credentials_diagnostic(&tail)
        })
    });

    let mut reader = BufReader::new(stdout);
    let mut writer = BufWriter::new(stdin);
    let mut next_request_id = 1_i64;
    let mut pending = HashMap::<RequestId, String>::new();
    let mut stopping = false;

    loop {
        tokio::select! {
            command = commands.recv(), if !stopping => {
                match command {
                    Some(BackendCommand::Request { method, params }) => {
                        if pending.len() >= MAX_PENDING_REQUESTS {
                            if !send_ui_event(
                                &ui_events,
                                BackendEvent::Response {
                                    method,
                                    result: None,
                                    error: Some("The desktop service request backlog is full.".to_string()),
                                },
                            ) {
                                break;
                            }
                            continue;
                        }
                        let id = RequestId::Number(next_request_id);
                        let Some(next_id) = next_request_id.checked_add(1) else {
                            let _ = send_ui_event(&ui_events, BackendEvent::LaunchFailed(
                                "Desktop request identity space was exhausted.".to_string(),
                            ));
                            break;
                        };
                        next_request_id = next_id;
                        let _replaced = pending.insert(id.clone(), method.clone());
                        let request = json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "method": method,
                            "params": params,
                        });
                        if protocol::write_frame(&mut writer, &request).await.is_err() {
                            let _ = send_ui_event(&ui_events, BackendEvent::LaunchFailed(
                                "The VT Code service stopped accepting requests.".to_string(),
                            ));
                            break;
                        }
                    }
                    Some(BackendCommand::Stop) | None => {
                        stopping = true;
                        let shutdown = json!({
                            "jsonrpc": "2.0",
                            "id": next_request_id,
                            "method": protocol::method::SHUTDOWN,
                            "params": {},
                        });
                        let _shutdown_send_result = protocol::write_frame(&mut writer, &shutdown).await;
                    }
                }
            }
            frame = protocol::read_frame(&mut reader) => {
                match frame {
                    Ok(Some(frame)) => {
                        match decode_frame(&frame, &mut pending) {
                            Ok(Some(event)) => {
                                if !send_ui_event(&ui_events, event) {
                                    break;
                                }
                            }
                            Ok(None) => {}
                            Err(_) => {
                                let _ = send_ui_event(&ui_events, BackendEvent::LaunchFailed(
                                    "The VT Code service sent a malformed protocol message.".to_string(),
                                ));
                                break;
                            }
                        }
                    }
                    Ok(None) => break,
                    Err(_) => {
                        let _ = send_ui_event(&ui_events, BackendEvent::LaunchFailed(
                            "The VT Code service sent an invalid or oversized protocol frame.".to_string(),
                        ));
                        break;
                    }
                }
            }
        }

        if stopping {
            let status = timeout(BACKEND_SHUTDOWN_TIMEOUT, child.wait()).await;
            if !matches!(status, Ok(Ok(_))) {
                let _kill_result = child.start_kill();
            }
            break;
        }
    }

    let status = match timeout(Duration::from_millis(250), child.wait()).await {
        Ok(Ok(status)) => status.code(),
        _ => {
            let _kill_result = child.start_kill();
            let _wait_for_exit = timeout(Duration::from_secs(2), child.wait()).await;
            None
        }
    };
    let authentication_required = match stderr_capture {
        Some(capture) => timeout(Duration::from_secs(1), capture)
            .await
            .unwrap_or(Ok(false))
            .unwrap_or(false),
        None => false,
    };
    let _ = send_ui_event(&ui_events, BackendEvent::Exited { status, authentication_required });
}

async fn spawn_backend(executable: &Path, workspace: &Path) -> Result<Child, String> {
    Command::new(executable)
        .arg("desktop-server")
        .arg("--workspace")
        .arg(workspace)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_error| {
            "The local VT Code service could not start. Build VT Code or reinstall the desktop app, then try again."
                .to_string()
        })
}

fn resolve_backend_executable() -> Result<PathBuf, String> {
    let current = std::env::current_exe()
        .map_err(|_error| "Could not locate the VT Code Desktop application binary.".to_string())?;
    let current_dir = current
        .parent()
        .ok_or_else(|| "The app binary has no parent directory.".to_string())?;
    let candidates = [
        current_dir.join("../Helpers/vtcode"),
        current_dir.join("vtcode"),
        manifest_workspace_root().join("target/release/vtcode"),
        manifest_workspace_root().join("target/debug/vtcode"),
    ];

    candidates
        .into_iter()
        .map(|candidate| vtcode_commons::paths::canonicalize(&candidate).unwrap_or(candidate))
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| {
            "The matching VT Code backend was not found. Run `cargo build --locked -p vtcode -p vtcode-desktop` or package the app again."
                .to_string()
        })
}

fn manifest_workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..").join(".")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ProtocolDecodeError;

fn decode_frame(
    frame: &[u8],
    pending: &mut HashMap<RequestId, String>,
) -> Result<Option<BackendEvent>, ProtocolDecodeError> {
    let value = serde_json::from_slice::<Value>(frame).map_err(|_error| ProtocolDecodeError)?;
    if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(ProtocolDecodeError);
    }
    if let Some(method) = value.get("method").and_then(Value::as_str) {
        let params = value.get("params").cloned().unwrap_or(Value::Null);
        return match method {
            protocol::method::THREAD_EVENT => serde_json::from_value::<ThreadEventNotification>(params)
                .map(BackendEvent::ThreadEvent)
                .map(Some)
                .map_err(|_error| ProtocolDecodeError),
            protocol::method::INTERACTION_REQUESTED => serde_json::from_value::<InteractionRequest>(params)
                .map(BackendEvent::Interaction)
                .map(Some)
                .map_err(|_error| ProtocolDecodeError),
            protocol::method::TURN_STATUS => Ok(Some(BackendEvent::TurnStatus(params))),
            _ => Ok(None),
        };
    }

    let id_value = value.get("id").ok_or(ProtocolDecodeError)?.clone();
    let id = serde_json::from_value::<RequestId>(id_value).map_err(|_error| ProtocolDecodeError)?;
    let method = pending.remove(&id).ok_or(ProtocolDecodeError)?;
    if let Some(error) = value.get("error") {
        let safe_message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("The VT Code service rejected the request.");
        let message = vtcode_commons::sanitizer::sanitize_provider_diagnostic(safe_message)
            .chars()
            .filter(|character| !character.is_control())
            .take(240)
            .collect();
        Ok(Some(BackendEvent::Response { method, result: None, error: Some(message) }))
    } else {
        let result = value.get("result").cloned().ok_or(ProtocolDecodeError)?;
        Ok(Some(BackendEvent::Response { method, result: Some(result), error: None }))
    }
}

fn send_ui_event(sender: &SyncSender<BackendEvent>, event: BackendEvent) -> bool {
    sender.send(event).is_ok()
}

fn missing_credentials_diagnostic(stderr: &[u8]) -> bool {
    let diagnostic = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    [
        "authentication not found",
        "api key not found",
        "missing api key",
        "no credentials found",
        "no authenticated fallback provider is available",
    ]
    .iter()
    .any(|marker| diagnostic.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_responses_are_correlated_with_the_request_method() {
        let mut pending = HashMap::from([(RequestId::Number(9), "session/list".to_string())]);
        let event = decode_frame(br#"{"jsonrpc":"2.0","id":9,"result":[]}"#, &mut pending)
            .expect("valid response")
            .expect("decoded response");
        assert!(
            matches!(event, BackendEvent::Response { method, result: Some(_), error: None } if method == "session/list")
        );
        assert!(pending.is_empty());
    }

    #[test]
    fn protocol_notifications_keep_authoritative_event_payloads() {
        let mut pending = HashMap::new();
        let frame = br#"{"jsonrpc":"2.0","method":"turn/status","params":{"status":"running","turnId":"t-1"}}"#;
        let event = decode_frame(frame, &mut pending)
            .expect("valid notification")
            .expect("decoded notification");
        assert!(matches!(event, BackendEvent::TurnStatus(value) if value["turnId"] == "t-1"));
    }

    #[test]
    fn malformed_protocol_frames_fail_closed_and_unknown_notifications_are_ignored() {
        let mut pending = HashMap::new();
        assert!(matches!(decode_frame(b"not-json", &mut pending), Err(ProtocolDecodeError)));
        assert!(matches!(
            decode_frame(br#"{"jsonrpc":"2.0","method":"unknown/event","params":{}}"#, &mut pending),
            Ok(None)
        ));
    }

    #[test]
    fn startup_auth_diagnostics_are_classified_without_exposing_raw_stderr() {
        assert!(missing_credentials_diagnostic(b"Authentication not found for OpenAI. Run `vtcode login openai`."));
        assert!(missing_credentials_diagnostic(b"API key not found for provider 'example'."));
        assert!(!missing_credentials_diagnostic(b"Connection refused while contacting the configured endpoint."));
    }
}
