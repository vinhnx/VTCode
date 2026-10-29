//! Track pending interaction identities and reject stale or duplicate replies.
use std::collections::HashMap;

use super::{InteractionRequest, InteractionResponse, ProtocolError};

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

    /// Validate a response without consuming the pending interaction.
    pub fn validate(&self, response: &InteractionResponse) -> Result<(), ProtocolError> {
        let Some((session_id, turn_id)) = self.pending.get(&response.interaction_id) else {
            return Err(ProtocolError::UnknownInteraction);
        };
        if session_id != &response.session_id || turn_id != &response.turn_id {
            return Err(ProtocolError::StaleInteraction);
        }
        Ok(())
    }

    /// Validate and consume exactly one matching response.
    pub fn resolve(&mut self, response: &InteractionResponse) -> Result<(), ProtocolError> {
        self.validate(response)?;
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
