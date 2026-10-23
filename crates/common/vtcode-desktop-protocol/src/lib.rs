//! Versioned JSON-RPC messages shared by the VT Code desktop app and service.
//!
//! The protocol deliberately carries the canonical [`VersionedThreadEvent`]
//! instead of defining a second runtime event model.

use std::collections::{HashMap, VecDeque};
use std::io;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};
use vtcode_exec_events::VersionedThreadEvent;

/// Current protocol version supported by the desktop client and service.
pub const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion { major: 1, minor: 0 };

/// Largest permitted JSON-RPC line, excluding its line ending.
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

/// Capacity used by host-owned event and command queues.
pub const DEFAULT_QUEUE_CAPACITY: usize = 128;

/// A comparable protocol version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ProtocolVersion {
    /// Incompatible wire-contract changes increment the major version.
    pub major: u16,
    /// Compatible additive changes increment the minor version.
    pub minor: u16,
}

/// A client's acceptable range for protocol negotiation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolRange {
    /// Lowest accepted version.
    pub minimum: ProtocolVersion,
    /// Highest accepted version.
    pub maximum: ProtocolVersion,
}

impl ProtocolRange {
    /// Select the highest compatible version in this range.
    pub fn negotiate(self, server: ProtocolVersion) -> Result<ProtocolVersion, ProtocolError> {
        if self.minimum.major != self.maximum.major || server.major != self.minimum.major {
            return Err(ProtocolError::IncompatibleVersion { client: self, server });
        }

        if server < self.minimum || server > self.maximum {
            return Err(ProtocolError::IncompatibleVersion { client: self, server });
        }

        Ok(server)
    }
}

/// JSON-RPC request id. `null` is intentionally not accepted for requests.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    /// String request identifier.
    String(String),
    /// Integer request identifier.
    Number(i64),
}

/// A JSON-RPC request received from the desktop client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    /// JSON-RPC version marker.
    pub jsonrpc: String,
    /// Request correlation id.
    pub id: RequestId,
    /// Method name.
    pub method: String,
    /// Typed by the method handler.
    #[serde(default)]
    pub params: Value,
}

/// A JSON-RPC notification received from or sent to the desktop client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notification {
    /// JSON-RPC version marker.
    pub jsonrpc: String,
    /// Method name.
    pub method: String,
    /// Typed by the method handler.
    #[serde(default)]
    pub params: Value,
}

/// A successful JSON-RPC response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    /// JSON-RPC version marker.
    pub jsonrpc: String,
    /// Request correlation id.
    pub id: RequestId,
    /// Result payload.
    pub result: Value,
}

/// A failed JSON-RPC response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorResponse {
    /// JSON-RPC version marker.
    pub jsonrpc: String,
    /// Request correlation id; omitted for malformed requests without an id.
    pub id: Option<RequestId>,
    /// Error payload.
    pub error: RpcError,
}

/// Standard JSON-RPC error body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RpcError {
    /// JSON-RPC error code.
    pub code: i32,
    /// Sanitized error message.
    pub message: String,
    /// Optional structured data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Client initialization request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    /// Range of protocol versions supported by the client.
    pub protocol: ProtocolRange,
    /// Client application name.
    pub client_name: String,
    /// Client application version.
    pub client_version: String,
}

/// Successful initialization response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    /// Negotiated protocol version.
    pub protocol: ProtocolVersion,
    /// Canonical selected workspace path.
    pub workspace: String,
    /// Server version.
    pub server_version: String,
}

/// Basic workspace and provider configuration exposed to the GUI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigurationSnapshot {
    /// Active provider identifier.
    pub provider: String,
    /// Active model identifier.
    pub model: String,
    /// Whether provider authentication requirements passed startup preflight.
    pub authenticated: bool,
    /// Existing CLI command users can run to configure credentials.
    pub credential_setup_command: String,
}

/// A persisted session available for reload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    /// Stable durable session identifier.
    pub session_id: String,
    /// Human-readable title when known.
    pub title: Option<String>,
    /// Session workspace path.
    pub workspace: String,
    /// Whether the last observed state was terminal.
    pub completed: bool,
    /// Whether this session can be continued using existing archive data.
    pub resumable: bool,
}

/// Text restored from the durable message snapshot for a loaded session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionHistoryMessage {
    /// Message role such as `user`, `assistant`, or `tool`.
    pub role: String,
    /// Text projection of the persisted message content.
    pub text: String,
}

/// Loaded durable session metadata and its bounded transcript projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionLoadResult {
    /// Selected session metadata.
    pub session: SessionSummary,
    /// Recent messages restored from the archive snapshot.
    pub messages: Vec<SessionHistoryMessage>,
}

/// Parameters for creating an empty session.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCreateParams {
    /// Optional title shown in history.
    pub title: Option<String>,
}

/// Parameters for loading a durable session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionIdParams {
    /// Durable session identifier.
    pub session_id: String,
}

/// Parameters for submitting a prompt to the selected session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnSubmitParams {
    /// Session that owns the new turn.
    pub session_id: String,
    /// User prompt.
    pub prompt: String,
}

/// Accepted turn response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnAccepted {
    /// Session identifier.
    pub session_id: String,
    /// Runtime turn identifier.
    pub turn_id: String,
}

/// Parameters for cancelling an active turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnCancelParams {
    /// Owning session.
    pub session_id: String,
    /// Exact turn identity to cancel.
    pub turn_id: String,
}

/// User response to a pending runtime interaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionResponse {
    /// Session owning the interaction.
    pub session_id: String,
    /// Turn that raised the interaction.
    pub turn_id: String,
    /// Opaque interaction identity provided by the service.
    pub interaction_id: String,
    /// Opaque choice identity provided by the service, if selecting an option.
    pub choice_id: Option<String>,
    /// Opaque identities selected for each step in a wizard interaction.
    #[serde(default)]
    pub choice_ids: Vec<String>,
    /// Free-form answer for text questions.
    pub text: Option<String>,
    /// Free-form answers for each ordered wizard step.
    #[serde(default)]
    pub answer_texts: Vec<Option<String>>,
    /// Whether the interaction was explicitly cancelled by the user.
    #[serde(default)]
    pub cancelled: bool,
}

/// Interaction request sent from the service to the desktop client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionRequest {
    /// Session owning the interaction.
    pub session_id: String,
    /// Turn that raised the interaction.
    pub turn_id: String,
    /// Opaque interaction identity.
    pub interaction_id: String,
    /// Kind of interaction required.
    pub kind: InteractionKind,
    /// Whether the runtime expects an ordered wizard response.
    #[serde(default)]
    pub wizard: bool,
    /// Dialog title or question prompt.
    pub title: String,
    /// Supporting text shown by the client.
    pub details: Vec<String>,
    /// Available choices, preserving each runtime's opaque identity.
    pub choices: Vec<InteractionChoice>,
    /// Ordered multi-step questions when the runtime request is a wizard.
    #[serde(default)]
    pub steps: Vec<InteractionStep>,
    /// Optional selected choice identity.
    pub selected_choice_id: Option<String>,
    /// Optional selected choice identities, one per wizard step.
    #[serde(default)]
    pub selected_choice_ids: Vec<String>,
}

/// One ordered step in a question or approval wizard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionStep {
    /// Step label.
    pub title: String,
    /// Question or instruction for this step.
    pub question: String,
    /// Choices preserving exact service-issued identities.
    pub choices: Vec<InteractionChoice>,
    /// Optional selected identity.
    pub selected_choice_id: Option<String>,
    /// Whether the client may submit a free-form answer.
    pub allow_freeform: bool,
}

/// Interaction category understood by native clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionKind {
    /// A tool or policy approval decision.
    Approval,
    /// Plan approval decision.
    PlanApproval,
    /// User-facing question requiring a response.
    Question,
    /// Non-security choice picker.
    Choice,
    /// Unsupported or informational transient request.
    Other,
}

/// One display label paired with a stable opaque identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionChoice {
    /// Opaque identity returned unchanged by the client.
    pub id: String,
    /// User-facing label.
    pub label: String,
    /// Optional supporting description.
    pub description: Option<String>,
    /// Whether the service recommends this option.
    pub recommended: bool,
}

/// Canonical runtime event notification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadEventNotification {
    /// Session owning the event.
    pub session_id: String,
    /// Turn identity when the event is turn scoped.
    pub turn_id: Option<String>,
    /// Authoritative, schema-versioned runtime event.
    pub event: VersionedThreadEvent,
}

/// Known desktop JSON-RPC methods.
pub mod method {
    /// Initialize and negotiate protocol support.
    pub const INITIALIZE: &str = "desktop/initialize";
    /// List sessions in the selected workspace.
    pub const SESSION_LIST: &str = "session/list";
    /// Create a session.
    pub const SESSION_CREATE: &str = "session/create";
    /// Load a durable session.
    pub const SESSION_LOAD: &str = "session/load";
    /// Close the selected session after cleanup.
    pub const SESSION_CLOSE: &str = "session/close";
    /// Submit a user turn.
    pub const TURN_SUBMIT: &str = "turn/submit";
    /// Cancel a running turn.
    pub const TURN_CANCEL: &str = "turn/cancel";
    /// Respond to a pending interaction.
    pub const INTERACTION_RESPOND: &str = "interaction/respond";
    /// Discover active provider/model setup.
    pub const CONFIG_DISCOVER: &str = "config/discover";
    /// Shut down the local service after owned tasks finish.
    pub const SHUTDOWN: &str = "desktop/shutdown";
    /// Stream an authoritative runtime event.
    pub const THREAD_EVENT: &str = "thread/event";
    /// Ask the client to resolve a runtime interaction.
    pub const INTERACTION_REQUESTED: &str = "interaction/requested";
    /// Publish visible turn state.
    pub const TURN_STATUS: &str = "turn/status";
}

/// Error raised by protocol framing, negotiation, or identity validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    /// A frame exceeds the configured byte limit.
    FrameTooLarge {
        /// Maximum accepted frame size in bytes.
        limit: usize,
    },
    /// The client and service do not share a supported protocol version.
    IncompatibleVersion {
        /// Client-supported range.
        client: ProtocolRange,
        /// Server version.
        server: ProtocolVersion,
    },
    /// Interaction does not exactly match the active session, turn, and id.
    StaleInteraction,
    /// Interaction was already resolved or never registered.
    UnknownInteraction,
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FrameTooLarge { limit } => write!(formatter, "JSON-RPC frame exceeds {limit} bytes"),
            Self::IncompatibleVersion { client, server } => write!(
                formatter,
                "incompatible desktop protocol versions: client {}.{}..={}.{}, server {}.{}",
                client.minimum.major,
                client.minimum.minor,
                client.maximum.major,
                client.maximum.minor,
                server.major,
                server.minor
            ),
            Self::StaleInteraction => {
                formatter.write_str("interaction response does not match the active session and turn")
            }
            Self::UnknownInteraction => formatter.write_str("interaction is no longer pending"),
        }
    }
}

impl std::error::Error for ProtocolError {}

/// Track pending interaction identities and reject stale or duplicate replies.
#[derive(Debug, Default)]
pub struct InteractionRegistry {
    pending: HashMap<String, (String, String)>,
}

impl InteractionRegistry {
    /// Register an interaction id for an exact session and turn.
    pub fn insert(&mut self, interaction: &InteractionRequest) -> Result<(), ProtocolError> {
        if self.pending.contains_key(&interaction.interaction_id) {
            return Err(ProtocolError::UnknownInteraction);
        }
        let replaced = self
            .pending
            .insert(interaction.interaction_id.clone(), (interaction.session_id.clone(), interaction.turn_id.clone()));
        debug_assert!(replaced.is_none(), "duplicate interaction identity was checked before insertion");
        Ok(())
    }

    /// Validate and consume exactly one matching response.
    pub fn resolve(&mut self, response: &InteractionResponse) -> Result<(), ProtocolError> {
        let Some((session_id, turn_id)) = self.pending.get(&response.interaction_id) else {
            return Err(ProtocolError::UnknownInteraction);
        };
        if session_id != &response.session_id || turn_id != &response.turn_id {
            return Err(ProtocolError::StaleInteraction);
        }
        if self.pending.remove(&response.interaction_id).is_none() {
            return Err(ProtocolError::UnknownInteraction);
        }
        Ok(())
    }

    /// Drop all requests for a turn after cancellation or completion.
    pub fn finish_turn(&mut self, session_id: &str, turn_id: &str) {
        self.pending
            .retain(|_, (pending_session, pending_turn)| pending_session != session_id || pending_turn != turn_id);
    }

    /// Drop all requests for a disconnected session.
    pub fn close_session(&mut self, session_id: &str) {
        self.pending.retain(|_, (pending_session, _)| pending_session != session_id);
    }
}

/// Read one bounded JSON-RPC line. A final unterminated line is accepted at EOF.
pub async fn read_frame<R>(reader: &mut R) -> io::Result<Option<Vec<u8>>>
where
    R: AsyncBufRead + Unpin,
{
    let mut frame = Vec::with_capacity(1024);
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return if frame.is_empty() { Ok(None) } else { Ok(Some(frame)) };
        }

        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |index| index + 1);
        let content_end = if newline.is_some() && available.get(take.saturating_sub(1)) == Some(&b'\n') {
            take.saturating_sub(1)
        } else {
            take
        };
        let content_end = if content_end > 0 && available.get(content_end.saturating_sub(1)) == Some(&b'\r') {
            content_end.saturating_sub(1)
        } else {
            content_end
        };

        if frame.len().saturating_add(content_end) > MAX_FRAME_BYTES {
            reader.consume(take);
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                ProtocolError::FrameTooLarge { limit: MAX_FRAME_BYTES },
            ));
        }
        let content = available
            .get(..content_end)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid buffered frame boundary"))?;
        frame.extend_from_slice(content);
        reader.consume(take);

        if newline.is_some() {
            return Ok(Some(frame));
        }
    }
}

/// Write one compact JSON-RPC line and flush it to the client.
pub async fn write_frame<W, T>(writer: &mut W, message: &T) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let encoded = serde_json::to_vec(message).map_err(io::Error::other)?;
    if encoded.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            ProtocolError::FrameTooLarge { limit: MAX_FRAME_BYTES },
        ));
    }
    writer.write_all(&encoded).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await
}

/// Build a JSON-RPC success response.
pub fn success_response(id: RequestId, result: impl Serialize) -> Result<Response, serde_json::Error> {
    Ok(Response {
        jsonrpc: "2.0".to_string(),
        id,
        result: serde_json::to_value(result)?,
    })
}

/// Build a JSON-RPC error response with a sanitized public message.
pub fn error_response(id: Option<RequestId>, code: i32, message: impl Into<String>) -> ErrorResponse {
    ErrorResponse {
        jsonrpc: "2.0".to_string(),
        id,
        error: RpcError { code, message: message.into(), data: None },
    }
}

/// Create the bounded live event channel that streams runtime events from the
/// agent runloop to the desktop service pump.
///
/// Unlike a plain bounded channel, a full queue never fails the producer:
/// `item.updated` snapshots replace the pending snapshot for the same item and
/// consecutive `plan.delta` events for one item merge into one entry, so a
/// slow consumer degrades the stream instead of failing a healthy turn.
/// Ordered events are only dropped when the queue holds no coalescable entry,
/// and every such drop is counted for consumer-side degradation reporting.
pub fn live_event_channel(capacity: usize) -> (LiveEventSender, LiveEventReceiver) {
    assert!(capacity > 0, "live event capacity must be positive");
    let inner = std::sync::Arc::new(LiveEventQueueInner {
        capacity,
        queue: std::sync::Mutex::new(VecDeque::new()),
        senders: std::sync::atomic::AtomicUsize::new(1),
        receiver_alive: std::sync::atomic::AtomicBool::new(true),
        dropped_ordered: std::sync::atomic::AtomicU64::new(0),
        dropped_stale: std::sync::atomic::AtomicU64::new(0),
        notify: tokio::sync::Notify::new(),
    });
    (LiveEventSender { inner: inner.clone() }, LiveEventReceiver { inner })
}

/// How a streaming event combines with the pending entry for the same key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CoalesceOp {
    /// The new event is a full snapshot that supersedes the pending one.
    Replace,
    /// The new event carries incremental text appended to the pending one.
    Merge,
}

/// One queue entry: a coalescable streaming event keyed by item, or an
/// ordered event that must keep its relative position.
struct LiveEventSlot {
    key: Option<String>,
    op: CoalesceOp,
    event: VersionedThreadEvent,
}

struct LiveEventQueueInner {
    capacity: usize,
    queue: std::sync::Mutex<VecDeque<LiveEventSlot>>,
    senders: std::sync::atomic::AtomicUsize,
    receiver_alive: std::sync::atomic::AtomicBool,
    dropped_ordered: std::sync::atomic::AtomicU64,
    dropped_stale: std::sync::atomic::AtomicU64,
    notify: tokio::sync::Notify,
}

impl LiveEventQueueInner {
    fn classify(event: &VersionedThreadEvent) -> Option<(String, CoalesceOp)> {
        match event.event() {
            vtcode_exec_events::ThreadEvent::ItemUpdated(update) => Some((update.item.id.clone(), CoalesceOp::Replace)),
            vtcode_exec_events::ThreadEvent::PlanDelta(delta) => Some((delta.item_id.clone(), CoalesceOp::Merge)),
            _ => None,
        }
    }

    /// Append an event, coalescing streaming updates and keeping the queue
    /// bounded. Returns `Err(())` only when the consumer half was dropped.
    fn publish(&self, event: VersionedThreadEvent) -> Result<(), ()> {
        if !self.receiver_alive.load(std::sync::atomic::Ordering::Acquire) {
            return Err(());
        }
        let mut queue = self.queue.lock().map_err(|_poisoned| ())?;
        let classified = Self::classify(&event);
        let slot_key = classified.as_ref().map(|(key, _)| key.clone());
        let slot_op = classified.as_ref().map_or(CoalesceOp::Replace, |(_, op)| *op);
        if let Some((key, op)) = classified {
            let pending_slot = queue
                .iter()
                .position(|slot| slot.key.as_deref() == Some(key.as_str()) && slot.op == op)
                .and_then(|index| queue.remove(index).map(|removed| (index, removed)));
            if let Some((index, pending)) = pending_slot {
                // Re-insert at the removed slot's position so the relative
                // order of every other queued event is preserved.
                let merged = match op {
                    CoalesceOp::Replace => event,
                    CoalesceOp::Merge => Self::merge_plan_deltas(pending.event, event),
                };
                queue.insert(index, LiveEventSlot { key: Some(key), op, event: merged });
                drop(queue);
                self.notify.notify_waiters();
                return Ok(());
            }
        }
        if queue.len() >= self.capacity {
            // Prefer displacing a stale streaming entry over dropping an
            // ordered event: the latest snapshot for an item always arrives
            // with its terminal `item.completed` event.
            let victim = queue.iter().position(|slot| slot.key.is_some());
            match victim {
                Some(index) => {
                    drop(queue.remove(index));
                    let _ = self.dropped_stale.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                None => {
                    drop(queue.pop_front());
                    let _ = self.dropped_ordered.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
        }
        queue.push_back(LiveEventSlot { key: slot_key, op: slot_op, event });
        drop(queue);
        self.notify.notify_waiters();
        Ok(())
    }

    /// Concatenate two `plan.delta` events for the same item, keeping the
    /// newest metadata and the freshest non-empty recovery snapshot.
    fn merge_plan_deltas(pending: VersionedThreadEvent, next: VersionedThreadEvent) -> VersionedThreadEvent {
        let (pending, next) = (pending.into_event(), next.into_event());
        match (pending, next) {
            (vtcode_exec_events::ThreadEvent::PlanDelta(older), vtcode_exec_events::ThreadEvent::PlanDelta(newer)) => {
                let mut merged = newer.clone();
                let mut text = older.delta;
                text.push_str(&newer.delta);
                merged.delta = text;
                if merged.recovery.is_none() {
                    merged.recovery = older.recovery;
                }
                VersionedThreadEvent::new(vtcode_exec_events::ThreadEvent::PlanDelta(merged))
            }
            (_, fallback) => VersionedThreadEvent::new(fallback),
        }
    }
}

/// Producer half of the live event channel.
pub struct LiveEventSender {
    inner: std::sync::Arc<LiveEventQueueInner>,
}

impl Clone for LiveEventSender {
    fn clone(&self) -> Self {
        let _ = self.inner.senders.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self { inner: std::sync::Arc::clone(&self.inner) }
    }
}

impl LiveEventSender {
    /// Publish one event. Coalesces streaming updates when the queue is
    /// saturated; fails only when the consumer half is gone.
    pub fn publish(&self, event: VersionedThreadEvent) -> Result<(), LiveEventSendError> {
        self.inner.publish(event).map_err(|()| LiveEventSendError(()))
    }
}

impl Drop for LiveEventSender {
    fn drop(&mut self) {
        let _ = self.inner.senders.fetch_sub(1, std::sync::atomic::Ordering::Release);
        self.inner.notify.notify_waiters();
    }
}

/// Consumer half of the live event channel.
pub struct LiveEventReceiver {
    inner: std::sync::Arc<LiveEventQueueInner>,
}

impl LiveEventReceiver {
    /// Receive the next event, waiting while the queue is empty.
    ///
    /// Returns `None` once every sender was dropped and the queue drained.
    pub async fn recv(&self) -> Option<VersionedThreadEvent> {
        loop {
            {
                let mut queue = self.inner.queue.lock().ok()?;
                if let Some(slot) = queue.pop_front() {
                    return Some(slot.event);
                }
                if self.inner.senders.load(std::sync::atomic::Ordering::Acquire) == 0 {
                    return None;
                }
            }
            let notified = self.inner.notify.notified();
            {
                let queue = self.inner.queue.lock().ok()?;
                if !queue.is_empty() {
                    continue;
                }
                if self.inner.senders.load(std::sync::atomic::Ordering::Acquire) == 0 {
                    return None;
                }
            }
            notified.await;
        }
    }

    /// Take and reset the count of ordered events dropped under saturation.
    pub fn take_dropped_ordered(&self) -> u64 {
        self.inner.dropped_ordered.swap(0, std::sync::atomic::Ordering::Relaxed)
    }

    /// Take and reset the count of superseded streaming snapshots displaced
    /// under saturation. These are best-effort previews, not stream gaps.
    pub fn take_dropped_stale(&self) -> u64 {
        self.inner.dropped_stale.swap(0, std::sync::atomic::Ordering::Relaxed)
    }
}

impl Drop for LiveEventReceiver {
    fn drop(&mut self) {
        self.inner.receiver_alive.store(false, std::sync::atomic::Ordering::Release);
        self.inner.notify.notify_waiters();
    }
}

/// The consumer half of the live event channel is gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveEventSendError(());

impl std::fmt::Display for LiveEventSendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("live event consumer disconnected")
    }
}

impl std::error::Error for LiveEventSendError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn negotiation_selects_server_version_and_rejects_major_mismatch() {
        let range = ProtocolRange {
            minimum: ProtocolVersion { major: 1, minor: 0 },
            maximum: ProtocolVersion { major: 1, minor: 3 },
        };
        assert_eq!(range.negotiate(PROTOCOL_VERSION), Ok(PROTOCOL_VERSION));
        assert!(matches!(
            ProtocolRange {
                minimum: ProtocolVersion { major: 2, minor: 0 },
                maximum: ProtocolVersion { major: 2, minor: 1 }
            }
            .negotiate(PROTOCOL_VERSION),
            Err(ProtocolError::IncompatibleVersion { .. })
        ));
    }

    #[test]
    fn interaction_responses_are_session_turn_scoped_and_single_use() {
        let mut registry = InteractionRegistry::default();
        let interaction = InteractionRequest {
            session_id: "session-a".to_string(),
            turn_id: "turn-9".to_string(),
            interaction_id: "interaction-x".to_string(),
            kind: InteractionKind::Approval,
            wizard: false,
            title: "Approve tool".to_string(),
            details: vec![],
            choices: vec![InteractionChoice {
                id: "runtime-choice-allow".to_string(),
                label: "Allow once".to_string(),
                description: None,
                recommended: true,
            }],
            steps: Vec::new(),
            selected_choice_id: None,
            selected_choice_ids: Vec::new(),
        };
        registry.insert(&interaction).expect("register request");
        let stale = InteractionResponse {
            session_id: "session-a".to_string(),
            turn_id: "turn-8".to_string(),
            interaction_id: "interaction-x".to_string(),
            choice_id: Some("runtime-choice-allow".to_string()),
            choice_ids: Vec::new(),
            text: None,
            answer_texts: Vec::new(),
            cancelled: false,
        };
        assert_eq!(registry.resolve(&stale), Err(ProtocolError::StaleInteraction));
        let valid = InteractionResponse { turn_id: "turn-9".to_string(), ..stale };
        assert_eq!(registry.resolve(&valid), Ok(()));
        assert_eq!(registry.resolve(&valid), Err(ProtocolError::UnknownInteraction));
    }

    #[tokio::test]
    async fn bounded_line_reader_accepts_crlf_and_final_unterminated_frame() -> Result<(), Box<dyn Error>> {
        let input = b"{\"jsonrpc\":\"2.0\"}\r\n{}";
        let mut reader = tokio::io::BufReader::new(&input[..]);
        assert_eq!(read_frame(&mut reader).await?, Some(br#"{"jsonrpc":"2.0"}"#.to_vec()));
        assert_eq!(read_frame(&mut reader).await?, Some(b"{}".to_vec()));
        assert_eq!(read_frame(&mut reader).await?, None);
        Ok(())
    }

    #[tokio::test]
    async fn bounded_line_reader_rejects_oversized_input() {
        let input = vec![b'x'; MAX_FRAME_BYTES + 1];
        let mut reader = tokio::io::BufReader::new(&input[..]);
        let error = read_frame(&mut reader).await.expect_err("oversized frame");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("exceeds"));
    }

    #[test]
    fn protocol_methods_and_wire_structs_use_camel_case() -> Result<(), Box<dyn Error>> {
        let params = InteractionResponse {
            session_id: "s".to_string(),
            turn_id: "t".to_string(),
            interaction_id: "i".to_string(),
            choice_id: Some("choice-opaque".to_string()),
            choice_ids: Vec::new(),
            text: None,
            answer_texts: Vec::new(),
            cancelled: false,
        };
        let value = serde_json::to_value(params)?;
        assert_eq!(value["sessionId"], "s");
        assert_eq!(value["interactionId"], "i");
        assert_eq!(value["choiceId"], "choice-opaque");
        assert_eq!(method::SESSION_CREATE, "session/create");
        Ok(())
    }

    fn item_update(id: &str, text: &str) -> VersionedThreadEvent {
        use vtcode_exec_events::{AgentMessageItem, ItemUpdatedEvent, ThreadItem, ThreadItemDetails};
        VersionedThreadEvent::new(vtcode_exec_events::ThreadEvent::ItemUpdated(ItemUpdatedEvent {
            item: ThreadItem {
                context: None,
                id: id.to_string(),
                details: ThreadItemDetails::AgentMessage(AgentMessageItem { text: text.to_string() }),
            },
        }))
    }

    fn plan_delta(item_id: &str, delta: &str) -> VersionedThreadEvent {
        use vtcode_exec_events::PlanDeltaEvent;
        VersionedThreadEvent::new(vtcode_exec_events::ThreadEvent::PlanDelta(Box::new(PlanDeltaEvent {
            thread_id: "thread".to_string(),
            turn_id: "turn".to_string(),
            item_id: item_id.to_string(),
            delta: delta.to_string(),
            recovery: None,
        })))
    }

    fn turn_completed() -> VersionedThreadEvent {
        use vtcode_exec_events::{TurnCompletedEvent, Usage};
        VersionedThreadEvent::new(vtcode_exec_events::ThreadEvent::TurnCompleted(TurnCompletedEvent {
            completed_at: None,
            usage: Usage {
                input_tokens: 0,
                cached_input_tokens: 0,
                cache_creation_tokens: 0,
                output_tokens: 0,
            },
            in_progress_exec_sessions: Vec::new(),
        }))
    }

    fn thread_started(id: &str) -> VersionedThreadEvent {
        use vtcode_exec_events::ThreadStartedEvent;
        VersionedThreadEvent::new(vtcode_exec_events::ThreadEvent::ThreadStarted(ThreadStartedEvent {
            thread_id: id.to_string(),
        }))
    }

    #[tokio::test]
    async fn saturated_queue_never_fails_the_producer_and_keeps_lifecycle_events() {
        let (sender, receiver) = live_event_channel(8);
        // Fill the queue past capacity with streaming snapshots and deltas.
        for round in 0..64 {
            sender
                .publish(item_update(&format!("item-{}", round % 4), &format!("round {round}")))
                .expect("publish");
            sender.publish(plan_delta("plan", &format!("{round} "))).expect("publish");
        }
        // A lifecycle event must be admitted even while the queue is full.
        sender.publish(turn_completed()).expect("lifecycle publish must not fail");
        // The producer going away ends the stream after draining.
        drop(sender);

        let mut ordered = Vec::new();
        while let Some(event) = receiver.recv().await {
            ordered.push(event);
        }

        assert_eq!(
            ordered.last().expect("drained events"),
            &turn_completed(),
            "lifecycle event must survive saturation"
        );
        let plan_events: Vec<_> = ordered
            .iter()
            .filter(|event| matches!(event.event(), vtcode_exec_events::ThreadEvent::PlanDelta(_)))
            .collect();
        assert_eq!(plan_events.len(), 1, "all plan deltas for one item merge into one event");
        if let vtcode_exec_events::ThreadEvent::PlanDelta(merged) = plan_events[0].event() {
            assert_eq!(merged.delta, (0..64).map(|round| format!("{round} ")).collect::<String>());
        } else {
            panic!("expected merged plan delta");
        }
        assert!(ordered.len() <= 8 + 1, "queue stays bounded: {}", ordered.len());
        assert_eq!(receiver.take_dropped_ordered(), 0, "no ordered event dropped while coalescable entries existed");
    }

    #[tokio::test]
    async fn saturated_ordered_events_drop_oldest_and_count_the_gap() {
        let (sender, receiver) = live_event_channel(4);
        for round in 0..10 {
            sender.publish(thread_started(&round.to_string())).expect("publish");
        }
        drop(sender);
        let mut drained = Vec::new();
        while let Some(event) = receiver.recv().await {
            drained.push(event);
        }
        assert_eq!(drained.len(), 4, "queue stays bounded");
        assert_eq!(receiver.take_dropped_ordered(), 6, "every displaced ordered event is counted");
        if let vtcode_exec_events::ThreadEvent::ThreadStarted(last) = drained[3].event() {
            assert_eq!(last.thread_id, "9", "the newest ordered event must survive");
        } else {
            panic!("expected thread.started");
        }
    }

    #[tokio::test]
    async fn publish_fails_only_after_the_consumer_half_drops() {
        let (sender, receiver) = live_event_channel(4);
        sender.publish(item_update("item", "text")).expect("publish with live consumer");
        drop(receiver);
        assert!(
            sender.publish(item_update("item", "more")).is_err(),
            "publishing after the consumer dropped must fail"
        );
    }

    #[tokio::test]
    async fn cloned_senders_keep_the_stream_alive_until_the_last_one_drops() {
        let (sender, receiver) = live_event_channel(4);
        let clone = sender.clone();
        drop(sender);
        // A second sender keeps the stream reachable.
        clone.publish(item_update("item", "text")).expect("publish with live clone");
        drop(clone);
        assert!(receiver.recv().await.is_some());
        assert_eq!(receiver.recv().await, None, "drained queue with no senders ends the stream");
    }
}
