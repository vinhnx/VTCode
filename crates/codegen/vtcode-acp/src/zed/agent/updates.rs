use super::super::helpers::build_available_commands;
use super::super::types::PlanProgress;
use super::ZedAgent;
use crate::acp;
use crate::acp::Error as SdkError;
use crate::zed::connection::ConnectionHandle;
use anyhow::Result;
use vtcode_core::prompts::discover_prompt_templates;
use vtcode_core::ui::slash::visible_commands;

impl ZedAgent {
    pub(super) async fn send_update(
        &self,
        session_id: &acp::SessionId,
        update: acp::SessionUpdate,
    ) -> Result<(), SdkError> {
        let Some(client) = self.client() else {
            return Err(SdkError::internal_error());
        };
        let notification = acp::SessionNotification::new(session_id.clone(), update);
        ConnectionHandle::send_session_notification(&client, notification)
    }

    pub(super) async fn send_plan_update(
        &self,
        session_id: &acp::SessionId,
        plan: &PlanProgress,
    ) -> Result<(), SdkError> {
        if !plan.has_entries() {
            return Ok(());
        }

        self.send_update(session_id, acp::SessionUpdate::Plan(plan.to_plan()))
            .await
    }

    pub(super) async fn send_available_commands_update(
        &self,
        session_id: &acp::SessionId,
    ) -> Result<(), SdkError> {
        let slash_commands = visible_commands();
        let prompt_templates = discover_prompt_templates(&self.config.workspace).await;
        let available_commands = build_available_commands(&slash_commands, &prompt_templates);

        tracing::debug!(
            session_id = %session_id,
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

    pub(super) async fn advance_plan_to_response(
        &self,
        session_id: &acp::SessionId,
        plan: &mut PlanProgress,
    ) -> Result<(), acp::Error> {
        if plan.has_context_step() && !plan.context_completed() && plan.complete_context() {
            self.send_plan_update(session_id, plan).await?;
        }
        if plan.start_response() {
            self.send_plan_update(session_id, plan).await?;
        }

        Ok(())
    }
}
