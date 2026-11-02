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
/// Read one bounded MCP app-resource body for static preview.
pub const MCP_APP_READ: &str = "mcp/app/read";
/// Close the selected session after cleanup.
pub const SESSION_CLOSE: &str = "session/close";
/// Delete one session archive by identifier.
pub const SESSION_DELETE: &str = "session/delete";
/// Read on-disk usage broken down by owning store.
pub const STORAGE_SUMMARY: &str = "storage/summary";
/// List managed loop-isolation worktrees.
pub const WORKTREE_LIST: &str = "worktree/list";
/// Remove one managed worktree by name.
pub const WORKTREE_REMOVE: &str = "worktree/remove";
/// Submit a user turn.
pub const TURN_SUBMIT: &str = "turn/submit";
/// Cancel a running turn.
pub const TURN_CANCEL: &str = "turn/cancel";
/// Pop the newest queued message back for editing.
pub const TURN_QUEUE_TAKE_LATEST: &str = "turn/queue/take-latest";
/// Force-terminate the selected session's foreground exec sessions.
pub const EXEC_FORCE_CANCEL: &str = "session/exec/force-cancel";
/// Move the selected session's foreground exec session to the background.
pub const EXEC_BACKGROUND: &str = "session/exec/background";
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
/// Open one layered config file in the platform default editor.
pub const CONFIG_OPEN_FILE: &str = "config/openFile";
/// Discover installed CLI skills and Agent Plugins.
pub const COMPOSER_RESOURCES: &str = "composer/resources";
/// List selectable models from the service's catalog.
pub const CONFIG_MODELS: &str = "config/models";
/// Select the provider/model for the selected session. The service restarts
/// the session runtime under the same session id; the thread is preserved.
pub const CONFIG_SELECT: &str = "config/select";
/// Store a provider API key from the desktop client in secure storage.
/// The secret travels the local channel only and is never echoed back.
pub const CONFIG_SECRET_STORE: &str = "config/secret/store";
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
/// Publish the authoritative queued-message snapshot.
pub const TURN_QUEUE_STATUS: &str = "turn/queue-status";
/// Read bridge state and the non-secret configured origin list.
pub const WEBMCP_STATUS: &str = "webmcp/status";
/// Start or reuse the WebMCP bridge and issue an origin-bound pairing code.
pub const WEBMCP_PAIR: &str = "webmcp/pair";
/// Revoke the bridge's pairings and stop its listener.
pub const WEBMCP_STOP: &str = "webmcp/stop";
/// Read the cached updater status (current/latest version, no network I/O).
pub const UPDATE_STATUS: &str = "update/status";

/// Transient UI progress for the active desktop session.
pub const UI_PROGRESS: &str = "session/ui_progress";
