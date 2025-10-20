use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use agent_client_protocol as acp;
use serde_json::Value;
use vtcode::acp::permissions::{AcpPermissionPrompter, DefaultPermissionPrompter};
use vtcode::acp::reports::{
    TOOL_PERMISSION_ALLOW_OPTION_ID, TOOL_PERMISSION_CANCELLED_MESSAGE,
    TOOL_PERMISSION_DENIED_MESSAGE, TOOL_PERMISSION_DENY_OPTION_ID,
    TOOL_PERMISSION_REQUEST_FAILURE_MESSAGE,
};
use vtcode::acp::tooling::{
    SupportedTool, TOOL_LIST_FILES_MODE_ARG, TOOL_LIST_FILES_PATH_ARG, TOOL_READ_FILE_PATH_ARG,
    TOOL_READ_FILE_URI_ARG, ToolDescriptor, ToolRegistryProvider,
};

#[path = "acp_fixtures.rs"]
mod acp_fixtures;

use acp_fixtures::{list_files_permission, read_file_permission};

enum FakeOutcome {
    Allow,
    Deny,
    Cancel,
    Error(acp::Error),
}

struct FakeClient {
    outcome: FakeOutcome,
    requests: Arc<Mutex<Vec<acp::RequestPermissionRequest>>>,
}

impl FakeClient {
    fn new(outcome: FakeOutcome) -> Self {
        Self {
            outcome,
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn recorded_requests(&self) -> Vec<acp::RequestPermissionRequest> {
        self.requests.lock().expect("request log poisoned").clone()
    }
}

#[async_trait::async_trait(?Send)]
impl acp::Client for FakeClient {
    async fn request_permission(
        &self,
        args: acp::RequestPermissionRequest,
    ) -> Result<acp::RequestPermissionResponse, acp::Error> {
        self.requests
            .lock()
            .expect("request log poisoned")
            .push(args.clone());

        match &self.outcome {
            FakeOutcome::Allow => Ok(acp::RequestPermissionResponse {
                outcome: acp::RequestPermissionOutcome::Selected {
                    option_id: acp::PermissionOptionId(Arc::from(TOOL_PERMISSION_ALLOW_OPTION_ID)),
                },
                meta: None,
            }),
            FakeOutcome::Deny => Ok(acp::RequestPermissionResponse {
                outcome: acp::RequestPermissionOutcome::Selected {
                    option_id: acp::PermissionOptionId(Arc::from(TOOL_PERMISSION_DENY_OPTION_ID)),
                },
                meta: None,
            }),
            FakeOutcome::Cancel => Ok(acp::RequestPermissionResponse {
                outcome: acp::RequestPermissionOutcome::Cancelled,
                meta: None,
            }),
            FakeOutcome::Error(error) => Err(error.clone()),
        }
    }

    async fn session_notification(
        &self,
        _args: acp::SessionNotification,
    ) -> Result<(), acp::Error> {
        Ok(())
    }
}

#[derive(Clone)]
struct FakeRegistry {
    descriptors: HashMap<String, ToolDescriptor>,
}

impl FakeRegistry {
    fn new() -> Self {
        let mut descriptors = HashMap::new();
        descriptors.insert(
            SupportedTool::ReadFile.function_name().to_string(),
            ToolDescriptor::Acp(SupportedTool::ReadFile),
        );
        descriptors.insert(
            SupportedTool::ListFiles.function_name().to_string(),
            ToolDescriptor::Acp(SupportedTool::ListFiles),
        );

        Self { descriptors }
    }

    fn render_read_file(&self, args: &Value) -> String {
        args.get(TOOL_READ_FILE_PATH_ARG)
            .or_else(|| args.get(TOOL_READ_FILE_URI_ARG))
            .and_then(Value::as_str)
            .map(|path| format!("Read file {path}"))
            .unwrap_or_else(|| "Read file".to_string())
    }

    fn render_list_files(&self, args: &Value) -> String {
        let scope = args
            .get(TOOL_LIST_FILES_PATH_ARG)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(|value| format!("{value}/"))
            .unwrap_or_else(|| "workspace".to_string());

        let mode = args
            .get(TOOL_LIST_FILES_MODE_ARG)
            .and_then(Value::as_str)
            .unwrap_or("list");

        format!("List files in {scope} ({mode})")
    }
}

impl ToolRegistryProvider for FakeRegistry {
    fn registered_tools(&self) -> Vec<SupportedTool> {
        self.descriptors
            .values()
            .filter_map(|descriptor| match descriptor {
                ToolDescriptor::Acp(tool) => Some(*tool),
                ToolDescriptor::Local => None,
            })
            .collect()
    }

    fn definitions_for(
        &self,
        _enabled_tools: &[SupportedTool],
        _include_local: bool,
    ) -> Vec<vtcode_core::llm::provider::ToolDefinition> {
        Vec::new()
    }

    fn render_title(
        &self,
        descriptor: ToolDescriptor,
        _function_name: &str,
        args: &Value,
    ) -> String {
        match descriptor {
            ToolDescriptor::Acp(SupportedTool::ReadFile) => self.render_read_file(args),
            ToolDescriptor::Acp(SupportedTool::ListFiles) => self.render_list_files(args),
            ToolDescriptor::Local => "Local tool".to_string(),
        }
    }

    fn lookup(&self, function_name: &str) -> Option<ToolDescriptor> {
        self.descriptors.get(function_name).copied()
    }

    fn local_definition(
        &self,
        _tool_name: &str,
    ) -> Option<vtcode_core::llm::provider::ToolDefinition> {
        None
    }

    fn has_local_tools(&self) -> bool {
        false
    }
}

fn prompter() -> DefaultPermissionPrompter<FakeRegistry> {
    DefaultPermissionPrompter::new(FakeRegistry::new())
}

#[tokio::test]
async fn permission_allow_flow_returns_none() {
    let fixture = read_file_permission();
    let client = FakeClient::new(FakeOutcome::Allow);
    let prompter = prompter();

    let report = prompter
        .request_tool_permission(
            &client,
            &fixture.session_id,
            &fixture.tool_call,
            SupportedTool::ReadFile,
            &fixture.arguments,
        )
        .await
        .expect("permission request should succeed");

    assert!(report.is_none(), "allowed flow must not short-circuit");

    let requests = client.recorded_requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.session_id, fixture.session_id);
    assert_eq!(request.options.len(), 2);
    let option_ids: Vec<_> = request
        .options
        .iter()
        .map(|option| option.id.0.as_ref().to_string())
        .collect();
    assert!(option_ids.contains(&TOOL_PERMISSION_ALLOW_OPTION_ID.to_string()));
    assert!(option_ids.contains(&TOOL_PERMISSION_DENY_OPTION_ID.to_string()));

    assert!(
        request.options.iter().any(|option| {
            option.name.contains("Read file ") && option.name.starts_with("Allow")
        })
    );
}

#[tokio::test]
async fn permission_denied_flow_returns_failure_report() {
    let fixture = read_file_permission();
    let client = FakeClient::new(FakeOutcome::Deny);
    let prompter = prompter();

    let report = prompter
        .request_tool_permission(
            &client,
            &fixture.session_id,
            &fixture.tool_call,
            SupportedTool::ReadFile,
            &fixture.arguments,
        )
        .await
        .expect("permission request should return a report")
        .expect("denied flow should produce a tool report");

    assert_eq!(report.status, acp::ToolCallStatus::Failed);
    assert!(report.llm_response.contains(TOOL_PERMISSION_DENIED_MESSAGE));
}

#[tokio::test]
async fn permission_cancelled_flow_returns_cancel_report() {
    let fixture = list_files_permission();
    let client = FakeClient::new(FakeOutcome::Cancel);
    let prompter = prompter();

    let report = prompter
        .request_tool_permission(
            &client,
            &fixture.session_id,
            &fixture.tool_call,
            SupportedTool::ListFiles,
            &fixture.arguments,
        )
        .await
        .expect("permission request should succeed")
        .expect("cancelled flow should produce a report");

    assert_eq!(report.status, acp::ToolCallStatus::Failed);
    assert!(
        report
            .llm_response
            .contains(TOOL_PERMISSION_CANCELLED_MESSAGE)
    );
}

#[tokio::test]
async fn permission_failure_flow_returns_error_report() {
    let fixture = read_file_permission();
    let client = FakeClient::new(FakeOutcome::Error(acp::Error::internal_error()));
    let prompter = prompter();

    let report = prompter
        .request_tool_permission(
            &client,
            &fixture.session_id,
            &fixture.tool_call,
            SupportedTool::ReadFile,
            &fixture.arguments,
        )
        .await
        .expect("permission request should resolve")
        .expect("failed transport should produce a report");

    assert_eq!(report.status, acp::ToolCallStatus::Failed);
    assert!(
        report
            .llm_response
            .contains(TOOL_PERMISSION_REQUEST_FAILURE_MESSAGE)
    );
}
