use crate::acp::permissions::{AcpPermissionPrompter, DefaultPermissionPrompter};
use crate::acp::reports::{
    TOOL_ERROR_LABEL, TOOL_FAILURE_PREFIX, TOOL_RESPONSE_KEY_CONTENT, TOOL_RESPONSE_KEY_MESSAGE,
    TOOL_RESPONSE_KEY_PATH, TOOL_RESPONSE_KEY_STATUS, TOOL_RESPONSE_KEY_TOOL,
    TOOL_RESPONSE_KEY_TRUNCATED, TOOL_SUCCESS_LABEL, ToolExecutionReport,
};
use crate::acp::tooling::{
    AcpToolRegistry, SupportedTool, TOOL_LIST_FILES_ITEMS_KEY, TOOL_LIST_FILES_MESSAGE_KEY,
    TOOL_LIST_FILES_PATH_ARG, TOOL_LIST_FILES_RESULT_KEY, TOOL_LIST_FILES_SUMMARY_MAX_ITEMS,
    TOOL_LIST_FILES_URI_ARG, TOOL_READ_FILE_LIMIT_ARG, TOOL_READ_FILE_LINE_ARG,
    TOOL_READ_FILE_PATH_ARG, TOOL_READ_FILE_URI_ARG, ToolDescriptor,
};
use crate::acp::workspace::{DefaultWorkspaceTrustSynchronizer, WorkspaceTrustSynchronizer};
use crate::acp::{acp_client, register_acp_client};
use agent_client_protocol as acp;
use agent_client_protocol::{AgentSideConnection, Client};
use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::StreamExt;
use path_clean::PathClean;
use percent_encoding::percent_decode_str;
use serde_json::{Value, json};
use shell_words::split;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::mem::discriminant;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::{error, info, warn};
use url::Url;

use vtcode_core::config::constants::tools;
use vtcode_core::config::types::{AgentConfig as CoreAgentConfig, CapabilityLevel};
use vtcode_core::config::{AgentClientProtocolZedConfig, ToolsConfig, VTCodeConfig};
use vtcode_core::core::interfaces::acp::{AcpClientAdapter, AcpLaunchParams};
use vtcode_core::llm::factory::{create_provider_for_model, create_provider_with_config};
use vtcode_core::llm::provider::{
    FinishReason, LLMRequest, LLMStreamEvent, Message, ToolCall as ProviderToolCall, ToolChoice,
    ToolDefinition,
};
use vtcode_core::prompts::read_system_prompt_from_md;
use vtcode_core::tools::file_ops::FileOpsTool;
use vtcode_core::tools::grep_search::GrepSearchManager;
use vtcode_core::tools::registry::{
    ToolRegistry as CoreToolRegistry, build_function_declarations,
    build_function_declarations_for_level,
};
use vtcode_core::tools::traits::Tool;

use crate::workspace_trust::WorkspaceTrustSyncOutcome;

#[derive(Debug, Default, Clone, Copy)]
pub struct ZedAcpAdapter;

#[async_trait(?Send)]
impl AcpClientAdapter for ZedAcpAdapter {
    async fn serve(&self, params: AcpLaunchParams<'_>) -> Result<()> {
        run_zed_agent(params.agent_config, params.runtime_config).await
    }
}

const SESSION_PREFIX: &str = "vtcode-zed-session";
const RESOURCE_FALLBACK_LABEL: &str = "Resource";
const RESOURCE_FAILURE_LABEL: &str = "Resource unavailable";
const RESOURCE_CONTEXT_OPEN: &str = "<context";
const RESOURCE_CONTEXT_CLOSE: &str = "</context>";
const RESOURCE_CONTEXT_URI_ATTR: &str = "uri";
const RESOURCE_CONTEXT_NAME_ATTR: &str = "name";
const MAX_TOOL_RESPONSE_CHARS: usize = 32_768;
const TOOL_DISABLED_PROVIDER_NOTICE: &str =
    "Skipping {tool} tool: model {model} on {provider} does not support function calling";
const TOOL_DISABLED_CAPABILITY_NOTICE: &str =
    "Skipping {tool} tool: client does not advertise fs.readTextFile capability";
const TOOL_DISABLED_PROVIDER_LOG_MESSAGE: &str =
    "ACP tool disabled because the selected model does not support function calling";
const TOOL_DISABLED_CAPABILITY_LOG_MESSAGE: &str =
    "ACP tool disabled because the client lacks fs.readTextFile support";
const INITIALIZE_VERSION_MISMATCH_LOG: &str =
    "Client requested unsupported ACP protocol version; responding with v1";
const TOOL_READ_FILE_INVALID_INTEGER_TEMPLATE: &str =
    "Invalid {argument} value: expected a positive integer";
const TOOL_READ_FILE_INTEGER_RANGE_TEMPLATE: &str = "{argument} value exceeds the supported range";
const TOOL_READ_FILE_ABSOLUTE_PATH_TEMPLATE: &str =
    "Invalid {argument} value: expected an absolute path";
const TOOL_READ_FILE_WORKSPACE_ESCAPE_TEMPLATE: &str =
    "Invalid {argument} value: path escapes the trusted workspace";
const PLAN_STEP_ANALYZE: &str = "Review the latest user request and conversation context";
const PLAN_STEP_GATHER_CONTEXT: &str = "Gather referenced workspace files when required";
const PLAN_STEP_RESPOND: &str = "Compose and send the assistant response";
const WORKSPACE_TRUST_UPGRADE_LOG: &str = "ACP workspace trust level updated";
const WORKSPACE_TRUST_ALREADY_SATISFIED_LOG: &str = "ACP workspace trust level already satisfied";
const WORKSPACE_TRUST_DOWNGRADE_SKIPPED_LOG: &str =
    "ACP workspace trust downgrade skipped because workspace is fully trusted";

enum ToolRuntime<'a> {
    Enabled,
    Disabled(ToolDisableReason<'a>),
}

#[derive(Clone, Copy)]
enum ToolDisableReason<'a> {
    Provider { provider: &'a str, model: &'a str },
    ClientCapabilities,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum RunTerminalMode {
    Terminal,
    Pty,
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

pub async fn run_zed_agent(config: &CoreAgentConfig, vt_cfg: &VTCodeConfig) -> Result<()> {
    let zed_config = &vt_cfg.acp.zed;
    let desired_trust_level = zed_config.workspace_trust.to_workspace_trust_level();
    let trust_synchronizer = DefaultWorkspaceTrustSynchronizer::new();
    match trust_synchronizer
        .synchronize(&config.workspace, desired_trust_level)
        .context("Failed to synchronize workspace trust for ACP bridge")?
    {
        WorkspaceTrustSyncOutcome::Upgraded { previous, new } => {
            info!(previous = ?previous, new = ?new, "{}", WORKSPACE_TRUST_UPGRADE_LOG);
        }
        WorkspaceTrustSyncOutcome::AlreadyMatches(level) => {
            info!(level = ?level, "{}", WORKSPACE_TRUST_ALREADY_SATISFIED_LOG);
        }
        WorkspaceTrustSyncOutcome::SkippedDowngrade(current) => {
            info!(
                current = ?current,
                requested = ?zed_config.workspace_trust,
                "{}",
                WORKSPACE_TRUST_DOWNGRADE_SKIPPED_LOG
            );
        }
    }

    let outgoing = tokio::io::stdout().compat_write();
    let incoming = tokio::io::stdin().compat();
    let system_prompt = read_system_prompt_from_md().unwrap_or_else(|_| String::new());
    let tools_config = vt_cfg.tools.clone();

    let local_set = tokio::task::LocalSet::new();
    let config_clone = config.clone();
    let zed_config_clone = zed_config.clone();

    local_set
        .run_until(async move {
            let (tx, mut rx) = mpsc::unbounded_channel::<NotificationEnvelope>();
            let tools_config_clone = tools_config.clone();
            let agent = ZedAgent::new(
                config_clone,
                zed_config_clone,
                tools_config_clone,
                system_prompt,
                tx,
            );
            let (raw_conn, io_task) =
                acp::AgentSideConnection::new(agent, outgoing, incoming, |fut| {
                    tokio::task::spawn_local(fut);
                });
            let conn = Arc::new(raw_conn);
            if let Err(existing) = register_acp_client(Arc::clone(&conn)) {
                warn!("ACP client already registered; continuing with existing instance");
                drop(existing);
            }

            let notifications_conn = Arc::clone(&conn);
            let notifications = tokio::task::spawn_local(async move {
                while let Some(envelope) = rx.recv().await {
                    let result = notifications_conn
                        .session_notification(envelope.notification)
                        .await;
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
    system_prompt: String,
    sessions: Rc<RefCell<HashMap<acp::SessionId, SessionHandle>>>,
    next_session_id: Cell<u64>,
    session_update_tx: mpsc::UnboundedSender<NotificationEnvelope>,
    acp_tool_registry: Rc<AcpToolRegistry>,
    permission_prompter: Rc<dyn AcpPermissionPrompter>,
    local_tool_registry: RefCell<CoreToolRegistry>,
    file_ops_tool: Option<FileOpsTool>,
    client_capabilities: Rc<RefCell<Option<acp::ClientCapabilities>>>,
}

impl ZedAgent {
    fn new(
        config: CoreAgentConfig,
        zed_config: AgentClientProtocolZedConfig,
        tools_config: ToolsConfig,
        system_prompt: String,
        session_update_tx: mpsc::UnboundedSender<NotificationEnvelope>,
    ) -> Self {
        let read_file_enabled = zed_config.tools.read_file;
        let workspace_root = config.workspace.clone();
        let file_ops_tool = if zed_config.tools.list_files {
            let search_root = workspace_root.clone();
            Some(FileOpsTool::new(
                workspace_root.clone(),
                Arc::new(GrepSearchManager::new(search_root)),
            ))
        } else {
            None
        };
        let list_files_enabled = file_ops_tool.is_some();

        let mut core_tool_registry = CoreToolRegistry::new(config.workspace.clone());
        if let Err(error) = core_tool_registry.apply_config_policies(&tools_config) {
            warn!(%error, "Failed to apply tools configuration to ACP tool registry");
        }
        let available_local_tools: HashSet<String> =
            core_tool_registry.available_tools().into_iter().collect();
        let mut local_definitions =
            build_function_declarations_for_level(CapabilityLevel::CodeSearch)
                .into_iter()
                .filter(|decl| decl.name != tools::READ_FILE && decl.name != tools::LIST_FILES)
                .filter(|decl| available_local_tools.contains(decl.name.as_str()))
                .map(|decl| {
                    ToolDefinition::function(
                        decl.name.clone(),
                        decl.description.clone(),
                        decl.parameters.clone(),
                    )
                })
                .collect::<Vec<_>>();

        if available_local_tools.contains(tools::BASH) {
            if let Some(bash_decl) = build_function_declarations()
                .into_iter()
                .find(|decl| decl.name == tools::BASH)
            {
                let already_registered = local_definitions
                    .iter()
                    .any(|definition| definition.function_name() == tools::BASH);
                if !already_registered {
                    local_definitions.push(ToolDefinition::function(
                        bash_decl.name.clone(),
                        bash_decl.description.clone(),
                        bash_decl.parameters.clone(),
                    ));
                }
            }
        }
        let acp_tool_registry = Rc::new(AcpToolRegistry::new(
            workspace_root.as_path(),
            read_file_enabled,
            list_files_enabled,
            local_definitions,
        ));
        let permission_prompter: Rc<dyn AcpPermissionPrompter> = Rc::new(
            DefaultPermissionPrompter::new(Rc::clone(&acp_tool_registry)),
        );

        Self {
            config,
            system_prompt,
            sessions: Rc::new(RefCell::new(HashMap::new())),
            next_session_id: Cell::new(0),
            session_update_tx,
            acp_tool_registry,
            permission_prompter,
            local_tool_registry: RefCell::new(core_tool_registry),
            file_ops_tool,
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

    fn client(&self) -> Option<Arc<AgentSideConnection>> {
        acp_client()
    }

    fn tool_definitions(
        &self,
        provider_supports_tools: bool,
        enabled_tools: &[SupportedTool],
    ) -> Option<Vec<ToolDefinition>> {
        if !provider_supports_tools {
            return None;
        }

        let include_local = self.acp_tool_registry.has_local_tools();
        if enabled_tools.is_empty() && !include_local {
            None
        } else {
            Some(
                self.acp_tool_registry
                    .definitions_for(enabled_tools, include_local),
            )
        }
    }

    fn tool_choice(&self, tools_available: bool) -> Option<ToolChoice> {
        if tools_available {
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

    fn client_supports_terminal(&self) -> bool {
        self.client_capabilities
            .borrow()
            .as_ref()
            .map(|capabilities| capabilities.terminal)
            .unwrap_or(false)
    }

    fn tool_availability<'a>(
        &'a self,
        provider_supports_tools: bool,
        client_supports_read_text_file: bool,
    ) -> Vec<(SupportedTool, ToolRuntime<'a>)> {
        self.acp_tool_registry
            .registered_tools()
            .into_iter()
            .map(|tool| {
                let runtime = if !provider_supports_tools {
                    ToolRuntime::Disabled(ToolDisableReason::Provider {
                        provider: self.config.provider.as_str(),
                        model: self.config.model.as_str(),
                    })
                } else {
                    match tool {
                        SupportedTool::ReadFile => {
                            if client_supports_read_text_file {
                                ToolRuntime::Enabled
                            } else {
                                ToolRuntime::Disabled(ToolDisableReason::ClientCapabilities)
                            }
                        }
                        SupportedTool::ListFiles => ToolRuntime::Enabled,
                    }
                };
                (tool, runtime)
            })
            .collect()
    }

    fn requested_terminal_mode(args: &Value) -> Result<RunTerminalMode, String> {
        if let Some(mode_value) = args.get("mode").and_then(Value::as_str) {
            let normalized = mode_value.trim().to_lowercase();
            match normalized.as_str() {
                "pty" => return Ok(RunTerminalMode::Pty),
                "terminal" | "" => return Ok(RunTerminalMode::Terminal),
                "streaming" => {
                    return Err("run_terminal_cmd does not support streaming mode".to_string());
                }
                _ => {}
            }
        }

        if args.get("tty").and_then(Value::as_bool).unwrap_or(false) {
            return Ok(RunTerminalMode::Pty);
        }

        Ok(RunTerminalMode::Terminal)
    }

    fn parse_terminal_command(args: &Value) -> Result<Vec<String>, String> {
        if let Some(array) = args.get("command").and_then(Value::as_array) {
            let mut parts = Vec::with_capacity(array.len());
            for value in array {
                let Some(segment) = value.as_str() else {
                    return Err("command array must contain only strings".to_string());
                };
                parts.push(segment.to_string());
            }
            if parts.is_empty() {
                return Err("command array cannot be empty".to_string());
            }
            return Ok(parts);
        }

        if let Some(command_str) = args.get("command").and_then(Value::as_str) {
            let segments = split(command_str)
                .map_err(|error| format!("failed to parse command string: {error}"))?;
            if segments.is_empty() {
                return Err("command string cannot be empty".to_string());
            }
            return Ok(segments);
        }

        Err("run_terminal_cmd requires a 'command' array".to_string())
    }

    fn resolve_terminal_working_dir(&self, args: &Value) -> Result<Option<PathBuf>, String> {
        let requested = args
            .get("working_dir")
            .or_else(|| args.get("cwd"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());

        let Some(raw_dir) = requested else {
            return Ok(None);
        };

        let candidate = Path::new(raw_dir);
        let resolved = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            self.config.workspace.join(candidate)
        };

        let cleaned = resolved.clean();
        if !cleaned.starts_with(&self.config.workspace) {
            return Err("working_dir must stay within the workspace".to_string());
        }

        Ok(Some(cleaned))
    }

    fn describe_terminal_location(&self, working_dir: Option<&PathBuf>) -> Option<String> {
        let workspace = &self.config.workspace;
        working_dir.and_then(|path| {
            path.strip_prefix(workspace).ok().map(|relative| {
                if relative.as_os_str().is_empty() {
                    ".".to_string()
                } else {
                    format!("./{}", relative.to_string_lossy())
                }
            })
        })
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

    fn render_tool_disable_notice(
        &self,
        tool: SupportedTool,
        reason: &ToolDisableReason<'_>,
    ) -> String {
        let tool_name = tool.function_name();
        match reason {
            ToolDisableReason::Provider { provider, model } => TOOL_DISABLED_PROVIDER_NOTICE
                .replace("{tool}", tool_name)
                .replace("{model}", model)
                .replace("{provider}", provider),
            ToolDisableReason::ClientCapabilities => {
                TOOL_DISABLED_CAPABILITY_NOTICE.replace("{tool}", tool_name)
            }
        }
    }

    fn log_tool_disable_reason(&self, tool: SupportedTool, reason: &ToolDisableReason<'_>) {
        match reason {
            ToolDisableReason::Provider { provider, model } => {
                warn!(
                    tool = tool.function_name(),
                    provider = %provider,
                    model = %model,
                    "{}",
                    TOOL_DISABLED_PROVIDER_LOG_MESSAGE
                );
            }
            ToolDisableReason::ClientCapabilities => {
                warn!(
                    tool = tool.function_name(),
                    "{}", TOOL_DISABLED_CAPABILITY_LOG_MESSAGE
                );
            }
        }
    }

    async fn send_tool_disable_notices(
        &self,
        session_id: &acp::SessionId,
        reasons: &[(SupportedTool, ToolDisableReason<'_>)],
    ) -> Result<(), acp::Error> {
        if reasons.is_empty() {
            return Ok(());
        }

        let mut combined = String::new();
        for (index, (tool, reason)) in reasons.iter().enumerate() {
            let mut notice = self.render_tool_disable_notice(*tool, reason);
            if !notice.ends_with('.') {
                notice.push('.');
            }
            if index > 0 {
                combined.push(' ');
            }
            combined.push_str(&notice);
        }

        self.send_update(
            session_id,
            acp::SessionUpdate::AgentThoughtChunk {
                content: combined.into(),
            },
        )
        .await
    }

    fn workspace_root(&self) -> &Path {
        self.config.workspace.as_path()
    }

    fn resolve_workspace_path(
        &self,
        candidate: PathBuf,
        argument: &str,
    ) -> Result<PathBuf, String> {
        let workspace_root = self.workspace_root().to_path_buf().clean();
        let resolved_candidate = if candidate.is_absolute() {
            candidate
        } else {
            self.workspace_root().join(candidate)
        };
        let normalized = resolved_candidate.clean();

        if !normalized.is_absolute() {
            return Err(Self::argument_message(
                TOOL_READ_FILE_ABSOLUTE_PATH_TEMPLATE,
                argument,
            ));
        }

        if !normalized.starts_with(&workspace_root) {
            return Err(Self::argument_message(
                TOOL_READ_FILE_WORKSPACE_ESCAPE_TEMPLATE,
                argument,
            ));
        }

        Ok(normalized)
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

    fn parse_tool_path(&self, args: &Value) -> Result<PathBuf, String> {
        if let Some(path) = args
            .get(TOOL_READ_FILE_PATH_ARG)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            let candidate = PathBuf::from(path);
            return self.resolve_workspace_path(candidate, TOOL_READ_FILE_PATH_ARG);
        }

        if let Some(uri) = args
            .get(TOOL_READ_FILE_URI_ARG)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            return self.parse_resource_path(uri);
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
            let tool_descriptor = self.acp_tool_registry.lookup(&call.function.name);
            let args_value_result: Result<Value, _> =
                serde_json::from_str(&call.function.arguments);
            let args_value_for_input = args_value_result.as_ref().ok().cloned();
            let title = match (tool_descriptor, args_value_for_input.as_ref()) {
                (Some(descriptor), Some(args)) => {
                    self.acp_tool_registry
                        .render_title(descriptor, &call.function.name, args)
                }
                (Some(descriptor), None) => {
                    let null_args = Value::Null;
                    self.acp_tool_registry
                        .render_title(descriptor, &call.function.name, &null_args)
                }
                (None, _) => format!("{} (unsupported)", call.function.name),
            };

            let call_id = acp::ToolCallId(Arc::from(call.id.clone()));
            let initial_call = acp::ToolCall {
                id: call_id.clone(),
                title,
                kind: tool_descriptor
                    .map(|descriptor| descriptor.kind())
                    .unwrap_or(acp::ToolKind::Other),
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
            } else if let (Some(ToolDescriptor::Acp(tool_kind)), Ok(args_value)) =
                (tool_descriptor, args_value_result.as_ref())
            {
                self.permission_prompter
                    .request_tool_permission(
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

            if tool_descriptor.is_some()
                && permission_override.is_none()
                && !session.cancel_flag.get()
            {
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
                match (tool_descriptor, args_value_result) {
                    (Some(descriptor), Ok(args_value)) => {
                        self.execute_descriptor(
                            descriptor,
                            &call.function.name,
                            &client,
                            session_id,
                            &args_value,
                        )
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
            if !report.locations.is_empty() {
                update_fields.locations = Some(report.locations.clone());
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

    async fn execute_descriptor(
        &self,
        descriptor: ToolDescriptor,
        tool_name: &str,
        client: &AgentSideConnection,
        session_id: &acp::SessionId,
        args: &Value,
    ) -> ToolExecutionReport {
        if tool_name == tools::RUN_TERMINAL_CMD {
            if let Some(report) = self
                .execute_terminal_via_client(tool_name, client, session_id, args)
                .await
            {
                return report;
            }
        }

        match descriptor {
            ToolDescriptor::Acp(tool) => {
                self.execute_acp_tool(tool, client, session_id, args).await
            }
            ToolDescriptor::Local => self.execute_local_tool(tool_name, args).await,
        }
    }

    async fn execute_terminal_via_client(
        &self,
        tool_name: &str,
        client: &AgentSideConnection,
        session_id: &acp::SessionId,
        args: &Value,
    ) -> Option<ToolExecutionReport> {
        if !self.client_supports_terminal() {
            return None;
        }

        match Self::requested_terminal_mode(args) {
            Ok(RunTerminalMode::Terminal) => None,
            Ok(RunTerminalMode::Pty) => Some(
                match self
                    .launch_client_terminal(tool_name, client, session_id, args)
                    .await
                {
                    Ok(report) => report,
                    Err(message) => ToolExecutionReport::failure(tool_name, &message),
                },
            ),
            Err(message) => Some(ToolExecutionReport::failure(tool_name, &message)),
        }
    }

    async fn launch_client_terminal(
        &self,
        tool_name: &str,
        client: &AgentSideConnection,
        session_id: &acp::SessionId,
        args: &Value,
    ) -> Result<ToolExecutionReport, String> {
        let command_parts = Self::parse_terminal_command(args)?;
        let (program, rest) = command_parts
            .split_first()
            .ok_or_else(|| "command array cannot be empty".to_string())?;

        let working_dir = self.resolve_terminal_working_dir(args)?;
        let location_display = self.describe_terminal_location(working_dir.as_ref());
        let command_display = command_parts.join(" ");

        let request = acp::CreateTerminalRequest {
            session_id: session_id.clone(),
            command: program.to_string(),
            args: rest.iter().cloned().collect(),
            env: Vec::new(),
            cwd: working_dir.clone(),
            output_byte_limit: None,
            meta: None,
        };

        let response = client
            .create_terminal(request)
            .await
            .map_err(|error| format!("Failed to create terminal: {error}"))?;
        let terminal_id = response.terminal_id;

        let mut content = Vec::new();
        let summary = match location_display.as_deref() {
            Some(".") | None => format!("Started terminal command: {command_display}"),
            Some(location) => {
                format!("Started terminal command in {location}: {command_display}")
            }
        };
        content.push(acp::ToolCallContent::from(summary));
        content.push(acp::ToolCallContent::Terminal {
            terminal_id: terminal_id.clone(),
        });

        let payload = json!({
            TOOL_RESPONSE_KEY_STATUS: TOOL_SUCCESS_LABEL,
            TOOL_RESPONSE_KEY_TOOL: tool_name,
            "result": {
                "terminal_id": terminal_id.to_string(),
                "mode": "pty",
                "command": command_parts,
                "working_dir": location_display,
            }
        });

        Ok(ToolExecutionReport::success(content, Vec::new(), payload))
    }

    async fn execute_acp_tool(
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
            SupportedTool::ListFiles => self.run_list_files(args).await.unwrap_or_else(|message| {
                ToolExecutionReport::failure(tools::LIST_FILES, &message)
            }),
        }
    }

    async fn execute_local_tool(&self, tool_name: &str, args: &Value) -> ToolExecutionReport {
        let mut registry = self.local_tool_registry.borrow_mut();
        match registry.execute_tool(tool_name, args.clone()).await {
            Ok(output) => {
                if let Some(error_value) = output.get("error") {
                    let message = error_value
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("Tool execution failed");
                    return ToolExecutionReport::failure(tool_name, message);
                }

                let content = self.render_local_tool_content(tool_name, &output);
                let payload = json!({
                    TOOL_RESPONSE_KEY_STATUS: TOOL_SUCCESS_LABEL,
                    TOOL_RESPONSE_KEY_TOOL: tool_name,
                    "result": output.clone(),
                });
                ToolExecutionReport::success(content, Vec::new(), payload)
            }
            Err(error) => {
                warn!(%error, tool = tool_name, "Failed to execute local tool");
                let message = format!("Unable to execute {tool_name}: {error}");
                ToolExecutionReport::failure(tool_name, &message)
            }
        }
    }

    fn render_local_tool_content(
        &self,
        tool_name: &str,
        output: &Value,
    ) -> Vec<acp::ToolCallContent> {
        let mut sections = Vec::new();

        if let Some(stdout) = output
            .get("stdout")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            let (rendered, truncated) = self.truncate_text(stdout);
            sections.push(format!("stdout:\n{rendered}"));
            if truncated {
                sections.push("[stdout truncated]".to_string());
            }
        }

        if let Some(stderr) = output
            .get("stderr")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            let (rendered, truncated) = self.truncate_text(stderr);
            sections.push(format!("stderr:\n{rendered}"));
            if truncated {
                sections.push("[stderr truncated]".to_string());
            }
        }

        if sections.is_empty() {
            if let Some(message) = output
                .get("message")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
            {
                let (rendered, truncated) = self.truncate_text(message);
                sections.push(rendered);
                if truncated {
                    sections.push("[message truncated]".to_string());
                }
            } else {
                let summary =
                    serde_json::to_string_pretty(output).unwrap_or_else(|_| output.to_string());
                let (rendered, truncated) = self.truncate_text(&summary);
                sections.push(rendered);
                if truncated {
                    sections.push("[response truncated]".to_string());
                }
            }
        }

        if sections.is_empty() {
            sections.push(format!("{tool_name} completed without output"));
        }

        vec![acp::ToolCallContent::from(sections.join("\n\n"))]
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

        let locations = vec![acp::ToolCallLocation {
            path: path.clone(),
            line,
            meta: None,
        }];

        Ok(ToolExecutionReport::success(
            vec![acp::ToolCallContent::from(tool_content)],
            locations,
            payload,
        ))
    }

    async fn run_list_files(&self, args: &Value) -> Result<ToolExecutionReport, String> {
        let Some(tool) = &self.file_ops_tool else {
            return Err("List files tool is unavailable".to_string());
        };

        let resolved_path = self
            .resolve_list_files_path(args)?
            .unwrap_or_else(|| ".".to_string());

        let mut normalized_args = match args.clone() {
            Value::Object(map) => map,
            _ => serde_json::Map::new(),
        };
        normalized_args.insert(
            TOOL_LIST_FILES_PATH_ARG.to_string(),
            Value::String(resolved_path),
        );
        let normalized_args = Value::Object(normalized_args);

        let listing = tool.execute(normalized_args).await.map_err(|error| {
            let detail = error.to_string();
            warn!(error = %detail, "Failed to execute list_files tool");
            format!("Unable to list files: {detail}")
        })?;

        let content = Self::list_files_content(&listing);
        let locations = Self::list_files_locations(&listing);
        let payload = json!({
            TOOL_RESPONSE_KEY_STATUS: TOOL_SUCCESS_LABEL,
            TOOL_RESPONSE_KEY_TOOL: tools::LIST_FILES,
            TOOL_LIST_FILES_RESULT_KEY: listing,
        });

        Ok(ToolExecutionReport::success(content, locations, payload))
    }

    fn resolve_list_files_path(&self, args: &Value) -> Result<Option<String>, String> {
        if let Some(path) = args
            .get(TOOL_LIST_FILES_PATH_ARG)
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            return Ok(Some(path.to_string()));
        }

        if let Some(uri) = args
            .get(TOOL_LIST_FILES_URI_ARG)
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            let resolved = self.parse_resource_path(uri)?;
            let workspace_root = self.workspace_root().to_path_buf().clean();
            let normalized = resolved.clean();

            if normalized == workspace_root {
                return Ok(Some(".".to_string()));
            }

            if let Ok(relative) = normalized.strip_prefix(&workspace_root) {
                if relative.as_os_str().is_empty() {
                    return Ok(Some(".".to_string()));
                }
                return Ok(Some(relative.to_string_lossy().into()));
            }

            return Ok(Some(normalized.to_string_lossy().into()));
        }

        Ok(None)
    }

    fn list_files_content(output: &Value) -> Vec<acp::ToolCallContent> {
        let mut lines = Vec::new();

        if let (Some(count), Some(total)) = (
            output.get("count").and_then(Value::as_u64),
            output.get("total").and_then(Value::as_u64),
        ) {
            lines.push(format!("Showing {} of {} items", count, total));
        }

        if let Some(items) = output
            .get(TOOL_LIST_FILES_ITEMS_KEY)
            .and_then(Value::as_array)
        {
            if items.is_empty() {
                lines.push("No items found.".to_string());
            } else {
                for item in items.iter().take(TOOL_LIST_FILES_SUMMARY_MAX_ITEMS) {
                    let path = item
                        .get("path")
                        .and_then(Value::as_str)
                        .or_else(|| item.get("name").and_then(Value::as_str))
                        .unwrap_or("(unknown)");
                    let item_type = item.get("type").and_then(Value::as_str).unwrap_or("file");
                    let prefix = match item_type {
                        "directory" => "[dir]",
                        "file" => "[file]",
                        other => other,
                    };
                    lines.push(format!("{prefix} {path}"));
                }

                if items.len() > TOOL_LIST_FILES_SUMMARY_MAX_ITEMS {
                    let remaining = items.len() - TOOL_LIST_FILES_SUMMARY_MAX_ITEMS;
                    lines.push(format!("… and {remaining} more"));
                }
            }
        } else {
            lines.push("No results returned.".to_string());
        }

        if let Some(has_more) = output.get("has_more").and_then(Value::as_bool) {
            if has_more {
                lines.push(
                    "Additional results available (adjust page or per_page to view more)."
                        .to_string(),
                );
            }
        }

        if let Some(message) = output
            .get(TOOL_LIST_FILES_MESSAGE_KEY)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            lines.push(message.to_string());
        }

        if lines.is_empty() {
            lines.push("No results.".to_string());
        }

        vec![acp::ToolCallContent::from(lines.join("\n"))]
    }

    fn list_files_locations(output: &Value) -> Vec<acp::ToolCallLocation> {
        let Some(items) = output
            .get(TOOL_LIST_FILES_ITEMS_KEY)
            .and_then(Value::as_array)
        else {
            return Vec::new();
        };

        items
            .iter()
            .filter_map(|item| {
                item.get("path")
                    .or_else(|| item.get("name"))
                    .and_then(Value::as_str)
                    .map(PathBuf::from)
            })
            .take(TOOL_LIST_FILES_SUMMARY_MAX_ITEMS)
            .map(|path| acp::ToolCallLocation {
                path,
                line: None,
                meta: None,
            })
            .collect()
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

    fn parse_resource_path(&self, uri: &str) -> Result<PathBuf, String> {
        if uri.is_empty() {
            return Err(format!(
                "Unable to resolve URI provided to {}",
                tools::READ_FILE
            ));
        }

        if uri.starts_with('/') {
            let candidate = PathBuf::from(uri);
            return self.resolve_workspace_path(candidate, TOOL_READ_FILE_URI_ARG);
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

        self.resolve_workspace_path(path, TOOL_READ_FILE_URI_ARG)
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

        let path = match self.parse_resource_path(&link.uri) {
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
            Some(self.config.reasoning_effort)
        } else {
            None
        };

        let mut stop_reason = acp::StopReason::EndTurn;
        let mut assistant_message = String::new();
        let client_supports_read_text_file = self.client_supports_read_text_file();
        let provider_supports_tools = provider.supports_tools(&self.config.model);
        let availability =
            self.tool_availability(provider_supports_tools, client_supports_read_text_file);
        let mut enabled_tools = Vec::new();
        let mut disabled_tools = Vec::new();
        for (tool, runtime) in availability {
            match runtime {
                ToolRuntime::Enabled => enabled_tools.push(tool),
                ToolRuntime::Disabled(reason) => disabled_tools.push((tool, reason)),
            }
        }
        disabled_tools.sort_by_key(|(tool, _)| tool.sort_key());
        if !disabled_tools.is_empty() && self.should_send_tool_notice(&session) {
            for (tool, reason) in &disabled_tools {
                self.log_tool_disable_reason(*tool, reason);
            }
            self.send_tool_disable_notices(&args.session_id, &disabled_tools)
                .await?;
            self.mark_tool_notice_sent(&session);
        }

        let has_local_tools = self.acp_tool_registry.has_local_tools();
        let tools_allowed =
            provider_supports_tools && (!enabled_tools.is_empty() || has_local_tools);
        let tool_definitions = self.tool_definitions(provider_supports_tools, &enabled_tools);
        let mut messages = self.resolved_messages(&session);
        let allow_streaming = supports_streaming && !tools_allowed;

        let mut plan = PlanProgress::new(tools_allowed);
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
                tools: tool_definitions.clone(),
                model: self.config.model.clone(),
                max_tokens: None,
                temperature: None,
                stream: true,
                tool_choice: self.tool_choice(tools_allowed),
                parallel_tool_calls: None,
                parallel_tool_config: None,
                reasoning_effort,
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
                    tools: tool_definitions.clone(),
                    model: self.config.model.clone(),
                    max_tokens: None,
                    temperature: None,
                    stream: false,
                    tool_choice: self.tool_choice(tools_allowed),
                    parallel_tool_calls: None,
                    parallel_tool_config: None,
                    reasoning_effort,
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::collections::BTreeMap;
    use std::rc::Rc;
    use tempfile::tempdir;
    use tokio::fs;
    use vtcode_core::config::core::PromptCachingConfig;
    use vtcode_core::config::types::{
        AgentConfig as CoreAgentConfig, ModelSelectionSource, ReasoningEffortLevel,
        UiSurfacePreference,
    };
    use vtcode_core::config::{AgentClientProtocolZedConfig, ToolsConfig};
    use vtcode_core::core::agent::snapshots::{
        DEFAULT_CHECKPOINTS_ENABLED, DEFAULT_MAX_AGE_DAYS, DEFAULT_MAX_SNAPSHOTS,
    };

    fn build_agent(workspace: &Path) -> ZedAgent {
        let core_config = CoreAgentConfig {
            model: "test-model".to_string(),
            api_key: String::new(),
            provider: "test-provider".to_string(),
            api_key_env: "TEST_API_KEY".to_string(),
            workspace: workspace.to_path_buf(),
            verbose: false,
            theme: "test".to_string(),
            reasoning_effort: ReasoningEffortLevel::Low,
            ui_surface: UiSurfacePreference::default(),
            prompt_cache: PromptCachingConfig::default(),
            model_source: ModelSelectionSource::WorkspaceConfig,
            custom_api_keys: BTreeMap::new(),
            checkpointing_enabled: DEFAULT_CHECKPOINTS_ENABLED,
            checkpointing_storage_dir: None,
            checkpointing_max_snapshots: DEFAULT_MAX_SNAPSHOTS,
            checkpointing_max_age_days: Some(DEFAULT_MAX_AGE_DAYS),
        };

        let mut zed_config = AgentClientProtocolZedConfig::default();
        zed_config.tools.list_files = true;
        zed_config.tools.read_file = false;

        let tools_config = ToolsConfig::default();
        let (tx, _rx) = mpsc::unbounded_channel();

        ZedAgent::new(core_config, zed_config, tools_config, String::new(), tx)
    }

    fn list_items_from_payload(payload: &Value) -> Vec<Value> {
        payload
            .get(TOOL_LIST_FILES_RESULT_KEY)
            .and_then(Value::as_object)
            .and_then(|result| result.get(TOOL_LIST_FILES_ITEMS_KEY))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    }

    #[tokio::test]
    async fn run_list_files_defaults_to_workspace_root() {
        let temp = tempdir().unwrap();
        let file_path = temp.path().join("example.txt");
        fs::write(&file_path, "hello").await.unwrap();

        let agent = build_agent(temp.path());
        let report = agent.run_list_files(&json!({})).await.unwrap();

        assert!(matches!(report.status, acp::ToolCallStatus::Completed));
        let payload = report.raw_output.unwrap();
        let items = list_items_from_payload(&payload);
        assert!(items.iter().any(|item| {
            item.get("name")
                .and_then(Value::as_str)
                .map(|name| name == "example.txt")
                .unwrap_or(false)
        }));
    }

    #[tokio::test]
    async fn run_list_files_accepts_uri_argument() {
        let temp = tempdir().unwrap();
        let nested = temp.path().join("nested");
        fs::create_dir_all(&nested).await.unwrap();
        let inner = nested.join("inner.txt");
        fs::write(&inner, "data").await.unwrap();

        let agent = build_agent(temp.path());
        let uri = format!("file://{}", nested.to_string_lossy());
        let report = agent
            .run_list_files(&json!({ TOOL_LIST_FILES_URI_ARG: uri }))
            .await
            .unwrap();

        assert!(matches!(report.status, acp::ToolCallStatus::Completed));
        let payload = report.raw_output.unwrap();
        let items = list_items_from_payload(&payload);
        assert!(items.iter().any(|item| {
            item.get("path")
                .and_then(Value::as_str)
                .map(|path| path.contains("inner.txt"))
                .unwrap_or(false)
        }));
    }
}
