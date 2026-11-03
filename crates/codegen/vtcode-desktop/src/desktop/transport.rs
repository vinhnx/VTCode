use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, BufReader, BufWriter};
use tokio::process::{Child, Command};
use tokio::sync::{Notify, mpsc};
use tokio::time::timeout;
use vtcode_desktop_protocol as protocol;
use vtcode_desktop_protocol::{InteractionRequest, RequestId, ThreadEventNotification};

use super::strings::{bundle, keys};

const CLIENT_QUEUE_CAPACITY: usize = 128;
const MAX_PENDING_REQUESTS: usize = 128;
const STDERR_TAIL_BYTES: usize = 16 * 1024;
pub(super) const UI_EVENT_CAPACITY: usize = 256;
const BACKEND_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(8);
const BACKEND_SUBCOMMAND: &str = "desktop-server";
const BACKEND_WORKSPACE_ARG: &str = "--workspace";
const BACKEND_BINARY_NAME: &str = "vtcode";
const DEV_RELEASE_BINARY: &str = "target/release/vtcode";
const DEV_DEBUG_BINARY: &str = "target/debug/vtcode";

#[derive(Debug)]
pub(crate) enum BackendCommand {
    Request { method: String, params: Value },
    Stop,
}

#[derive(Debug)]
pub(crate) enum BackendEvent {
    WorkspaceReviewResponse {
        request_id: String,
        result: Option<Value>,
        error: Option<String>,
    },
    Response {
        method: String,
        result: Option<Value>,
        error: Option<String>,
    },
    SessionHistoryResponse {
        request_id: String,
        result: Option<Value>,
        error: Option<String>,
    },
    SessionOutputResponse {
        request_id: String,
        result: Option<Value>,
        error: Option<String>,
    },
    McpAppResponse {
        request_id: String,
        result: Option<Value>,
        error: Option<String>,
    },
    InteractionResponded {
        identity: InteractionResponseIdentity,
        error: Option<String>,
    },
    TurnBlockedByStartupInteraction {
        session_id: String,
    },
    ThreadEvent(ThreadEventNotification),
    PrimaryAgentChanged(protocol::PrimaryAgentChanged),
    Interaction(InteractionRequest),
    TurnStatus(Value),
    TurnQueueStatus(Value),
    UiProgress(Value),
    Exited {
        status: Option<i32>,
        authentication_required: bool,
    },
    LaunchFailed(String),
}

#[derive(Debug)]
pub(crate) struct InteractionResponseIdentity {
    pub session_id: String,
    pub turn_id: String,
    pub interaction_id: String,
    pub cancelled: bool,
}

#[derive(Debug)]
struct PendingRequest {
    method: String,
    workspace_review_request_id: Option<String>,
    interaction: Option<InteractionResponseIdentity>,
    session_history_request_id: Option<String>,
    session_output_request_id: Option<String>,
    mcp_app_request_id: Option<String>,
    turn_session_id: Option<String>,
}

impl PendingRequest {
    fn new(method: String, params: &Value) -> Self {
        let interaction = (method == protocol::method::INTERACTION_RESPOND)
            .then(|| {
                Some(InteractionResponseIdentity {
                    session_id: params.get(keys::SESSION_ID)?.as_str()?.to_owned(),
                    turn_id: params.get(keys::TURN_ID)?.as_str()?.to_owned(),
                    interaction_id: params.get(keys::INTERACTION_ID)?.as_str()?.to_owned(),
                    cancelled: params.get(keys::CANCELLED)?.as_bool()?,
                })
            })
            .flatten();
        let session_history_request_id = (method == protocol::method::SESSION_HISTORY)
            .then(|| params.get(keys::REQUEST_ID).and_then(Value::as_str).map(str::to_owned))
            .flatten();
        let session_output_request_id = (method == protocol::method::SESSION_OUTPUT_READ)
            .then(|| params.get(keys::REQUEST_ID).and_then(Value::as_str).map(str::to_owned))
            .flatten();
        let mcp_app_request_id = (method == protocol::method::MCP_APP_READ)
            .then(|| params.get(keys::REQUEST_ID).and_then(Value::as_str).map(str::to_owned))
            .flatten();
        let turn_session_id = (method == protocol::method::TURN_SUBMIT)
            .then(|| params.get(keys::SESSION_ID).and_then(Value::as_str).map(str::to_owned))
            .flatten();
        Self {
            workspace_review_request_id: (method == protocol::method::WORKSPACE_REVIEW)
                .then(|| params.get(keys::REQUEST_ID).and_then(Value::as_str).map(str::to_owned))
                .flatten(),
            method,
            interaction,
            session_history_request_id,
            session_output_request_id,
            mcp_app_request_id,
            turn_session_id,
        }
    }

    fn finish(self, result: Option<Value>, error: Option<String>) -> BackendEvent {
        if let Some(request_id) = self.workspace_review_request_id {
            return BackendEvent::WorkspaceReviewResponse { request_id, result, error };
        }
        if let Some(request_id) = self.session_history_request_id {
            return BackendEvent::SessionHistoryResponse { request_id, result, error };
        }
        if let Some(request_id) = self.session_output_request_id {
            return BackendEvent::SessionOutputResponse { request_id, result, error };
        }
        if let Some(request_id) = self.mcp_app_request_id {
            return BackendEvent::McpAppResponse { request_id, result, error };
        }
        match self.interaction {
            Some(identity) => BackendEvent::InteractionResponded { identity, error },
            None => BackendEvent::Response { method: self.method, result, error },
        }
    }
}

#[derive(Clone)]
pub(crate) struct BackendHandle {
    pub commands: mpsc::Sender<BackendCommand>,
}

#[derive(Clone)]
pub(crate) struct BackendEventSender {
    sender: SyncSender<BackendEvent>,
    wake: Arc<Notify>,
}

impl BackendEventSender {
    pub(crate) fn channel(capacity: usize) -> (Self, Receiver<BackendEvent>) {
        let (sender, receiver) = sync_channel(capacity);
        (Self { sender, wake: Arc::default() }, receiver)
    }

    pub(crate) fn send(&self, event: BackendEvent) -> Result<(), Box<std::sync::mpsc::SendError<BackendEvent>>> {
        self.sender.send(event).map_err(Box::new)?;
        self.wake.notify_one();
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn try_send(&self, event: BackendEvent) -> Result<(), Box<std::sync::mpsc::TrySendError<BackendEvent>>> {
        self.sender.try_send(event).map_err(Box::new)?;
        self.wake.notify_one();
        Ok(())
    }

    pub(crate) async fn notified(&self) {
        self.wake.notified().await;
    }

    pub(crate) fn wake(&self) {
        self.wake.notify_one();
    }
}

/// Include JSON escaping and the largest issued request ID in the wire-size check.
pub(super) fn request_fits_frame(method: &str, params: &Value) -> bool {
    let request =
        json!({ keys::JSONRPC: keys::JSONRPC_VERSION, keys::ID: i64::MAX, keys::METHOD: method, keys::PARAMS: params });
    serde_json::to_vec(&request).is_ok_and(|bytes| bytes.len() <= protocol::MAX_FRAME_BYTES)
}

pub(crate) fn launch_backend(workspace: PathBuf, ui_events: BackendEventSender) -> Result<BackendHandle, String> {
    let executable = resolve_backend_executable()?;
    let (commands, command_rx) = mpsc::channel(CLIENT_QUEUE_CAPACITY);

    let _transport_thread = std::thread::Builder::new()
        .name("vtcode-desktop-transport".to_string())
        .spawn(move || {
            // The transport pumps one framed stdio conversation plus a stderr
            // drain: purely sequential I/O with no blocking or CPU work, so a
            // single-threaded runtime on this dedicated thread is sufficient
            // and avoids parking worker threads.
            let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(runtime) => runtime,
                Err(_) => {
                    let _ = send_ui_event(
                        &ui_events,
                        BackendEvent::LaunchFailed(rust_i18n::t!("app.service.runtime_init").to_string()),
                    );
                    return;
                }
            };
            runtime.block_on(run_client(executable, workspace, command_rx, ui_events));
        })
        .map_err(|_error| rust_i18n::t!("app.service.transport_thread").to_string())?;

    Ok(BackendHandle { commands })
}

async fn run_client(
    executable: PathBuf,
    workspace: PathBuf,
    mut commands: mpsc::Receiver<BackendCommand>,
    ui_events: BackendEventSender,
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
            BackendEvent::LaunchFailed(rust_i18n::t!("app.service.no_proto_out").to_string()),
        );
        return;
    };
    let Some(stdin) = child.stdin.take() else {
        let _ =
            send_ui_event(&ui_events, BackendEvent::LaunchFailed(rust_i18n::t!("app.service.no_proto_in").to_string()));
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
    let mut pending = HashMap::<RequestId, PendingRequest>::new();
    let mut stopping = false;

    loop {
        tokio::select! {
            command = commands.recv(), if !stopping => {
                match command {
                    Some(BackendCommand::Request { method, params }) => {
                        let request_context = PendingRequest::new(method.clone(), &params);
                        if pending.len() >= MAX_PENDING_REQUESTS {
                            if !send_ui_event(
                                &ui_events,
                                request_context.finish(None, Some(rust_i18n::t!("app.service.backlog_full").to_string())),
                            ) {
                                break;
                            }
                            continue;
                        }
                        let id = RequestId::Number(next_request_id);
                        let Some(next_id) = next_request_id.checked_add(1) else {
                            let _ = send_ui_event(&ui_events, BackendEvent::LaunchFailed(
                                rust_i18n::t!("app.service.id_exhausted").to_string(),
                            ));
                            break;
                        };
                        next_request_id = next_id;
                        let _replaced = pending.insert(id.clone(), request_context);
                        let request = json!({
                            keys::JSONRPC: keys::JSONRPC_VERSION,
                            keys::ID: id,
                            keys::METHOD: method,
                            keys::PARAMS: params,
                        });
                        if protocol::write_frame(&mut writer, &request).await.is_err() {
                            let _ = send_ui_event(&ui_events, BackendEvent::LaunchFailed(
                                rust_i18n::t!("app.service.not_accepting").to_string(),
                            ));
                            break;
                        }
                    }
                    Some(BackendCommand::Stop) | None => {
                        stopping = true;
                        let shutdown = json!({
                            keys::JSONRPC: keys::JSONRPC_VERSION,
                            keys::ID: next_request_id,
                            keys::METHOD: protocol::method::SHUTDOWN,
                            keys::PARAMS: {},
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
                                    rust_i18n::t!("app.service.malformed").to_string(),
                                ));
                                break;
                            }
                        }
                    }
                    Ok(None) => break,
                    Err(_) => {
                        let _ = send_ui_event(&ui_events, BackendEvent::LaunchFailed(
                            rust_i18n::t!("app.service.bad_frame").to_string(),
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
        .arg(BACKEND_SUBCOMMAND)
        .arg(BACKEND_WORKSPACE_ARG)
        .arg(workspace)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_error| rust_i18n::t!("app.service.cannot_start").to_string())
}

fn resolve_backend_executable() -> Result<PathBuf, String> {
    let current = std::env::current_exe().map_err(|_error| rust_i18n::t!("app.service.no_binary").to_string())?;
    let current_dir = current
        .parent()
        .ok_or_else(|| rust_i18n::t!("app.service.no_parent").to_string())?;
    let (candidates, missing_message) = match packaged_backend_candidates(current_dir) {
        Some(candidates) => (candidates, rust_i18n::t!("app.service.missing_helper").to_string()),
        None => (
            backend_development_candidates(current_dir, &manifest_workspace_root()),
            rust_i18n::t!("app.service.missing_dev_backend").to_string(),
        ),
    };

    candidates
        .into_iter()
        .map(|candidate| vtcode_commons::paths::canonicalize(&candidate).unwrap_or(candidate))
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| missing_message.to_string())
}

/// Binary for CLI handoff scripts: the same resolved backend executable, so a
/// GUI-spawned terminal (minimal PATH, no user profile) runs the
/// version-matched binary instead of failing to find `vtcode` on PATH.
pub(crate) fn handoff_vtcode_binary() -> Result<PathBuf, String> {
    #[cfg(test)]
    if let Some(overridden) = TEST_HANDOFF_BINARY.with(|slot| slot.borrow().clone()) {
        return overridden;
    }
    resolve_backend_executable()
}

#[cfg(test)]
std::thread_local! {
    static TEST_HANDOFF_BINARY: std::cell::RefCell<Option<Result<PathBuf, String>>> =
        const { std::cell::RefCell::new(None) };
}

/// Redirect handoff binary resolution for one GUI test: `Ok` pins the script
/// binary, `Err` simulates resolution failure (copy+open fallback). Hold the
/// returned guard for the test's duration; dropping it restores real resolution.
#[cfg(test)]
#[must_use = "hold the guard or the override leaks into other tests"]
pub(super) fn set_test_handoff_binary(binary: Result<PathBuf, String>) -> TestHandoffBinaryGuard {
    TEST_HANDOFF_BINARY.with(|slot| *slot.borrow_mut() = Some(binary));
    TestHandoffBinaryGuard { _private: () }
}

/// RAII guard restoring real binary resolution on drop, even on failure.
#[cfg(test)]
pub(super) struct TestHandoffBinaryGuard {
    _private: (),
}

#[cfg(test)]
impl Drop for TestHandoffBinaryGuard {
    fn drop(&mut self) {
        TEST_HANDOFF_BINARY.with(|slot| *slot.borrow_mut() = None);
    }
}

fn packaged_backend_candidates(current_dir: &Path) -> Option<Vec<PathBuf>> {
    if current_dir.file_name() != Some(std::ffi::OsStr::new(bundle::MACOS_DIR)) {
        return None;
    }
    let contents_dir = current_dir.parent()?;
    if contents_dir.file_name() != Some(std::ffi::OsStr::new(bundle::CONTENTS_DIR))
        || contents_dir.parent()?.extension() != Some(std::ffi::OsStr::new(bundle::APP_EXTENSION))
    {
        return None;
    }
    Some(vec![contents_dir.join(bundle::HELPERS_VTCODE)])
}

fn backend_development_candidates(current_dir: &Path, workspace_root: &Path) -> Vec<PathBuf> {
    let mut candidates = vec![current_dir.join(BACKEND_BINARY_NAME)];
    for candidate in [
        workspace_root.join(DEV_RELEASE_BINARY),
        workspace_root.join(DEV_DEBUG_BINARY),
    ] {
        if !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    }
    candidates
}

fn manifest_workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..").join(".")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ProtocolDecodeError;

fn decode_frame(
    frame: &[u8],
    pending: &mut HashMap<RequestId, PendingRequest>,
) -> Result<Option<BackendEvent>, ProtocolDecodeError> {
    let mut value = serde_json::from_slice::<Value>(frame).map_err(|_error| ProtocolDecodeError)?;
    if value.get(keys::JSONRPC).and_then(Value::as_str) != Some(keys::JSONRPC_VERSION) {
        return Err(ProtocolDecodeError);
    }
    // Move the payload out of the parsed frame instead of cloning it: thread
    // events can carry full items with large text and output bodies on every
    // streaming tick. Only the short method name is cloned, to end the borrow
    // before the payload moves.
    if let Some(method) = value.get(keys::METHOD).and_then(Value::as_str).map(str::to_owned) {
        let params = value.get_mut(keys::PARAMS).map(Value::take).unwrap_or(Value::Null);
        return match method.as_str() {
            protocol::method::THREAD_EVENT => serde_json::from_value::<ThreadEventNotification>(params)
                .map(BackendEvent::ThreadEvent)
                .map(Some)
                .map_err(|_error| ProtocolDecodeError),
            protocol::method::INTERACTION_REQUESTED => serde_json::from_value::<InteractionRequest>(params)
                .map(BackendEvent::Interaction)
                .map(Some)
                .map_err(|_error| ProtocolDecodeError),
            protocol::method::PRIMARY_AGENT_CHANGED => serde_json::from_value::<protocol::PrimaryAgentChanged>(params)
                .map(BackendEvent::PrimaryAgentChanged)
                .map(Some)
                .map_err(|_error| ProtocolDecodeError),
            protocol::method::TURN_STATUS => Ok(Some(BackendEvent::TurnStatus(params))),
            protocol::method::TURN_QUEUE_STATUS => Ok(Some(BackendEvent::TurnQueueStatus(params))),
            protocol::method::UI_PROGRESS => Ok(Some(BackendEvent::UiProgress(params))),
            _ => Ok(None),
        };
    }

    let id_value = value.get_mut(keys::ID).map(Value::take).ok_or(ProtocolDecodeError)?;
    let id = serde_json::from_value::<RequestId>(id_value).map_err(|_error| ProtocolDecodeError)?;
    let request = pending.remove(&id).ok_or(ProtocolDecodeError)?;
    if let Some(error) = value.get(keys::ERROR) {
        if request.method == protocol::method::TURN_SUBMIT
            && error.get(keys::CODE).and_then(Value::as_i64)
                == Some(i64::from(protocol::STARTUP_INTERACTION_PENDING_ERROR))
        {
            if let Some(session_id) = request.turn_session_id.clone() {
                return Ok(Some(BackendEvent::TurnBlockedByStartupInteraction { session_id }));
            }
        }
        let safe_message = error
            .get(keys::MESSAGE)
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| rust_i18n::t!("app.service.request_rejected").to_string());
        let message = vtcode_commons::sanitizer::sanitize_provider_diagnostic(safe_message)
            .chars()
            .filter(|character| !character.is_control())
            .take(240)
            .collect();
        Ok(Some(request.finish(None, Some(message))))
    } else {
        let result = value.get_mut(keys::RESULT).map(Value::take).ok_or(ProtocolDecodeError)?;
        Ok(Some(request.finish(Some(result), None)))
    }
}

fn send_ui_event(sender: &BackendEventSender, event: BackendEvent) -> bool {
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
    fn startup_interaction_error_is_typed_only_for_turn_submission() {
        for (method, typed) in [
            (protocol::method::TURN_SUBMIT, true),
            (protocol::method::SESSION_CREATE, false),
        ] {
            let mut pending = HashMap::from([(
                RequestId::Number(7),
                PendingRequest::new(method.into(), &json!({ "sessionId": "session" })),
            )]);
            let frame = serde_json::to_vec(&protocol::error_response(
                Some(RequestId::Number(7)),
                protocol::STARTUP_INTERACTION_PENDING_ERROR,
                "waiting",
            ))
            .unwrap();
            let event = decode_frame(&frame, &mut pending).unwrap().unwrap();
            assert_eq!(matches!(event, BackendEvent::TurnBlockedByStartupInteraction { .. }), typed);
        }
    }

    #[tokio::test]
    async fn request_size_preflight_matches_protocol_encoding_at_the_boundary() {
        let method = protocol::method::TURN_SUBMIT;
        let empty = json!({ "prompt": "" });
        let envelope = json!({ "jsonrpc": "2.0", "id": i64::MAX, "method": method, "params": empty });
        let overhead = serde_json::to_vec(&envelope).unwrap().len();
        for (extra, fits) in [(0, true), (1, false)] {
            let params = json!({ "prompt": "a".repeat(protocol::MAX_FRAME_BYTES - overhead + extra) });
            assert_eq!(request_fits_frame(method, &params), fits);
            let request = json!({ "jsonrpc": "2.0", "id": i64::MAX, "method": method, "params": params });
            assert_eq!(protocol::write_frame(&mut tokio::io::sink(), &request).await.is_ok(), fits);
        }
        assert!(!request_fits_frame(method, &json!({ "prompt": "\u{1}".repeat(protocol::MAX_FRAME_BYTES / 5) })));
    }

    #[test]
    fn protocol_responses_are_correlated_with_the_request_method() {
        let mut pending =
            HashMap::from([(RequestId::Number(9), PendingRequest::new("session/list".into(), &Value::Null))]);
        let event = decode_frame(br#"{"jsonrpc":"2.0","id":9,"result":[]}"#, &mut pending)
            .expect("valid response")
            .expect("decoded response");
        assert!(
            matches!(event, BackendEvent::Response { method, result: Some(_), error: None } if method == "session/list")
        );
        assert!(pending.is_empty());
    }

    #[test]
    fn workspace_review_errors_retain_request_identity() {
        let mut pending = HashMap::from([(
            RequestId::Number(12),
            PendingRequest::new(protocol::method::WORKSPACE_REVIEW.into(), &json!({ "requestId": "review-old" })),
        )]);
        let event = decode_frame(br#"{"jsonrpc":"2.0","id":12,"error":{"message":"review failed"}}"#, &mut pending)
            .expect("valid response")
            .expect("decoded response");
        assert!(
            matches!(event, BackendEvent::WorkspaceReviewResponse { request_id, result: None, error: Some(error) }
            if request_id == "review-old" && error == "review failed")
        );
    }

    #[test]
    fn session_history_responses_retain_the_page_request_identity() {
        let mut pending = HashMap::from([(
            RequestId::Number(10),
            PendingRequest::new(
                protocol::method::SESSION_HISTORY.into(),
                &json!({ "requestId": "page-current", "sessionId": "session" }),
            ),
        )]);
        let event = decode_frame(br#"{"jsonrpc":"2.0","id":10,"error":{"message":"page failed"}}"#, &mut pending)
            .expect("valid response")
            .expect("decoded response");
        assert!(matches!(
            event,
            BackendEvent::SessionHistoryResponse { request_id, result: None, error: Some(error) }
                if request_id == "page-current" && error == "page failed"
        ));
    }

    #[test]
    fn session_output_responses_retain_the_page_request_identity() {
        let mut pending = HashMap::from([(
            RequestId::Number(11),
            PendingRequest::new(
                protocol::method::SESSION_OUTPUT_READ.into(),
                &json!({ "requestId": "output-current", "itemId": "tool-output" }),
            ),
        )]);
        let event = decode_frame(br#"{"jsonrpc":"2.0","id":11,"error":{"message":"page failed"}}"#, &mut pending)
            .expect("valid response")
            .expect("decoded response");
        assert!(matches!(
            event,
            BackendEvent::SessionOutputResponse { request_id, result: None, error: Some(error) }
                if request_id == "output-current" && error == "page failed"
        ));
    }

    #[test]
    fn mcp_app_responses_retain_the_read_request_identity() {
        let mut pending = HashMap::from([(
            RequestId::Number(12),
            PendingRequest::new(
                protocol::method::MCP_APP_READ.into(),
                &json!({ "requestId": "app-current", "uri": "ui://demo/app.html" }),
            ),
        )]);
        let event = decode_frame(br#"{"jsonrpc":"2.0","id":12,"result":{"uri":"ui://demo/app.html"}}"#, &mut pending)
            .expect("valid response")
            .expect("decoded response");
        assert!(matches!(
            event,
            BackendEvent::McpAppResponse { request_id, result: Some(_), error: None }
                if request_id == "app-current"
        ));
    }

    #[test]
    fn interaction_replies_retain_request_identity_when_responses_arrive_out_of_order() {
        let request = |interaction_id: &str, cancelled| {
            PendingRequest::new(
                protocol::method::INTERACTION_RESPOND.into(),
                &json!({ "sessionId": "session", "turnId": "turn", "interactionId": interaction_id, "cancelled": cancelled }),
            )
        };
        let mut pending = HashMap::from([
            (RequestId::Number(7), request("old-question", false)),
            (RequestId::Number(8), request("new-question", true)),
        ]);
        let rejected =
            decode_frame(br#"{"jsonrpc":"2.0","id":8,"error":{"message":"Retry this decision"}}"#, &mut pending)
                .expect("valid rejection")
                .expect("correlated event");
        assert!(matches!(rejected, BackendEvent::InteractionResponded { identity, error: Some(error) }
            if identity.interaction_id == "new-question" && identity.session_id == "session"
                && identity.turn_id == "turn" && identity.cancelled && error == "Retry this decision"));
        let accepted = decode_frame(br#"{"jsonrpc":"2.0","id":7,"result":{}}"#, &mut pending)
            .expect("valid acceptance")
            .expect("correlated event");
        assert!(matches!(accepted, BackendEvent::InteractionResponded { identity, error: None }
            if identity.interaction_id == "old-question" && !identity.cancelled));
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
    fn primary_agent_notifications_are_typed_and_session_scoped() {
        let mut pending = HashMap::new();
        let frame = br#"{"jsonrpc":"2.0","method":"session/primaryAgentChanged","params":{"sessionId":"session-a","displayName":"Plan"}}"#;
        let event = decode_frame(frame, &mut pending)
            .expect("valid notification")
            .expect("decoded notification");
        assert!(matches!(
            event,
            BackendEvent::PrimaryAgentChanged(protocol::PrimaryAgentChanged { session_id, display_name: Some(name) })
                if session_id == "session-a" && name == "Plan"
        ));
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

    #[test]
    fn packaged_apps_only_resolve_their_bundled_backend() {
        let candidates = packaged_backend_candidates(Path::new("/Applications/VT Code.app/Contents/MacOS"));
        assert_eq!(candidates, Some(vec![PathBuf::from("/Applications/VT Code.app/Contents/Helpers/vtcode")]));
    }

    #[test]
    fn development_builds_keep_adjacent_and_workspace_backend_fallbacks() {
        let candidates = backend_development_candidates(Path::new("/workspace/target/debug"), Path::new("/workspace"));
        assert_eq!(
            candidates,
            [
                PathBuf::from("/workspace/target/debug/vtcode"),
                PathBuf::from("/workspace/target/release/vtcode"),
            ]
        );
        assert!(packaged_backend_candidates(Path::new("/workspace/target/debug")).is_none());
    }
}
