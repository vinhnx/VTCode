use std::sync::Arc;

use agent_client_protocol as acp;
use agent_client_protocol::{Client, Error as AcpError};
use async_trait::async_trait;
use serde_json::Value;
use tracing::{error, warn};

use crate::acp::reports::{
    TOOL_PERMISSION_ALLOW_ALWAYS_OPTION_ID, TOOL_PERMISSION_ALLOW_OPTION_ID,
    TOOL_PERMISSION_ALLOW_PREFIX, TOOL_PERMISSION_CANCELLED_MESSAGE,
    TOOL_PERMISSION_DENIED_MESSAGE, TOOL_PERMISSION_DENY_ALWAYS_OPTION_ID,
    TOOL_PERMISSION_DENY_OPTION_ID, TOOL_PERMISSION_DENY_PREFIX,
    TOOL_PERMISSION_REQUEST_FAILURE_LOG, TOOL_PERMISSION_REQUEST_FAILURE_MESSAGE,
    TOOL_PERMISSION_UNKNOWN_OPTION_LOG, ToolExecutionReport,
};

use super::tooling::{SupportedTool, ToolDescriptor, ToolRegistryProvider};

#[async_trait(?Send)]
pub trait AcpPermissionPrompter {
    fn permission_options(
        &self,
        tool: SupportedTool,
        args: Option<&Value>,
    ) -> Vec<acp::PermissionOption>;

    async fn request_tool_permission(
        &self,
        client: &dyn Client,
        session_id: &acp::SessionId,
        call: &acp::ToolCall,
        tool: SupportedTool,
        args: &Value,
    ) -> Result<Option<ToolExecutionReport>, AcpError>;
}

pub struct DefaultPermissionPrompter<P> {
    registry: P,
}

impl<P> DefaultPermissionPrompter<P>
where
    P: ToolRegistryProvider,
{
    pub fn new(registry: P) -> Self {
        Self { registry }
    }

    fn render_action_label(&self, tool: SupportedTool, args: Option<&Value>) -> String {
        if let Some(arguments) = args {
            self.registry
                .render_title(ToolDescriptor::Acp(tool), tool.function_name(), arguments)
        } else {
            tool.default_title().to_string()
        }
    }
}

#[async_trait(?Send)]
impl<P> AcpPermissionPrompter for DefaultPermissionPrompter<P>
where
    P: ToolRegistryProvider,
{
    fn permission_options(
        &self,
        tool: SupportedTool,
        args: Option<&Value>,
    ) -> Vec<acp::PermissionOption> {
        let action_label = self.render_action_label(tool, args);

        let allow_once_option = acp::PermissionOption::new(
            acp::PermissionOptionId::from(Arc::from(TOOL_PERMISSION_ALLOW_OPTION_ID)),
            format!("{TOOL_PERMISSION_ALLOW_PREFIX} {action_label} once"),
            acp::PermissionOptionKind::AllowOnce,
        );

        let allow_always_option = acp::PermissionOption::new(
            acp::PermissionOptionId::from(Arc::from(TOOL_PERMISSION_ALLOW_ALWAYS_OPTION_ID)),
            format!("{TOOL_PERMISSION_ALLOW_PREFIX} {action_label} always"),
            acp::PermissionOptionKind::AllowAlways,
        );

        let deny_once_option = acp::PermissionOption::new(
            acp::PermissionOptionId::from(Arc::from(TOOL_PERMISSION_DENY_OPTION_ID)),
            format!("{TOOL_PERMISSION_DENY_PREFIX} {action_label} once"),
            acp::PermissionOptionKind::RejectOnce,
        );

        let deny_always_option = acp::PermissionOption::new(
            acp::PermissionOptionId::from(Arc::from(TOOL_PERMISSION_DENY_ALWAYS_OPTION_ID)),
            format!("{TOOL_PERMISSION_DENY_PREFIX} {action_label} always"),
            acp::PermissionOptionKind::RejectAlways,
        );

        vec![
            allow_once_option,
            allow_always_option,
            deny_once_option,
            deny_always_option,
        ]
    }

    async fn request_tool_permission(
        &self,
        client: &dyn Client,
        session_id: &acp::SessionId,
        call: &acp::ToolCall,
        tool: SupportedTool,
        args: &Value,
    ) -> Result<Option<ToolExecutionReport>, AcpError> {
        let fields = acp::ToolCallUpdateFields::default()
            .title(call.title.clone())
            .kind(tool.kind())
            .status(acp::ToolCallStatus::Pending)
            .raw_input(args.clone());

        let request = acp::RequestPermissionRequest::new(
            session_id.clone(),
            acp::ToolCallUpdate::new(call.tool_call_id.clone(), fields),
            self.permission_options(tool, Some(args)),
        );

        match client.request_permission(request).await {
            Ok(response) => match response.outcome {
                acp::RequestPermissionOutcome::Cancelled => Ok(Some(ToolExecutionReport::failure(
                    tool.function_name(),
                    TOOL_PERMISSION_CANCELLED_MESSAGE,
                ))),
                acp::RequestPermissionOutcome::Selected(outcome) => {
                    let option_id_str = outcome.option_id.0.as_ref();
                    if option_id_str == TOOL_PERMISSION_ALLOW_OPTION_ID
                        || option_id_str == TOOL_PERMISSION_ALLOW_ALWAYS_OPTION_ID
                    {
                        Ok(None)
                    } else if option_id_str == TOOL_PERMISSION_DENY_OPTION_ID
                        || option_id_str == TOOL_PERMISSION_DENY_ALWAYS_OPTION_ID
                    {
                        Ok(Some(ToolExecutionReport::failure(
                            tool.function_name(),
                            TOOL_PERMISSION_DENIED_MESSAGE,
                        )))
                    } else {
                        warn!("{}", TOOL_PERMISSION_UNKNOWN_OPTION_LOG);
                        Ok(Some(ToolExecutionReport::failure(
                            tool.function_name(),
                            TOOL_PERMISSION_DENIED_MESSAGE,
                        )))
                    }
                }
                _ => Ok(Some(ToolExecutionReport::failure(
                    tool.function_name(),
                    TOOL_PERMISSION_DENIED_MESSAGE,
                ))),
            },
            Err(error) => {
                error!(%error, "{}", TOOL_PERMISSION_REQUEST_FAILURE_LOG);
                Ok(Some(ToolExecutionReport::failure(
                    tool.function_name(),
                    TOOL_PERMISSION_REQUEST_FAILURE_MESSAGE,
                )))
            }
        }
    }
}
