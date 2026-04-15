use super::super::helpers::build_available_commands;
use super::super::types::{NotificationEnvelope, PlanProgress};
use super::ZedAgent;
use agent_client_protocol as acp;
use anyhow::Result;
use tokio::sync::oneshot;
use vtcode_core::prompts::discover_prompt_templates;
use vtcode_core::ui::slash::visible_commands;

impl ZedAgent {
    pub(super) async fn send_update(
        &self,
        session_id: &acp::SessionId,
        update: acp::SessionUpdate,
    ) -> Result<(), acp::Error> {
        let (completion, completion_rx) = oneshot::channel();
        let notification = acp::SessionNotification::new(session_id.clone(), update);

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
        let slash_commands = visible_commands();
        let prompt_templates = discover_prompt_templates(&self.config.workspace).await;
        let available_commands = build_available_commands(&slash_commands, &prompt_templates);

        tracing::debug!(
            session_id = %session_id.0,
            command_count = available_commands.len(),
            slash_command_count = slash_commands.len(),
            template_count = prompt_templates.len(),
            "Sending available commands update to ACP client"
        );

        self.send_update(
            session_id,
            acp::SessionUpdate::AvailableCommandsUpdate(acp::AvailableCommandsUpdate::new(
                available_commands,
            )),
        )
        .await
    }
}
