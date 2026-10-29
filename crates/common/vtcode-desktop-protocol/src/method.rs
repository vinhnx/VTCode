//! Known desktop JSON-RPC methods.
/// Initialize and negotiate protocol support.
pub const INITIALIZE: &str = "desktop/initialize";
/// Capture a bounded read-only Git preview, including valid unborn branches.
pub const WORKSPACE_REVIEW: &str = "workspace/review";
/// List sessions in the selected workspace.
pub const SESSION_LIST: &str = "session/list";
/// Create a session.
pub const SESSION_CREATE: &str = "session/create";
/// Load a durable session.
pub const SESSION_LOAD: &str = "session/load";
/// Load one bounded earlier page from the selected durable session.
pub const SESSION_HISTORY: &str = "session/history";
/// Read one bounded page of archived tool output on demand.
pub const SESSION_OUTPUT_READ: &str = "session/output/read";
/// Close the selected session after cleanup.
pub const SESSION_CLOSE: &str = "session/close";
/// Submit a user turn.
pub const TURN_SUBMIT: &str = "turn/submit";
/// Cancel a running turn.
pub const TURN_CANCEL: &str = "turn/cancel";
/// Respond to a pending interaction.
pub const INTERACTION_RESPOND: &str = "interaction/respond";
/// Discover active provider/model setup.
pub const CONFIG_DISCOVER: &str = "config/discover";
/// Change runtime session options while idle.
pub const CONFIG_OPTIONS: &str = "config/options";
/// Persist the selected built-in CLI theme.
pub const CONFIG_THEME: &str = "config/theme";
/// Read a bounded, credential-filtered CLI settings catalog.
pub const CONFIG_SETTINGS_READ: &str = "config/settings/read";
/// Persist one CLI setting through the shared configuration service.
pub const CONFIG_SETTINGS_WRITE: &str = "config/settings/write";
/// Discover installed CLI skills and Agent Plugins.
pub const COMPOSER_RESOURCES: &str = "composer/resources";
/// List selectable models from the service's catalog.
pub const CONFIG_MODELS: &str = "config/models";
/// Select the provider/model used for the next session.
pub const CONFIG_SELECT: &str = "config/select";
/// Shut down the local service after owned tasks finish.
pub const SHUTDOWN: &str = "desktop/shutdown";
/// Stream an authoritative runtime event.
pub const THREAD_EVENT: &str = "thread/event";
/// Ask the client to resolve a runtime interaction.
pub const INTERACTION_REQUESTED: &str = "interaction/requested";
/// Publish the active primary agent for a live session.
pub const PRIMARY_AGENT_CHANGED: &str = "session/primaryAgentChanged";
/// Publish visible turn state.
pub const TURN_STATUS: &str = "turn/status";
/// Read bridge state and the non-secret configured origin list.
pub const WEBMCP_STATUS: &str = "webmcp/status";
/// Start or reuse the WebMCP bridge and issue an origin-bound pairing code.
pub const WEBMCP_PAIR: &str = "webmcp/pair";
/// Revoke the bridge's pairings and stop its listener.
pub const WEBMCP_STOP: &str = "webmcp/stop";
