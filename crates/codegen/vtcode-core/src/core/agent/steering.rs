use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Maximum number of follow-up intents held by a runtime before new intents
/// park in the overflow buffer. The overflow buffer keeps rapid influx from
/// silently dropping user messages; once both buffers are full the caller
/// receives `FollowUpQueueFull` so back-pressure stays visible.
pub const MAX_QUEUED_FOLLOW_UP_INTENTS: usize = 64;
/// Overflow park capacity after the primary FIFO is full. Combined with the
/// primary cap this bounds total retained follow-ups while still absorbing
/// a burst instead of discarding it.
pub const MAX_OVERFLOW_FOLLOW_UP_INTENTS: usize = 64;
/// Total follow-up slots: primary plus overflow plus the one reserved
/// recovery slot. The memory envelope retains the same count oldest-first
/// per snapshot.
pub const MAX_PENDING_FOLLOW_UP_INTENTS: usize = MAX_QUEUED_FOLLOW_UP_INTENTS + MAX_OVERFLOW_FOLLOW_UP_INTENTS + 1;
/// Maximum number of applied follow-up IDs retained for restart recovery.
pub const MAX_APPLIED_FOLLOW_UP_INTENT_IDS: usize = 64;

/// Origin of a queued follow-up, preserved when it is applied to conversation history.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FollowUpProvenance {
    /// Input supplied by a user through the steering interface.
    #[default]
    UserInput,
    /// A harness-generated continuation or recovery instruction.
    Synthetic,
}

/// A follow-up instruction with a stable identity for recovery and
/// de-duplication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedFollowUpIntent {
    id: String,
    text: String,
    /// Defaults to user input for envelopes written before provenance existed.
    #[serde(default)]
    provenance: FollowUpProvenance,
}

impl QueuedFollowUpIntent {
    /// Create a new intent with a UUIDv4 identity.
    #[must_use]
    pub fn new(text: String) -> Self {
        Self::new_with_provenance(text, FollowUpProvenance::UserInput)
    }

    /// Create a new intent with an explicit source and UUIDv4 identity.
    #[must_use]
    pub fn new_with_provenance(text: String, provenance: FollowUpProvenance) -> Self {
        Self { id: Uuid::new_v4().to_string(), text, provenance }
    }

    /// Reconstruct an intent recovered from durable state.
    pub fn from_parts(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self::from_parts_with_provenance(id, text, FollowUpProvenance::UserInput)
    }

    /// Reconstruct an intent recovered from durable state with its source.
    pub fn from_parts_with_provenance(
        id: impl Into<String>,
        text: impl Into<String>,
        provenance: FollowUpProvenance,
    ) -> Self {
        Self { id: id.into(), text: text.into(), provenance }
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub const fn provenance(&self) -> FollowUpProvenance {
        self.provenance
    }

    /// Consume the intent and return its user-visible text.
    #[must_use]
    pub fn into_parts(self) -> (String, String) {
        (self.id, self.text)
    }

    /// Consume the intent and return its identity, text, and source.
    #[must_use]
    pub fn into_parts_with_provenance(self) -> (String, String, FollowUpProvenance) {
        (self.id, self.text, self.provenance)
    }
}

/// Error returned when a runtime cannot accept another follow-up intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FollowUpQueueFull {
    pub capacity: usize,
}

impl fmt::Display for FollowUpQueueFull {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "follow-up intent queue is full (capacity {})", self.capacity)
    }
}

impl std::error::Error for FollowUpQueueFull {}

/// Why a runtime refused a live steer. The sender uses this to decide how to
/// fall back without ever losing the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SteerRejection {
    /// The turn's admission window is already closed.
    TurnClosing,
    /// Cancellation was requested and takes precedence over the steer.
    CancellationPrecedence,
    /// Both the primary follow-up queue and its overflow park are full.
    QueueFull,
    /// There is no live turn to steer (no receiver or no active turn).
    NoLiveTurn,
    /// The addressed session, turn, or queue entry was superseded.
    Superseded,
}

impl fmt::Display for SteerRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::TurnClosing => "turn is closing",
            Self::CancellationPrecedence => "cancellation takes precedence",
            Self::QueueFull => "follow-up queue is full",
            Self::NoLiveTurn => "no live turn",
            Self::Superseded => "session or turn was superseded",
        };
        f.write_str(label)
    }
}

/// The single terminal outcome of a live steering handoff. Exactly one of
/// these is sent on the receipt carried by [`FollowUpSteer`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SteerOutcome {
    /// The runtime admitted the intent into the follow-up queue. `intent_id`
    /// is the stable identity preserved through history and checkpointing.
    Accepted { intent_id: String },
    /// The runtime refused the steer; the sender must fall back to its own
    /// queue so the message is delivered exactly once.
    Rejected { reason: SteerRejection },
}

impl SteerOutcome {
    /// Whether the steer was admitted.
    #[must_use]
    pub const fn accepted(&self) -> bool {
        matches!(self, Self::Accepted { .. })
    }
}

/// One-shot channel used to report a steer's terminal outcome back to its sender.
pub type SteerReceiptSender = tokio::sync::oneshot::Sender<SteerOutcome>;
/// Receiver half of [`SteerReceiptSender`].
pub type SteerReceipt = tokio::sync::oneshot::Receiver<SteerOutcome>;

/// A follow-up steering input together with its optional delivery receipt.
///
/// The runtime resolves the receipt at the exact moment it admits or refuses
/// the intent, so the sender never has to guess from a channel send whether
/// the message reached model-facing history.
#[derive(Debug)]
pub struct FollowUpSteer {
    pub text: String,
    pub receipt: Option<SteerReceiptSender>,
}

impl FollowUpSteer {
    /// A steer without a receipt (control-path and embedded callers).
    #[must_use]
    pub fn new(text: String) -> Self {
        Self { text, receipt: None }
    }

    /// A steer whose sender expects a terminal outcome.
    #[must_use]
    pub fn with_receipt(text: String, receipt: SteerReceiptSender) -> Self {
        Self { text, receipt: Some(receipt) }
    }

    /// Create a steer and the receiver for its terminal outcome.
    #[must_use]
    pub fn channel(text: String) -> (Self, SteerReceipt) {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        (Self::with_receipt(text, sender), receiver)
    }

    /// Resolve the receipt with exactly one terminal outcome. Consuming `self`
    /// makes a second outcome impossible.
    pub fn resolve(self, outcome: SteerOutcome) {
        if let Some(receipt) = self.receipt {
            // A dropped receiver means the sender no longer cares; there is
            // nobody to deliver a second outcome to.
            let _ = receipt.send(outcome);
        }
    }

    /// Reject this steer with a reason.
    pub fn reject(self, reason: SteerRejection) {
        self.resolve(SteerOutcome::Rejected { reason });
    }
}

/// Messages used to steer the agent's execution loop from an external source.
///
/// This is a control-plane value only: it is never serialized or compared for
/// equality, so it can carry a one-shot receipt directly.
#[derive(Debug)]
pub enum SteeringMessage {
    /// Stop the agent's execution loop immediately (Steering).
    SteerStop,
    /// Pause the agent's execution loop.
    Pause,
    /// Resume the agent's execution loop.
    Resume,
    /// Inject input as a follow-up user message and resolve its delivery
    /// receipt when the runtime admits or refuses it.
    FollowUpInput(FollowUpSteer),
}
