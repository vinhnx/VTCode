use serde::{Deserialize, Serialize};

/// Messages used to steer the agent's execution loop from an external source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SteeringMessage {
    /// Stop the agent's execution loop immediately (Steering).
    SteerStop,
    /// Pause the agent's execution loop.
    Pause,
    /// Resume the agent's execution loop.
    Resume,
    /// Inject input as a follow-up message after the current turn ends (FollowUp).
    FollowUpInput(String),
}
