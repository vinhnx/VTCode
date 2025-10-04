use agent_client_protocol as acp;
use agent_client_protocol::{AgentSideConnection, Client};
use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::StreamExt;
use percent_encoding::percent_decode_str;
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::mem::discriminant;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::{error, warn};
use url::Url;

use vtcode_core::config::AgentClientProtocolZedConfig;
use vtcode_core::config::constants::tools;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::llm::factory::{create_provider_for_model, create_provider_with_config};
use vtcode_core::llm::provider::{
    FinishReason, LLMRequest, LLMStreamEvent, Message, ToolCall as ProviderToolCall, ToolChoice,
    ToolDefinition,
};
use vtcode_core::prompts::read_system_prompt_from_md;

const SESSION_PREFIX: &str = "vtcode-zed-session";
const RESOURCE_FALLBACK_LABEL: &str = "Resource";
const RESOURCE_FAILURE_LABEL: &str = "Resource unavailable";
const RESOURCE_CONTEXT_OPEN: &str = "<context";
const RESOURCE_CONTEXT_CLOSE: &str = "</context>";
const RESOURCE_CONTEXT_URI_ATTR: &str = "uri";
const RESOURCE_CONTEXT_NAME_ATTR: &str = "name";
const TOOL_READ_FILE_DESCRIPTION: &str =
    "Read the contents of a text file accessible to the Zed workspace";
const TOOL_READ_FILE_URI_ARG: &str = "uri";
const TOOL_READ_FILE_PATH_ARG: &str = "path";
const TOOL_READ_FILE_LINE_ARG: &str = "line";
const TOOL_READ_FILE_LIMIT_ARG: &str = "limit";
const TOOL_FAILURE_PREFIX: &str = "Tool execution failed";
const TOOL_SUCCESS_LABEL: &str = "success";
const TOOL_ERROR_LABEL: &str = "error";
const TOOL_RESPONSE_KEY_STATUS: &str = "status";
const TOOL_RESPONSE_KEY_TOOL: &str = "tool";
const TOOL_RESPONSE_KEY_PATH: &str = "path";
const TOOL_RESPONSE_KEY_CONTENT: &str = "content";
const TOOL_RESPONSE_KEY_TRUNCATED: &str = "truncated";
const TOOL_RESPONSE_KEY_MESSAGE: &str = "message";
const MAX_TOOL_RESPONSE_CHARS: usize = 32_768;
const TOOL_DISABLED_CONFIG_NOTICE: &str = "Skipping {tool} tool: disabled via [acp.zed.tools]";
const TOOL_DISABLED_PROVIDER_NOTICE: &str =
    "Skipping {tool} tool: model {model} on {provider} does not support function calling";
const TOOL_DISABLED_CAPABILITY_NOTICE: &str =
    "Skipping {tool} tool: client does not advertise fs.readTextFile capability";
const TOOL_DISABLED_CONFIG_LOG: &str = "ACP tools disabled by configuration";
const TOOL_DISABLED_PROVIDER_LOG: &str =
    "ACP tools disabled because the selected model does not support function calling";
const TOOL_DISABLED_CAPABILITY_LOG: &str =
    "ACP tools disabled because the client lacks fs.readTextFile support";
const TOOL_PERMISSION_ALLOW_OPTION_ID: &str = "allow-once";
const TOOL_PERMISSION_DENY_OPTION_ID: &str = "reject-once";
const TOOL_PERMISSION_ALLOW_PREFIX: &str = "Allow";
const TOOL_PERMISSION_DENY_PREFIX: &str = "Deny";
const TOOL_PERMISSION_DENIED_MESSAGE: &str =
    "Tool execution cancelled: permission denied by the user";
const TOOL_PERMISSION_CANCELLED_MESSAGE: &str =
    "Tool execution cancelled: permission request interrupted";
const TOOL_PERMISSION_REQUEST_FAILURE_LOG: &str =
    "Failed to request ACP tool permission, cancelling the tool invocation";
const TOOL_PERMISSION_UNKNOWN_OPTION_LOG: &str =
    "Received unsupported ACP permission option selection";
const INITIALIZE_VERSION_MISMATCH_LOG: &str =
    "Client requested unsupported ACP protocol version; responding with v1";
const TOOL_EXECUTION_CANCELLED_MESSAGE: &str = "Tool execution cancelled at the client's request";
const TOOL_PERMISSION_REQUEST_FAILURE_MESSAGE: &str =
    "Tool execution cancelled: permission request failed";
const TOOL_READ_FILE_INVALID_INTEGER_TEMPLATE: &str =
    "Invalid {argument} value: expected a positive integer";
const TOOL_READ_FILE_INTEGER_RANGE_TEMPLATE: &str = "{argument} value exceeds the supported range";
const TOOL_READ_FILE_ABSOLUTE_PATH_TEMPLATE: &str =
    "Invalid {argument} value: expected an absolute path";
const PLAN_STEP_ANALYZE: &str = "Review the latest user request and conversation context";
const PLAN_STEP_GATHER_CONTEXT: &str = "Gather referenced workspace files when required";
const PLAN_STEP_RESPOND: &str = "Compose and send the assistant response";

type SharedClient = Rc<RefCell<Option<Rc<AgentSideConnection>>>>;

enum ToolRuntime<'a> {
    Enabled,
    Disabled(ToolDisableReason<'a>),
}

enum ToolDisableReason<'a> {
    Config,
    Provider { provider: &'a str, model: &'a str },
    ClientCapabilities,
}

#[derive(Clone, Copy)]
enum SupportedTool {
    ReadFile,
}

impl SupportedTool {
    fn kind(&self) -> acp::ToolKind {
        match self {
            Self::ReadFile => acp::ToolKind::Fetch,
        }
    }

    fn default_title(&self) -> &'static str {
        match self {
            Self::ReadFile => "Read file",
        }
    }

    fn function_name(&self) -> &'static str {
        match self {
            Self::ReadFile => tools::READ_FILE,
        }
    }
}

struct ToolRegistry {
    definitions: Vec<ToolDefinition>,
    mapping: HashMap<String, SupportedTool>,
}

impl ToolRegistry {
    fn new(read_file_enabled: bool) -> Self {
        let mut definitions = Vec::new();
        let mut mapping = HashMap::new();

        if read_file_enabled {
            let read_file_schema = json!({
                "type": "object",
                "properties": {
                    TOOL_READ_FILE_PATH_ARG: {
                        "type": "string",
                        "description": "Absolute path to the file within the workspace",
                    },
                    TOOL_READ_FILE_URI_ARG: {
                        "type": "string",
                        "description": "File URI using file://, zed://, or zed-fs:// schemes",
                    },
                    TOOL_READ_FILE_LINE_ARG: {
                        "type": "integer",
                        "minimum": 1,
                        "description": "1-based line number to start reading from",
                    },
                    TOOL_READ_FILE_LIMIT_ARG: {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Maximum number of lines to read",
                    }
                },
                "additionalProperties": false,
                "anyOf": [
                    {"required": [TOOL_READ_FILE_PATH_ARG]},
                    {"required": [TOOL_READ_FILE_URI_ARG]}
                ]
            });

            let read_file = ToolDefinition::function(
                tools::READ_FILE.to_string(),
                TOOL_READ_FILE_DESCRIPTION.to_string(),
                read_file_schema,
            );
            mapping.insert(
                read_file.function_name().to_string(),
                SupportedTool::ReadFile,
            );
            definitions.push(read_file);
        }

        Self {
            definitions,
            mapping,
        }
    }

    fn is_empty(&self) -> bool {
        self.definitions.is_empty()
    }

    fn definitions(&self) -> Vec<ToolDefinition> {
        self.definitions.clone()
    }

    fn lookup(&self, name: &str) -> Option<SupportedTool> {
        self.mapping.get(name).copied()
    }

    fn render_title(&self, tool: SupportedTool, args: &Value) -> String {
        match tool {
            SupportedTool::ReadFile => {
                if let Some(path) = args
                    .get(TOOL_READ_FILE_PATH_ARG)
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                {
                    format!("Read file {path}")
                } else if let Some(uri) = args
                    .get(TOOL_READ_FILE_URI_ARG)
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                {
                    format!("Read file {uri}")
                } else {
                    tool.default_title().to_string()
                }
            }
        }
    }
}

struct ToolExecutionReport {
    status: acp::ToolCallStatus,
    llm_response: String,
    content: Vec<acp::ToolCallContent>,
    raw_output: Option<Value>,
}

struct PlanProgress {
    entries: Vec<acp::PlanEntry>,
    analyze_index: usize,
    gather_index: Option<usize>,
    respond_index: usize,
}

impl PlanProgress {
    fn new(include_context_step: bool) -> Self {
        let mut entries = Vec::new();

        let analyze_index = entries.len();
        entries.push(acp::PlanEntry {
            content: PLAN_STEP_ANALYZE.to_string(),
            priority: acp::PlanEntryPriority::High,
            status: acp::PlanEntryStatus::InProgress,
            meta: None,
        });

        let gather_index = if include_context_step {
            let index = entries.len();
            entries.push(acp::PlanEntry {
                content: PLAN_STEP_GATHER_CONTEXT.to_string(),
                priority: acp::PlanEntryPriority::Medium,
                status: acp::PlanEntryStatus::Pending,
                meta: None,
            });
            Some(index)
        } else {
            None
        };

        let respond_index = entries.len();
        entries.push(acp::PlanEntry {
            content: PLAN_STEP_RESPOND.to_string(),
            priority: acp::PlanEntryPriority::High,
            status: acp::PlanEntryStatus::Pending,
            meta: None,
        });

        Self {
            entries,
            analyze_index,
            gather_index,
            respond_index,
        }
    }

    fn has_entries(&self) -> bool {
        !self.entries.is_empty()
    }

    fn update_status(&mut self, index: usize, status: acp::PlanEntryStatus) -> bool {
        if discriminant(&self.entries[index].status) == discriminant(&status) {
            return false;
        }

        self.entries[index].status = status;
        true
    }

    fn complete_analysis(&mut self) -> bool {
        self.update_status(self.analyze_index, acp::PlanEntryStatus::Completed)
    }

    fn start_context(&mut self) -> bool {
        if let Some(index) = self.gather_index {
            if discriminant(&self.entries[index].status)
                == discriminant(&acp::PlanEntryStatus::Pending)
            {
                return self.update_status(index, acp::PlanEntryStatus::InProgress);
            }
        }
        false
    }

    fn complete_context(&mut self) -> bool {
        if let Some(index) = self.gather_index {
            if discriminant(&self.entries[index].status)
                != discriminant(&acp::PlanEntryStatus::Completed)
            {
                return self.update_status(index, acp::PlanEntryStatus::Completed);
            }
        }
        false
    }

    fn has_context_step(&self) -> bool {
        self.gather_index.is_some()
    }

    fn context_completed(&self) -> bool {
        self.gather_index
            .map(|index| {
                discriminant(&self.entries[index].status)
                    == discriminant(&acp::PlanEntryStatus::Completed)
            })
            .unwrap_or(true)
    }

    fn start_response(&mut self) -> bool {
        if discriminant(&self.entries[self.respond_index].status)
            == discriminant(&acp::PlanEntryStatus::Pending)
        {
            return self.update_status(self.respond_index, acp::PlanEntryStatus::InProgress);
        }
        false
    }

    fn complete_response(&mut self) -> bool {
        if discriminant(&self.entries[self.respond_index].status)
            != discriminant(&acp::PlanEntryStatus::Completed)
        {
            return self.update_status(self.respond_index, acp::PlanEntryStatus::Completed);
        }
        false
    }

    fn to_plan(&self) -> acp::Plan {
        acp::Plan {
            entries: self.entries.clone(),
            meta: None,
        }
    }
}

impl ToolExecutionReport {
    fn success(content: Vec<acp::ToolCallContent>, payload: Value) -> Self {
        Self {
            status: acp::ToolCallStatus::Completed,
            llm_response: payload.to_string(),
            content,
            raw_output: Some(payload),
        }
    }

    fn failure(tool_name: &str, message: &str) -> Self {
        let payload = json!({
            TOOL_RESPONSE_KEY_STATUS: TOOL_ERROR_LABEL,
            TOOL_RESPONSE_KEY_TOOL: tool_name,
            TOOL_RESPONSE_KEY_MESSAGE: message,
        });
        Self {
            status: acp::ToolCallStatus::Failed,
            llm_response: payload.to_string(),
            content: vec![acp::ToolCallContent::from(format!(
                "{TOOL_FAILURE_PREFIX}: {message}"
            ))],
            raw_output: Some(payload),
        }
    }

    fn cancelled(tool_name: &str) -> Self {
        Self::failure(tool_name, TOOL_EXECUTION_CANCELLED_MESSAGE)
    }
}

struct ToolCallResult {
    tool_call_id: String,
    llm_response: String,
}

#[derive(Clone)]
struct SessionHandle {
    data: Rc<RefCell<SessionData>>,
    cancel_flag: Rc<Cell<bool>>,
}

struct SessionData {
    messages: Vec<Message>,
    tool_notice_sent: bool,
}

struct NotificationEnvelope {
    notification: acp::SessionNotification,
    completion: oneshot::Sender<()>,
}

pub async fn run_zed_agent(
    config: &CoreAgentConfig,
    zed_config: &AgentClientProtocolZedConfig,
) -> Result<()> {
    let outgoing = tokio::io::stdout().compat_write();
    let incoming = tokio::io::stdin().compat();
    let system_prompt = read_system_prompt_from_md().unwrap_or_else(|_| String::new());

    let local_set = tokio::task::LocalSet::new();
    let config_clone = config.clone();
    let zed_config_clone = zed_config.clone();
    let client_handle: SharedClient = Rc::new(RefCell::new(None));

    local_set
        .run_until(async move {
            let (tx, mut rx) = mpsc::unbounded_channel::<NotificationEnvelope>();
            let agent = ZedAgent::new(
                config_clone,
                zed_config_clone,
                system_prompt,
                tx,
                Rc::clone(&client_handle),
            );
            let (raw_conn, io_task) =
                acp::AgentSideConnection::new(agent, outgoing, incoming, |fut| {
                    tokio::task::spawn_local(fut);
                });
            let conn = Rc::new(raw_conn);
            client_handle.replace(Some(Rc::clone(&conn)));

            let notifications = tokio::task::spawn_local(async move {
                while let Some(envelope) = rx.recv().await {
                    let result = conn.session_notification(envelope.notification).await;
                    if let Err(error) = result {
                        error!(%error, "Failed to forward ACP session notification");
                    }
                    let _ = envelope.completion.send(());
                }
            });

            let io_result = io_task.await;
            notifications.abort();
            io_result
        })
        .await
        .context("ACP stdio bridge task failed")?;

    Ok(())
}

struct ZedAgent {
    config: CoreAgentConfig,
    zed_config: AgentClientProtocolZedConfig,
    system_prompt: String,
    sessions: Rc<RefCell<HashMap<acp::SessionId, SessionHandle>>>,
    next_session_id: Cell<u64>,
    session_update_tx: mpsc::UnboundedSender<NotificationEnvelope>,
    client: SharedClient,
    tool_registry: ToolRegistry,
    client_capabilities: Rc<RefCell<Option<acp::ClientCapabilities>>>,
}

impl ZedAgent {
    fn new(
        config: CoreAgentConfig,
        zed_config: AgentClientProtocolZedConfig,
        system_prompt: String,
        session_update_tx: mpsc::UnboundedSender<NotificationEnvelope>,
        client: SharedClient,
    ) -> Self {
        let read_file_enabled = zed_config.tools.read_file;

        Self {
            config,
            zed_config,
            system_prompt,
            sessions: Rc::new(RefCell::new(HashMap::new())),
            next_session_id: Cell::new(0),
            session_update_tx,
            client,
            tool_registry: ToolRegistry::new(read_file_enabled),
            client_capabilities: Rc::new(RefCell::new(None)),
        }
    }

    fn register_session(&self) -> acp::SessionId {
        let raw_id = self.next_session_id.get();
        self.next_session_id.set(raw_id + 1);
        let session_id = acp::SessionId(Arc::from(format!("{SESSION_PREFIX}-{raw_id}")));
        let handle = SessionHandle {
            data: Rc::new(RefCell::new(SessionData {
                messages: Vec::new(),
                tool_notice_sent: false,
            })),
            cancel_flag: Rc::new(Cell::new(false)),
        };
        self.sessions
            .borrow_mut()
            .insert(session_id.clone(), handle);
        session_id
    }

    fn session_handle(&self, session_id: &acp::SessionId) -> Option<SessionHandle> {
        self.sessions.borrow().get(session_id).cloned()
    }

    fn push_message(&self, session: &SessionHandle, message: Message) {
        session.data.borrow_mut().messages.push(message);
    }

    fn should_send_tool_notice(&self, session: &SessionHandle) -> bool {
        !session.data.borrow().tool_notice_sent
    }

    fn mark_tool_notice_sent(&self, session: &SessionHandle) {
        session.data.borrow_mut().tool_notice_sent = true;
    }

    fn resolved_messages(&self, session: &SessionHandle) -> Vec<Message> {
        let mut messages = Vec::new();
        if !self.system_prompt.trim().is_empty() {
            messages.push(Message::system(self.system_prompt.clone()));
        }

        let history = session.data.borrow();
        messages.extend(history.messages.iter().cloned());
        messages
    }

    fn stop_reason_from_finish(finish: FinishReason) -> acp::StopReason {
        match finish {
            FinishReason::Stop | FinishReason::ToolCalls => acp::StopReason::EndTurn,
            FinishReason::Length => acp::StopReason::MaxTokens,
            FinishReason::ContentFilter | FinishReason::Error(_) => acp::StopReason::Refusal,
        }
    }

    fn client(&self) -> Option<Rc<AgentSideConnection>> {
        self.client.borrow().as_ref().map(Rc::clone)
    }

    fn tool_definitions(&self, enabled: bool) -> Option<Vec<ToolDefinition>> {
        if enabled && !self.tool_registry.is_empty() {
            Some(self.tool_registry.definitions())
        } else {
            None
        }
    }

    fn tool_choice(&self, enabled: bool) -> Option<ToolChoice> {
        if enabled && !self.tool_registry.is_empty() {
            Some(ToolChoice::auto())
        } else {
            Some(ToolChoice::none())
        }
    }

    fn client_supports_read_text_file(&self) -> bool {
        self.client_capabilities
            .borrow()
            .as_ref()
            .map(|capabilities| capabilities.fs.read_text_file)
            .unwrap_or(false)
    }

    fn truncate_text(&self, input: &str) -> (String, bool) {
        if input.chars().count() <= MAX_TOOL_RESPONSE_CHARS {
            return (input.to_string(), false);
        }

        let truncated: String = input.chars().take(MAX_TOOL_RESPONSE_CHARS).collect();
        (truncated, true)
    }

    fn argument_message(template: &str, argument: &str) -> String {
        template.replace("{argument}", argument)
    }

    fn ensure_absolute_path(path: PathBuf, argument: &str) -> Result<PathBuf, String> {
        if path.is_absolute() {
            Ok(path)
        } else {
            Err(Self::argument_message(
                TOOL_READ_FILE_ABSOLUTE_PATH_TEMPLATE,
                argument,
            ))
        }
    }

    fn parse_positive_argument(args: &Value, key: &str) -> Result<Option<u32>, String> {
        let Some(raw_value) = args.get(key) else {
            return Ok(None);
        };

        if raw_value.is_null() {
            return Ok(None);
        }

        let Some(value) = raw_value.as_u64() else {
            return Err(Self::argument_message(
                TOOL_READ_FILE_INVALID_INTEGER_TEMPLATE,
                key,
            ));
        };

        if value == 0 {
            return Err(Self::argument_message(
                TOOL_READ_FILE_INVALID_INTEGER_TEMPLATE,
                key,
            ));
        }

        if value > u32::MAX as u64 {
            return Err(Self::argument_message(
                TOOL_READ_FILE_INTEGER_RANGE_TEMPLATE,
                key,
            ));
        }

        Ok(Some(value as u32))
    }

    fn permission_options(
        &self,
        tool: SupportedTool,
        args: Option<&Value>,
    ) -> Vec<acp::PermissionOption> {
        let action_label = match (tool, args) {
            (SupportedTool::ReadFile, Some(args)) => self.tool_registry.render_title(tool, args),
            _ => tool.default_title().to_string(),
        };

        let allow_name = format!(
            "{prefix} {action}",
            prefix = TOOL_PERMISSION_ALLOW_PREFIX,
            action = action_label.clone(),
        );
        let deny_name = format!(
            "{prefix} {action}",
            prefix = TOOL_PERMISSION_DENY_PREFIX,
            action = action_label,
        );

        let allow_option = acp::PermissionOption {
            id: acp::PermissionOptionId(Arc::from(TOOL_PERMISSION_ALLOW_OPTION_ID)),
            name: allow_name,
            kind: acp::PermissionOptionKind::AllowOnce,
            meta: None,
        };

        let deny_option = acp::PermissionOption {
            id: acp::PermissionOptionId(Arc::from(TOOL_PERMISSION_DENY_OPTION_ID)),
            name: deny_name,
            kind: acp::PermissionOptionKind::RejectOnce,
            meta: None,
        };

        vec![allow_option, deny_option]
    }

    async fn request_tool_permission(
        &self,
        client: &AgentSideConnection,
        session_id: &acp::SessionId,
        call: &acp::ToolCall,
        tool: SupportedTool,
        args: &Value,
    ) -> Result<Option<ToolExecutionReport>, acp::Error> {
        let mut fields = acp::ToolCallUpdateFields::default();
        fields.title = Some(call.title.clone());
        fields.kind = Some(tool.kind());
        fields.status = Some(acp::ToolCallStatus::Pending);
        fields.raw_input = Some(args.clone());

        let request = acp::RequestPermissionRequest {
            session_id: session_id.clone(),
            tool_call: acp::ToolCallUpdate {
                id: call.id.clone(),
                fields,
                meta: None,
            },
            options: self.permission_options(tool, Some(args)),
            meta: None,
        };

        match client.request_permission(request).await {
            Ok(response) => match response.outcome {
                acp::RequestPermissionOutcome::Cancelled => Ok(Some(ToolExecutionReport::failure(
                    tool.function_name(),
                    TOOL_PERMISSION_CANCELLED_MESSAGE,
                ))),
                acp::RequestPermissionOutcome::Selected { option_id } => {
                    let id_value = option_id.0.as_ref();
                    if id_value == TOOL_PERMISSION_ALLOW_OPTION_ID {
                        Ok(None)
                    } else if id_value == TOOL_PERMISSION_DENY_OPTION_ID {
                        Ok(Some(ToolExecutionReport::failure(
                            tool.function_name(),
                            TOOL_PERMISSION_DENIED_MESSAGE,
                        )))
                    } else {
                        warn!(
                            option = %option_id,
                            "{}",
                            TOOL_PERMISSION_UNKNOWN_OPTION_LOG
                        );
                        Ok(Some(ToolExecutionReport::failure(
                            tool.function_name(),
                            TOOL_PERMISSION_DENIED_MESSAGE,
                        )))
                    }
                }
            },
            Err(error) => {
                error!(
                    %error,
                    tool = tool.function_name(),
                    "{}",
                    TOOL_PERMISSION_REQUEST_FAILURE_LOG
                );
                let failure_message = format!("{TOOL_PERMISSION_REQUEST_FAILURE_MESSAGE}: {error}");
                Ok(Some(ToolExecutionReport::failure(
                    tool.function_name(),
                    &failure_message,
                )))
            }
        }
    }

    fn parse_tool_path(&self, args: &Value) -> Result<PathBuf, String> {
        if let Some(path) = args
            .get(TOOL_READ_FILE_PATH_ARG)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            let candidate = PathBuf::from(path);
            return Self::ensure_absolute_path(candidate, TOOL_READ_FILE_PATH_ARG);
        }

        if let Some(uri) = args
            .get(TOOL_READ_FILE_URI_ARG)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            return Self::parse_resource_path(uri);
        }

        Err(format!(
            "{TOOL_FAILURE_PREFIX}: missing {TOOL_READ_FILE_PATH_ARG} or {TOOL_READ_FILE_URI_ARG}"
        ))
    }

    async fn execute_tool_calls(
        &self,
        session: &SessionHandle,
        session_id: &acp::SessionId,
        calls: &[ProviderToolCall],
    ) -> Result<Vec<ToolCallResult>, acp::Error> {
        if calls.is_empty() {
            return Ok(Vec::new());
        }

        let Some(client) = self.client() else {
            return Ok(calls
                .iter()
                .map(|call| ToolCallResult {
                    tool_call_id: call.id.clone(),
                    llm_response: json!({
                        TOOL_RESPONSE_KEY_STATUS: TOOL_ERROR_LABEL,
                        TOOL_RESPONSE_KEY_TOOL: call.function.name,
                        TOOL_RESPONSE_KEY_MESSAGE: "Client connection unavailable",
                    })
                    .to_string(),
                })
                .collect());
        };

        let mut results = Vec::new();

        for call in calls {
            let tool = self.tool_registry.lookup(&call.function.name);
            let args_value_result: Result<Value, _> =
                serde_json::from_str(&call.function.arguments);
            let args_value_for_input = args_value_result.as_ref().ok().cloned();
            let title = match (tool, args_value_for_input.as_ref()) {
                (Some(tool), Some(args)) => self.tool_registry.render_title(tool, args),
                (Some(tool), None) => tool.default_title().to_string(),
                (None, _) => format!("{} (unsupported)", call.function.name),
            };

            let call_id = acp::ToolCallId(Arc::from(call.id.clone()));
            let initial_call = acp::ToolCall {
                id: call_id.clone(),
                title,
                kind: tool.map(|t| t.kind()).unwrap_or(acp::ToolKind::Other),
                status: acp::ToolCallStatus::Pending,
                content: Vec::new(),
                locations: Vec::new(),
                raw_input: args_value_for_input.clone(),
                raw_output: None,
                meta: None,
            };

            self.send_update(
                session_id,
                acp::SessionUpdate::ToolCall(initial_call.clone()),
            )
            .await?;

            let permission_override = if session.cancel_flag.get() {
                None
            } else if let (Some(tool_kind), Ok(args_value)) = (tool, args_value_result.as_ref()) {
                self.request_tool_permission(
                    client.as_ref(),
                    session_id,
                    &initial_call,
                    tool_kind,
                    args_value,
                )
                .await?
            } else {
                None
            };

            if tool.is_some() && permission_override.is_none() && !session.cancel_flag.get() {
                let mut in_progress_fields = acp::ToolCallUpdateFields::default();
                in_progress_fields.status = Some(acp::ToolCallStatus::InProgress);
                let progress_update = acp::ToolCallUpdate {
                    id: call_id.clone(),
                    fields: in_progress_fields,
                    meta: None,
                };
                self.send_update(
                    session_id,
                    acp::SessionUpdate::ToolCallUpdate(progress_update),
                )
                .await?;
            }

            let mut report = if let Some(report) = permission_override {
                report
            } else if session.cancel_flag.get() {
                ToolExecutionReport::cancelled(&call.function.name)
            } else {
                match (tool, args_value_result) {
                    (Some(tool), Ok(args_value)) => {
                        self.execute_tool(tool, &client, session_id, &args_value)
                            .await
                    }
                    (None, Ok(_)) => {
                        ToolExecutionReport::failure(&call.function.name, "Unsupported tool")
                    }
                    (_, Err(error)) => ToolExecutionReport::failure(
                        &call.function.name,
                        &format!("Invalid JSON arguments: {error}"),
                    ),
                }
            };

            if session.cancel_flag.get() && matches!(report.status, acp::ToolCallStatus::Completed)
            {
                report = ToolExecutionReport::cancelled(&call.function.name);
            }

            let mut update_fields = acp::ToolCallUpdateFields::default();
            update_fields.status = Some(report.status);
            if !report.content.is_empty() {
                update_fields.content = Some(report.content.clone());
            }
            if let Some(raw_output) = &report.raw_output {
                update_fields.raw_output = Some(raw_output.clone());
            }

            let update = acp::ToolCallUpdate {
                id: call_id.clone(),
                fields: update_fields,
                meta: None,
            };

            self.send_update(session_id, acp::SessionUpdate::ToolCallUpdate(update))
                .await?;

            results.push(ToolCallResult {
                tool_call_id: call.id.clone(),
                llm_response: report.llm_response,
            });
        }

        Ok(results)
    }

    async fn execute_tool(
        &self,
        tool: SupportedTool,
        client: &AgentSideConnection,
        session_id: &acp::SessionId,
        args: &Value,
    ) -> ToolExecutionReport {
        match tool {
            SupportedTool::ReadFile => self
                .run_read_file(client, session_id, args)
                .await
                .unwrap_or_else(|message| ToolExecutionReport::failure(tools::READ_FILE, &message)),
        }
    }

    async fn run_read_file(
        &self,
        client: &AgentSideConnection,
        session_id: &acp::SessionId,
        args: &Value,
    ) -> Result<ToolExecutionReport, String> {
        let path = self.parse_tool_path(args)?;
        let line = Self::parse_positive_argument(args, TOOL_READ_FILE_LINE_ARG)?;
        let limit = Self::parse_positive_argument(args, TOOL_READ_FILE_LIMIT_ARG)?;

        let request = acp::ReadTextFileRequest {
            session_id: session_id.clone(),
            path: path.clone(),
            line,
            limit,
            meta: None,
        };

        let response = client.read_text_file(request).await.map_err(|error| {
            warn!(%error, path = ?path, "Failed to read file via ACP client");
            format!("Unable to read file: {error}")
        })?;

        let (truncated_content, truncated) = self.truncate_text(&response.content);
        let mut tool_content = truncated_content.clone();
        if truncated {
            tool_content.push_str("\n\n[truncated]");
        }

        let payload = json!({
            TOOL_RESPONSE_KEY_STATUS: TOOL_SUCCESS_LABEL,
            TOOL_RESPONSE_KEY_TOOL: tools::READ_FILE,
            TOOL_RESPONSE_KEY_PATH: path.to_string_lossy(),
            TOOL_RESPONSE_KEY_CONTENT: truncated_content,
            TOOL_RESPONSE_KEY_TRUNCATED: truncated,
        });

        Ok(ToolExecutionReport::success(
            vec![acp::ToolCallContent::from(tool_content)],
            payload,
        ))
    }

    fn append_segment(target: &mut String, segment: &str) {
        if !target.is_empty() {
            target.push('\n');
        }
        target.push_str(segment);
    }

    fn render_context_block(name: &str, uri: &str, body: Option<&str>) -> String {
        match body {
            Some(content) => {
                let mut rendered = String::new();
                rendered.push_str(RESOURCE_CONTEXT_OPEN);
                rendered.push(' ');
                rendered.push_str(RESOURCE_CONTEXT_URI_ATTR);
                rendered.push_str("=\"");
                rendered.push_str(uri);
                rendered.push_str("\" ");
                rendered.push_str(RESOURCE_CONTEXT_NAME_ATTR);
                rendered.push_str("=\"");
                rendered.push_str(name);
                rendered.push_str("\">\n");
                rendered.push_str(content);
                if !content.ends_with('\n') {
                    rendered.push('\n');
                }
                rendered.push_str(RESOURCE_CONTEXT_CLOSE);
                rendered
            }
            None => format!("{RESOURCE_FALLBACK_LABEL} {name} ({uri})"),
        }
    }

    fn parse_resource_path(uri: &str) -> Result<PathBuf, String> {
        if uri.is_empty() {
            return Err(format!(
                "Unable to resolve URI provided to {}",
                tools::READ_FILE
            ));
        }

        if uri.starts_with('/') {
            let candidate = PathBuf::from(uri);
            return Self::ensure_absolute_path(candidate, TOOL_READ_FILE_URI_ARG);
        }

        let parsed = Url::parse(uri)
            .map_err(|_| format!("Unable to resolve URI provided to {}", tools::READ_FILE))?;

        let path = match parsed.scheme() {
            "file" => parsed
                .to_file_path()
                .map_err(|_| format!("Unable to resolve URI provided to {}", tools::READ_FILE))?,
            "zed" | "zed-fs" => {
                let raw_path = parsed.path();
                if raw_path.is_empty() {
                    return Err(format!(
                        "Unable to resolve URI provided to {}",
                        tools::READ_FILE
                    ));
                }
                let decoded = percent_decode_str(raw_path).decode_utf8().map_err(|_| {
                    format!("Unable to resolve URI provided to {}", tools::READ_FILE)
                })?;
                PathBuf::from(decoded.as_ref())
            }
            _ => {
                return Err(format!(
                    "Unable to resolve URI provided to {}",
                    tools::READ_FILE
                ));
            }
        };

        Self::ensure_absolute_path(path, TOOL_READ_FILE_URI_ARG)
    }

    async fn resolve_prompt(
        &self,
        session_id: &acp::SessionId,
        prompt: &[acp::ContentBlock],
    ) -> Result<String, acp::Error> {
        let mut aggregated = String::new();

        for block in prompt {
            match block {
                acp::ContentBlock::Text(text) => Self::append_segment(&mut aggregated, &text.text),
                acp::ContentBlock::ResourceLink(link) => {
                    let rendered = self.render_resource_link(session_id, link).await?;
                    Self::append_segment(&mut aggregated, &rendered);
                }
                acp::ContentBlock::Resource(resource) => match &resource.resource {
                    acp::EmbeddedResourceResource::TextResourceContents(text) => {
                        let rendered =
                            Self::render_context_block(&text.uri, &text.uri, Some(&text.text));
                        Self::append_segment(&mut aggregated, &rendered);
                    }
                    acp::EmbeddedResourceResource::BlobResourceContents(blob) => {
                        warn!(
                            uri = blob.uri,
                            "Ignoring unsupported embedded blob resource"
                        );
                        let rendered = format!(
                            "{RESOURCE_FAILURE_LABEL} {name} ({uri})",
                            name = blob.uri,
                            uri = blob.uri
                        );
                        Self::append_segment(&mut aggregated, &rendered);
                    }
                },
                acp::ContentBlock::Image(image) => {
                    let identifier = image.uri.as_deref().unwrap_or(image.mime_type.as_str());
                    let placeholder = format!(
                        "{RESOURCE_FALLBACK_LABEL} image ({identifier})",
                        identifier = identifier
                    );
                    Self::append_segment(&mut aggregated, &placeholder);
                }
                acp::ContentBlock::Audio(audio) => {
                    let placeholder = format!(
                        "{RESOURCE_FALLBACK_LABEL} audio ({mime})",
                        mime = audio.mime_type
                    );
                    Self::append_segment(&mut aggregated, &placeholder);
                }
            }
        }

        Ok(aggregated)
    }

    async fn render_resource_link(
        &self,
        session_id: &acp::SessionId,
        link: &acp::ResourceLink,
    ) -> Result<String, acp::Error> {
        let Some(client) = self.client() else {
            return Ok(Self::render_context_block(&link.name, &link.uri, None));
        };

        if !self.client_supports_read_text_file() {
            return Ok(Self::render_context_block(&link.name, &link.uri, None));
        }

        let path = match Self::parse_resource_path(&link.uri) {
            Ok(path) => path,
            Err(_) => {
                return Ok(Self::render_context_block(&link.name, &link.uri, None));
            }
        };

        let request = acp::ReadTextFileRequest {
            session_id: session_id.clone(),
            path,
            line: None,
            limit: None,
            meta: None,
        };

        match client.read_text_file(request).await {
            Ok(response) => Ok(Self::render_context_block(
                &link.name,
                &link.uri,
                Some(response.content.as_str()),
            )),
            Err(error) => {
                warn!(%error, uri = link.uri, name = link.name, "Failed to read linked resource");
                Ok(format!(
                    "{RESOURCE_FAILURE_LABEL} {name} ({uri})",
                    name = link.name,
                    uri = link.uri
                ))
            }
        }
    }

    async fn send_tool_disable_notice(
        &self,
        session_id: &acp::SessionId,
        reason: &ToolDisableReason<'_>,
    ) -> Result<(), acp::Error> {
        let mut notice = match reason {
            ToolDisableReason::Config => {
                TOOL_DISABLED_CONFIG_NOTICE.replace("{tool}", tools::READ_FILE)
            }
            ToolDisableReason::Provider { provider, model } => TOOL_DISABLED_PROVIDER_NOTICE
                .replace("{tool}", tools::READ_FILE)
                .replace("{model}", model)
                .replace("{provider}", provider),
            ToolDisableReason::ClientCapabilities => {
                TOOL_DISABLED_CAPABILITY_NOTICE.replace("{tool}", tools::READ_FILE)
            }
        };

        if !notice.ends_with('.') {
            notice.push('.');
        }

        self.send_update(
            session_id,
            acp::SessionUpdate::AgentThoughtChunk {
                content: notice.into(),
            },
        )
        .await
    }

    async fn send_update(
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

    async fn send_plan_update(
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
}

#[async_trait(?Send)]
impl acp::Agent for ZedAgent {
    async fn initialize(
        &self,
        args: acp::InitializeRequest,
    ) -> Result<acp::InitializeResponse, acp::Error> {
        self.client_capabilities
            .replace(Some(args.client_capabilities.clone()));

        if args.protocol_version != acp::V1 {
            warn!(
                requested = ?args.protocol_version,
                "{}",
                INITIALIZE_VERSION_MISMATCH_LOG
            );
        }

        let mut capabilities = acp::AgentCapabilities::default();
        capabilities.prompt_capabilities.embedded_context = true;

        Ok(acp::InitializeResponse {
            protocol_version: acp::V1,
            agent_capabilities: capabilities,
            auth_methods: Vec::new(),
            meta: None,
        })
    }

    async fn authenticate(
        &self,
        _args: acp::AuthenticateRequest,
    ) -> Result<acp::AuthenticateResponse, acp::Error> {
        Ok(acp::AuthenticateResponse::default())
    }

    async fn new_session(
        &self,
        _args: acp::NewSessionRequest,
    ) -> Result<acp::NewSessionResponse, acp::Error> {
        let session_id = self.register_session();
        Ok(acp::NewSessionResponse {
            session_id,
            modes: None,
            meta: None,
        })
    }

    async fn prompt(&self, args: acp::PromptRequest) -> Result<acp::PromptResponse, acp::Error> {
        let Some(session) = self.session_handle(&args.session_id) else {
            return Err(
                acp::Error::invalid_params().with_data(json!({ "reason": "unknown_session" }))
            );
        };

        session.cancel_flag.set(false);

        let user_message = self.resolve_prompt(&args.session_id, &args.prompt).await?;
        self.push_message(&session, Message::user(user_message.clone()));

        let provider = match create_provider_for_model(
            &self.config.model,
            self.config.api_key.clone(),
            Some(self.config.prompt_cache.clone()),
        ) {
            Ok(provider) => provider,
            Err(_) => create_provider_with_config(
                &self.config.provider,
                Some(self.config.api_key.clone()),
                None,
                Some(self.config.model.clone()),
                Some(self.config.prompt_cache.clone()),
            )
            .map_err(acp::Error::into_internal_error)?,
        };

        let supports_streaming = provider.supports_streaming();
        let reasoning_effort = if provider.supports_reasoning_effort(&self.config.model) {
            Some(self.config.reasoning_effort.as_str().to_string())
        } else {
            None
        };

        let mut stop_reason = acp::StopReason::EndTurn;
        let mut assistant_message = String::new();
        let has_registered_tools = !self.tool_registry.is_empty();
        let client_supports_read_text_file = self.client_supports_read_text_file();
        let tool_runtime = if has_registered_tools {
            if !self.zed_config.tools.read_file {
                ToolRuntime::Disabled(ToolDisableReason::Config)
            } else if !client_supports_read_text_file {
                ToolRuntime::Disabled(ToolDisableReason::ClientCapabilities)
            } else if provider.supports_tools(&self.config.model) {
                ToolRuntime::Enabled
            } else {
                ToolRuntime::Disabled(ToolDisableReason::Provider {
                    provider: self.config.provider.as_str(),
                    model: self.config.model.as_str(),
                })
            }
        } else {
            ToolRuntime::Disabled(ToolDisableReason::Config)
        };
        let tools_allowed = matches!(tool_runtime, ToolRuntime::Enabled);
        if has_registered_tools {
            if let ToolRuntime::Disabled(reason) = &tool_runtime {
                if self.should_send_tool_notice(&session) {
                    match reason {
                        ToolDisableReason::Config => {
                            warn!("{}", TOOL_DISABLED_CONFIG_LOG);
                        }
                        ToolDisableReason::Provider { provider, model } => {
                            warn!(
                                provider = %provider,
                                model = %model,
                                "{}",
                                TOOL_DISABLED_PROVIDER_LOG
                            );
                        }
                        ToolDisableReason::ClientCapabilities => {
                            warn!("{}", TOOL_DISABLED_CAPABILITY_LOG);
                        }
                    }

                    self.send_tool_disable_notice(&args.session_id, reason)
                        .await?;
                    self.mark_tool_notice_sent(&session);
                }
            }
        }

        let tool_definitions = self.tool_definitions(tools_allowed);
        let mut messages = self.resolved_messages(&session);
        let allow_streaming = supports_streaming && !tools_allowed;

        let mut plan = PlanProgress::new(tools_allowed && has_registered_tools);
        if plan.has_entries() {
            self.send_plan_update(&args.session_id, &plan).await?;
            if plan.complete_analysis() {
                self.send_plan_update(&args.session_id, &plan).await?;
            }
        }

        if allow_streaming {
            let request = LLMRequest {
                messages: messages.clone(),
                system_prompt: None,
                tools: tool_definitions,
                model: self.config.model.clone(),
                max_tokens: None,
                temperature: None,
                stream: true,
                tool_choice: self.tool_choice(tools_allowed),
                parallel_tool_calls: None,
                parallel_tool_config: None,
                reasoning_effort: reasoning_effort.clone(),
            };

            let mut stream = provider
                .stream(request)
                .await
                .map_err(acp::Error::into_internal_error)?;

            if plan.start_response() {
                self.send_plan_update(&args.session_id, &plan).await?;
            }

            while let Some(event) = stream.next().await {
                let event = event.map_err(acp::Error::into_internal_error)?;

                if session.cancel_flag.get() {
                    stop_reason = acp::StopReason::Cancelled;
                    break;
                }

                match event {
                    LLMStreamEvent::Token { delta } => {
                        if !delta.is_empty() {
                            assistant_message.push_str(&delta);
                            self.send_update(
                                &args.session_id,
                                acp::SessionUpdate::AgentMessageChunk {
                                    content: delta.into(),
                                },
                            )
                            .await?;
                        }
                    }
                    LLMStreamEvent::Reasoning { delta } => {
                        if !delta.is_empty() {
                            self.send_update(
                                &args.session_id,
                                acp::SessionUpdate::AgentThoughtChunk {
                                    content: delta.into(),
                                },
                            )
                            .await?;
                        }
                    }
                    LLMStreamEvent::Completed { response } => {
                        if assistant_message.is_empty()
                            && let Some(content) = response.content
                        {
                            if !content.is_empty() {
                                self.send_update(
                                    &args.session_id,
                                    acp::SessionUpdate::AgentMessageChunk {
                                        content: content.clone().into(),
                                    },
                                )
                                .await?;
                            }
                            assistant_message.push_str(&content);
                        }

                        if let Some(reasoning) =
                            response.reasoning.filter(|reasoning| !reasoning.is_empty())
                        {
                            self.send_update(
                                &args.session_id,
                                acp::SessionUpdate::AgentThoughtChunk {
                                    content: reasoning.into(),
                                },
                            )
                            .await?;
                        }

                        stop_reason = Self::stop_reason_from_finish(response.finish_reason);
                        break;
                    }
                }
            }
        } else {
            loop {
                if session.cancel_flag.get() {
                    stop_reason = acp::StopReason::Cancelled;
                    break;
                }

                let request = LLMRequest {
                    messages: messages.clone(),
                    system_prompt: None,
                    tools: self.tool_definitions(tools_allowed),
                    model: self.config.model.clone(),
                    max_tokens: None,
                    temperature: None,
                    stream: false,
                    tool_choice: self.tool_choice(tools_allowed),
                    parallel_tool_calls: None,
                    parallel_tool_config: None,
                    reasoning_effort: reasoning_effort.clone(),
                };

                let response = provider
                    .generate(request)
                    .await
                    .map_err(acp::Error::into_internal_error)?;

                if session.cancel_flag.get() {
                    stop_reason = acp::StopReason::Cancelled;
                    break;
                }

                if tools_allowed {
                    if let Some(tool_calls) = response
                        .tool_calls
                        .clone()
                        .filter(|calls| !calls.is_empty())
                    {
                        if plan.start_context() {
                            self.send_plan_update(&args.session_id, &plan).await?;
                        }
                        self.push_message(
                            &session,
                            Message::assistant_with_tools(
                                response.content.clone().unwrap_or_default(),
                                tool_calls.clone(),
                            ),
                        );
                        let tool_results = self
                            .execute_tool_calls(&session, &args.session_id, &tool_calls)
                            .await?;
                        if plan.complete_context() {
                            self.send_plan_update(&args.session_id, &plan).await?;
                        }
                        for result in tool_results {
                            self.push_message(
                                &session,
                                Message::tool_response(result.tool_call_id, result.llm_response),
                            );
                        }
                        if session.cancel_flag.get() {
                            stop_reason = acp::StopReason::Cancelled;
                            break;
                        }
                        messages = self.resolved_messages(&session);
                        continue;
                    }
                }

                if let Some(content) = response.content.clone() {
                    if !content.is_empty() {
                        if plan.has_context_step() && !plan.context_completed() {
                            if plan.complete_context() {
                                self.send_plan_update(&args.session_id, &plan).await?;
                            }
                        }
                        if plan.start_response() {
                            self.send_plan_update(&args.session_id, &plan).await?;
                        }
                        if session.cancel_flag.get() {
                            stop_reason = acp::StopReason::Cancelled;
                            break;
                        }
                        self.send_update(
                            &args.session_id,
                            acp::SessionUpdate::AgentMessageChunk {
                                content: content.clone().into(),
                            },
                        )
                        .await?;
                    }
                    assistant_message = content;
                }

                if let Some(reasoning) =
                    response.reasoning.filter(|reasoning| !reasoning.is_empty())
                {
                    if session.cancel_flag.get() {
                        stop_reason = acp::StopReason::Cancelled;
                        break;
                    }
                    self.send_update(
                        &args.session_id,
                        acp::SessionUpdate::AgentThoughtChunk {
                            content: reasoning.into(),
                        },
                    )
                    .await?;
                }

                stop_reason = Self::stop_reason_from_finish(response.finish_reason);
                break;
            }
        }

        if stop_reason != acp::StopReason::Cancelled && !assistant_message.is_empty() {
            self.push_message(&session, Message::assistant(assistant_message));
        }

        if stop_reason != acp::StopReason::Cancelled {
            if plan.complete_context() {
                self.send_plan_update(&args.session_id, &plan).await?;
            }
            if plan.complete_response() {
                self.send_plan_update(&args.session_id, &plan).await?;
            }
        }

        Ok(acp::PromptResponse {
            stop_reason,
            meta: None,
        })
    }

    async fn cancel(&self, args: acp::CancelNotification) -> Result<(), acp::Error> {
        if let Some(session) = self.session_handle(&args.session_id) {
            session.cancel_flag.set(true);
        }
        Ok(())
    }
}
