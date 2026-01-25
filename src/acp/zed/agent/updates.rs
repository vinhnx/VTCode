use super::ZedAgent;
use agent_client_protocol as acp;
use anyhow::Result;
use tokio::sync::oneshot;
use super::super::helpers::build_available_commands;
use super::super::types::{NotificationEnvelope, PlanProgress};

impl ZedAgent {
    pub(super) async fn send_update(
        &self,
        session_id: &acp::SessionId,
        update: acp::SessionUpdate,
    ) -> Result<(), acp::Error> {
        let (completion, completion_rx) = oneshot::channel();
        let notification = acp::SessionNotification {
            session_id: session_id.clone(),
            update,
            meta: None,
        };

        self.session_update_tx
            .send(NotificationEnvelope {
                notification,
                completion,
            })
            .map_err(|_| acp::Error::internal_error())?;

        completion_rx
            .await
            .map_err(|_| acp::Error::internal_error())
    }

    pub(super) async fn send_plan_update(
        &self,
        session_id: &acp::SessionId,
        plan: &PlanProgress,
    ) -> Result<(), acp::Error> {
        if !plan.has_entries() {
            return Ok(());
        }

        self.send_update(session_id, acp::SessionUpdate::Plan(plan.to_plan()))
            .await
    }

    pub(super) async fn send_available_commands_update(
        &self,
        session_id: &acp::SessionId,
    ) -> Result<(), acp::Error> {
        let available_commands = build_available_commands();
        self.send_update(
            session_id,
            acp::SessionUpdate::AvailableCommandsUpdate(acp::AvailableCommandsUpdate {
                available_commands,
                meta: None,
            }),
        )
        .await
    }
}
