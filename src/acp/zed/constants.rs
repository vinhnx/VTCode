use vtcode_core::config::constants::tools;

pub(crate) const SESSION_PREFIX: &str = "vtcode-zed-session";
pub(crate) const RESOURCE_FALLBACK_LABEL: &str = "Resource";
pub(crate) const RESOURCE_FAILURE_LABEL: &str = "Resource unavailable";
pub(crate) const RESOURCE_CONTEXT_OPEN: &str = "<context";
pub(crate) const RESOURCE_CONTEXT_CLOSE: &str = "</context>";
pub(crate) const RESOURCE_CONTEXT_URI_ATTR: &str = "uri";
pub(crate) const RESOURCE_CONTEXT_NAME_ATTR: &str = "name";
pub(crate) const MAX_TOOL_RESPONSE_CHARS: usize = 32_768;
pub(crate) const TOOL_DISABLED_PROVIDER_NOTICE: &str =
    "Skipping {tool} tool: model {model} on {provider} does not support function calling";
pub(crate) const TOOL_DISABLED_CAPABILITY_NOTICE: &str =
    "Skipping {tool} tool: client does not advertise fs.readTextFile capability";
pub(crate) const TOOL_DISABLED_PROVIDER_LOG_MESSAGE: &str =
    "ACP tool disabled because the selected model does not support function calling";
pub(crate) const TOOL_DISABLED_CAPABILITY_LOG_MESSAGE: &str =
    "ACP tool disabled because the client lacks fs.readTextFile support";
pub(crate) const INITIALIZE_VERSION_MISMATCH_LOG: &str =
    "Client requested unsupported ACP protocol version; responding with v1";
pub(crate) const TOOL_READ_FILE_INVALID_INTEGER_TEMPLATE: &str =
    "Invalid {argument} value: expected a positive integer";
pub(crate) const TOOL_READ_FILE_INTEGER_RANGE_TEMPLATE: &str =
    "{argument} value exceeds the supported range";
pub(crate) const TOOL_READ_FILE_ABSOLUTE_PATH_TEMPLATE: &str =
    "Invalid {argument} value: expected an absolute path";
pub(crate) const TOOL_READ_FILE_WORKSPACE_ESCAPE_TEMPLATE: &str =
    "Invalid {argument} value: path escapes the trusted workspace";
pub(crate) const PLAN_STEP_ANALYZE: &str =
    "Review the latest user request and conversation context";
pub(crate) const PLAN_STEP_GATHER_CONTEXT: &str = "Gather referenced workspace files when required";
pub(crate) const PLAN_STEP_RESPOND: &str = "Compose and send the assistant response";
pub(crate) const WORKSPACE_TRUST_UPGRADE_LOG: &str = "ACP workspace trust level updated";
pub(crate) const WORKSPACE_TRUST_ALREADY_SATISFIED_LOG: &str =
    "ACP workspace trust level already satisfied";
pub(crate) const WORKSPACE_TRUST_DOWNGRADE_SKIPPED_LOG: &str =
    "ACP workspace trust downgrade skipped because workspace is fully trusted";

/// Tools that are not exposed via ACP because the protocol has native support for these features.
///
/// - Plan mode tools: ACP has built-in session modes (ask/architect/code) - see https://agentclientprotocol.com/protocol/session-modes.md
/// - HITL tools: ACP has native permission request mechanism
pub(crate) const TOOLS_EXCLUDED_FROM_ACP: &[&str] = &[
    tools::ENTER_PLAN_MODE,
    tools::EXIT_PLAN_MODE,
    tools::REQUEST_USER_INPUT,
    tools::ASK_QUESTIONS,
    tools::ASK_USER_QUESTION,
];

/// ACP Session Mode identifiers (aligned with https://agentclientprotocol.com/protocol/session-modes.md)
pub(crate) const MODE_ID_ASK: &str = "ask";
pub(crate) const MODE_ID_ARCHITECT: &str = "architect";
pub(crate) const MODE_ID_CODE: &str = "code";
