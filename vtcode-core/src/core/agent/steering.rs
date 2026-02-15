use serde::{Deserialize, Serialize};

/// Messages used to steer the agent's execution loop from an external source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SteeringMessage {
    /// Stop the agent's execution loop immediately.
    Stop,
    /// Pause the agent's execution loop.
    Pause,
    /// Resume the agent's execution loop.
    Resume,
    /// Inject input into the agent's context as if it were a user message.
    InjectInput(String),
}
