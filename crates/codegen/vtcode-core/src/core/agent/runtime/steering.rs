use super::*;
use crate::core::agent::steering::{SteerOutcome, SteerRejection};

/// Manages steering messages (stop, pause, resume, follow-up inputs) for a running agent turn.
pub struct RuntimeSteering {
    steering_receiver: Option<UnboundedReceiver<SteeringMessage>>,
    pub(super) queued_follow_up_inputs: VecDeque<QueuedFollowUpIntent>,
    /// Overflow park for follow-ups that arrive while the primary FIFO is
    /// full. Drained FIFO into the primary queue as space frees so a rapid
    /// influx is never silently dropped.
    overflow_follow_up_inputs: VecDeque<QueuedFollowUpIntent>,
    recovered_follow_up: Option<QueuedFollowUpIntent>,
    pub(super) in_flight_follow_up_intents: VecDeque<QueuedFollowUpIntent>,
    applied_follow_up_intent_ids: VecDeque<String>,
    /// Once closed, no further live steer may be admitted. Closed atomically
    /// with the final steering drain so a late arrival is rejected instead of
    /// being stranded in the channel after the turn ends.
    admission_closed: bool,
    /// Set by the runloop when cancellation outranks a pending steer.
    cancellation_precedence: bool,
}

impl Default for RuntimeSteering {
    fn default() -> Self {
        Self::new(None)
    }
}

impl RuntimeSteering {
    pub(super) fn new(steering_receiver: Option<UnboundedReceiver<SteeringMessage>>) -> Self {
        Self {
            steering_receiver,
            queued_follow_up_inputs: VecDeque::new(),
            overflow_follow_up_inputs: VecDeque::new(),
            recovered_follow_up: None,
            in_flight_follow_up_intents: VecDeque::new(),
            applied_follow_up_intent_ids: VecDeque::new(),
            admission_closed: false,
            cancellation_precedence: false,
        }
    }

    /// Replace the steering message receiver channel.
    pub fn set_receiver(&mut self, receiver: Option<UnboundedReceiver<SteeringMessage>>) {
        self.steering_receiver = receiver;
    }

    /// Take ownership of the steering receiver, leaving `None` in its place.
    pub fn take_receiver(&mut self) -> Option<UnboundedReceiver<SteeringMessage>> {
        self.steering_receiver.take()
    }

    /// Reopen admission for a new turn. Called once at turn setup.
    pub fn open_admission(&mut self) {
        self.admission_closed = false;
        self.cancellation_precedence = false;
    }

    /// Close admission atomically with the final steering drain. Later arrivals
    /// are rejected rather than left stranded in the channel.
    pub fn close_admission(&mut self) {
        self.admission_closed = true;
    }

    #[must_use]
    pub const fn admission_closed(&self) -> bool {
        self.admission_closed
    }

    /// Declare that cancellation outranks any pending steer until the next
    /// [`Self::open_admission`].
    pub fn set_cancellation_precedence(&mut self, value: bool) {
        self.cancellation_precedence = value;
    }

    /// Reject steers that arrived after admission closed but before the next
    /// turn reopens it. Without this drain a late arrival would be admitted
    /// into the following turn instead of being rejected as intended.
    pub fn drain_rejected_steers(&mut self) {
        let Some(mut receiver) = self.steering_receiver.take() else {
            return;
        };
        while let Ok(message) = receiver.try_recv() {
            if let SteeringMessage::FollowUpInput(steer) = message {
                steer.reject(SteerRejection::TurnClosing);
            }
        }
        self.steering_receiver = Some(receiver);
    }

    /// The single authoritative admission point for a live steer.
    ///
    /// Checks the admission gate and capacity, queues the intent with a fresh
    /// identity, and resolves the receipt with exactly one terminal outcome.
    /// Returns the admitted intent so callers can apply it to model-facing
    /// history; `None` means the steer was rejected and the sender must fall
    /// back to its own queue.
    pub fn admit_follow_up_steer(&mut self, steer: FollowUpSteer) -> Option<QueuedFollowUpIntent> {
        let FollowUpSteer { text, mut receipt } = steer;
        let reject = |reason: SteerRejection, receipt: &mut Option<tokio::sync::oneshot::Sender<SteerOutcome>>| {
            if let Some(receipt) = receipt.take() {
                let _ = receipt.send(SteerOutcome::Rejected { reason });
            }
            None
        };
        if self.admission_closed {
            return reject(SteerRejection::TurnClosing, &mut receipt);
        }
        if self.cancellation_precedence {
            return reject(SteerRejection::CancellationPrecedence, &mut receipt);
        }
        let intent = QueuedFollowUpIntent::new(text);
        if let Err(error) = self.try_queue_follow_up_intent(intent.clone()) {
            tracing::warn!(%error, "Rejected live steer: follow-up queue full");
            return reject(SteerRejection::QueueFull, &mut receipt);
        }
        if let Some(receipt) = receipt {
            let _ = receipt.send(SteerOutcome::Accepted { intent_id: intent.id().to_string() });
        }
        Some(intent)
    }

    #[must_use]
    pub fn has_pending_follow_up_inputs(&self) -> bool {
        !self.queued_follow_up_inputs.is_empty()
            || !self.overflow_follow_up_inputs.is_empty()
            || self.recovered_follow_up.is_some()
    }

    /// Dequeue the next follow-up user input, if any are pending.
    pub fn pop_follow_up_input(&mut self) -> Option<String> {
        self.pop_follow_up_intent().map(|intent| intent.into_parts().1)
    }

    /// Dequeue the next identified follow-up intent.
    ///
    /// Refills the primary FIFO from overflow first so an empty primary with
    /// parked overflow still yields (`has_pending_follow_up_inputs` staying
    /// true must not race a `None` pop).
    pub fn pop_follow_up_intent(&mut self) -> Option<QueuedFollowUpIntent> {
        self.drain_overflow_into_primary();
        self.queued_follow_up_inputs
            .pop_front()
            .or_else(|| self.recovered_follow_up.take())
    }

    /// Move overflow-parked intents into free primary-FIFO slots.
    fn drain_overflow_into_primary(&mut self) {
        while self.queued_follow_up_inputs.len() < MAX_QUEUED_FOLLOW_UP_INTENTS
            && let Some(intent) = self.overflow_follow_up_inputs.pop_front()
        {
            self.queued_follow_up_inputs.push_back(intent);
        }
        if self.overflow_follow_up_inputs.is_empty()
            && self.queued_follow_up_inputs.len() < MAX_QUEUED_FOLLOW_UP_INTENTS
            && let Some(intent) = self.recovered_follow_up.take()
        {
            self.queued_follow_up_inputs.push_back(intent);
        }
    }

    /// Queue a follow-up user input. The boolean is `false` when the FIFO is
    /// full; callers that need the error detail should use the `try_` variant.
    pub fn queue_follow_up_input(&mut self, input: String) -> bool {
        match self.try_queue_follow_up_input(input) {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(%error, "Rejected follow-up steering intent");
                false
            }
        }
    }

    /// Queue a follow-up user input and return an explicit overflow error.
    pub fn try_queue_follow_up_input(&mut self, input: String) -> Result<(), FollowUpQueueFull> {
        self.try_queue_follow_up_intent(QueuedFollowUpIntent::new(input))
    }

    /// Queue a harness-generated continuation or recovery instruction.
    pub fn try_queue_synthetic_follow_up_input(&mut self, input: String) -> Result<(), FollowUpQueueFull> {
        self.try_queue_follow_up_intent(QueuedFollowUpIntent::new_with_provenance(input, FollowUpProvenance::Synthetic))
    }

    /// Queue a recovered intent without changing its stable identity.
    ///
    /// Primary FIFO first; when it is full the intent parks in a bounded
    /// overflow buffer so rapid influx is preserved rather than dropped.
    /// Only when both buffers are full does the call fail.
    pub fn try_queue_follow_up_intent(&mut self, intent: QueuedFollowUpIntent) -> Result<(), FollowUpQueueFull> {
        while self.queued_follow_up_inputs.len() + self.in_flight_follow_up_intents.len() < MAX_QUEUED_FOLLOW_UP_INTENTS
            && let Some(older) = self.overflow_follow_up_inputs.pop_front()
        {
            self.queued_follow_up_inputs.push_back(older);
        }
        if self.overflow_follow_up_inputs.is_empty()
            && self.queued_follow_up_inputs.len() + self.in_flight_follow_up_intents.len()
                < MAX_QUEUED_FOLLOW_UP_INTENTS
            && let Some(recovered) = self.recovered_follow_up.take()
        {
            self.queued_follow_up_inputs.push_back(recovered);
        } else if self.overflow_follow_up_inputs.len() < MAX_OVERFLOW_FOLLOW_UP_INTENTS
            && let Some(recovered) = self.recovered_follow_up.take()
        {
            self.overflow_follow_up_inputs.push_back(recovered);
        }
        if self.recovered_follow_up.is_some() {
            return Err(FollowUpQueueFull {
                capacity: MAX_QUEUED_FOLLOW_UP_INTENTS + MAX_OVERFLOW_FOLLOW_UP_INTENTS,
            });
        }
        if self.queued_follow_up_inputs.len() + self.in_flight_follow_up_intents.len() < MAX_QUEUED_FOLLOW_UP_INTENTS {
            self.queued_follow_up_inputs.push_back(intent);
            return Ok(());
        }
        if self.overflow_follow_up_inputs.len() < MAX_OVERFLOW_FOLLOW_UP_INTENTS {
            self.overflow_follow_up_inputs.push_back(intent);
            return Ok(());
        }
        Err(FollowUpQueueFull {
            capacity: MAX_QUEUED_FOLLOW_UP_INTENTS + MAX_OVERFLOW_FOLLOW_UP_INTENTS,
        })
    }

    /// Reserve one additional slot for an interrupted, already accepted input.
    pub fn try_queue_recovered_follow_up(&mut self, intent: QueuedFollowUpIntent) -> Result<(), FollowUpQueueFull> {
        if self.recovered_follow_up.is_some() {
            return Err(FollowUpQueueFull { capacity: MAX_PENDING_FOLLOW_UP_INTENTS });
        }
        match self.try_queue_follow_up_intent(intent.clone()) {
            Ok(()) => Ok(()),
            Err(_) => {
                self.recovered_follow_up = Some(intent);
                Ok(())
            }
        }
    }

    /// Mark an intent as applied after its tagged user message is stored.
    pub fn acknowledge_follow_up_intent(&mut self, intent_id: impl Into<String>) {
        if self.applied_follow_up_intent_ids.len() >= MAX_APPLIED_FOLLOW_UP_INTENT_IDS {
            self.applied_follow_up_intent_ids.pop_front();
        }
        self.applied_follow_up_intent_ids.push_back(intent_id.into());
    }

    #[must_use]
    pub fn applied_follow_up_intent_ids(&self) -> &VecDeque<String> {
        &self.applied_follow_up_intent_ids
    }

    /// Primary-FIFO view of pending follow-ups (excludes overflow park and
    /// in-flight). Prefer [`Self::pending_follow_up_intents_snapshot`] when
    /// the complete unwritten set matters.
    #[must_use]
    pub fn pending_follow_up_intents(&self) -> &VecDeque<QueuedFollowUpIntent> {
        &self.queued_follow_up_inputs
    }

    /// Return all accepted intents that are not yet represented by durable
    /// session history, including the intent currently being processed and
    /// any parked in the overflow buffer.
    #[must_use]
    pub fn pending_follow_up_intents_snapshot(&self) -> Vec<QueuedFollowUpIntent> {
        self.in_flight_follow_up_intents
            .iter()
            .chain(self.queued_follow_up_inputs.iter())
            .chain(self.overflow_follow_up_inputs.iter())
            .chain(self.recovered_follow_up.iter())
            .cloned()
            .collect()
    }

    /// Pop every queued follow-up intent and move it to the in-flight queue,
    /// returning the intents for immediate application to live history
    /// (mid-turn steering). Until the post-turn history checkpoint succeeds,
    /// the moved intents stay in the pending snapshot exactly like
    /// turn-boundary delivery.
    pub fn drain_follow_up_intents_to_in_flight(&mut self) -> Vec<QueuedFollowUpIntent> {
        self.drain_overflow_into_primary();
        let drained: Vec<_> = self.queued_follow_up_inputs.drain(..).collect();
        self.in_flight_follow_up_intents.extend(drained.iter().cloned());
        drained
    }

    /// Mark intents whose tagged user messages have been durably checkpointed
    /// as applied. Until this is called, the intents remain in the pending
    /// snapshot so a crash cannot lose them between history and envelope IO.
    pub fn acknowledge_durable_follow_up_intents(&mut self) {
        while let Some(intent) = self.in_flight_follow_up_intents.pop_front() {
            self.acknowledge_follow_up_intent(intent.id());
        }
    }

    /// Release intents after their tagged messages have been applied when no
    /// history archive exists. This frees FIFO capacity for the in-process
    /// session without adding applied IDs that could falsely imply crash
    /// durability.
    pub fn release_in_flight_follow_up_intents_without_persistence(&mut self) {
        self.in_flight_follow_up_intents.clear();
    }

    pub fn clear_pending_follow_up_inputs(&mut self) {
        self.queued_follow_up_inputs.clear();
        self.overflow_follow_up_inputs.clear();
        self.recovered_follow_up = None;
        self.in_flight_follow_up_intents.clear();
    }

    /// Poll the steering channel for control signals during the current turn.
    pub async fn poll_turn_control(&mut self) -> RuntimeControl {
        self.poll_control().await
    }

    /// Poll the steering channel for control signals during tool execution.
    pub async fn poll_tool_control(&mut self) -> RuntimeControl {
        self.poll_control().await
    }

    async fn poll_control(&mut self) -> RuntimeControl {
        let mut paused = false;

        loop {
            let Some(receiver) = self.steering_receiver.as_mut() else {
                return if paused {
                    RuntimeControl::Resumed
                } else {
                    RuntimeControl::Continue
                };
            };

            match receiver.try_recv() {
                Ok(SteeringMessage::SteerStop) => return RuntimeControl::StopRequested,
                Ok(SteeringMessage::Pause) => {
                    paused = true;
                    if matches!(self.wait_for_resume().await, RuntimeControl::StopRequested) {
                        return RuntimeControl::StopRequested;
                    }
                }
                Ok(SteeringMessage::Resume) => {
                    paused = true;
                }
                Ok(SteeringMessage::FollowUpInput(steer)) => {
                    let _ = self.admit_follow_up_steer(steer);
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => {
                    return if paused {
                        RuntimeControl::Resumed
                    } else {
                        RuntimeControl::Continue
                    };
                }
            }
        }
    }

    async fn wait_for_resume(&mut self) -> RuntimeControl {
        loop {
            let Some(receiver) = self.steering_receiver.as_mut() else {
                return RuntimeControl::Continue;
            };

            match receiver.recv().await {
                Some(SteeringMessage::Resume) => return RuntimeControl::Continue,
                Some(SteeringMessage::SteerStop) => return RuntimeControl::StopRequested,
                Some(SteeringMessage::FollowUpInput(steer)) => {
                    let _ = self.admit_follow_up_steer(steer);
                }
                Some(SteeringMessage::Pause) => {}
                None => return RuntimeControl::Continue,
            }
        }
    }
}
