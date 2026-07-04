https://github.com/searxng/searxng

---

- Systematically removing the remaining ~8,128 unwrap/expect/panic sites.

---

- Enabling #![warn(missing_docs)] project-wide.

---

implement automatically download any new updates when vtcode starts up.

---

fix cargo warnings

Compiling vtcode-acp v0.134.3 (/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-acp)
warning: unused imports: `SESSION_CONFIG_MODEL_ID`, `SESSION_CONFIG_PRIMARY_AGENT_ID`, `SESSION_CONFIG_PROVIDER_ID`, and `SESSION_CONFIG_THOUGHT_LEVEL_ID`
--> vtcode-acp/src/zed/agent/handlers.rs:32:5
|
32 | SESSION_CONFIG_MODEL_ID, SESSION_CONFIG_PRIMARY_AGENT_ID, SESSION_CONFIG_PROVIDER_ID,
| ^^^^^^^^^^^^^^^^^^^^^^^ ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ ^^^^^^^^^^^^^^^^^^^^^^^^^^
33 | SESSION_CONFIG_THOUGHT_LEVEL_ID, agent_implementation_info, text_chunk,
| ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
|
= note: `#[warn(unused_imports)]` on by default

warning: unused import: `vtcode_core::config::types::ReasoningEffortLevel`
--> vtcode-acp/src/zed/agent/handlers.rs:54:5
|
54 | use vtcode_core::config::types::ReasoningEffortLevel;
| ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: unnecessary qualification
--> vtcode-acp/src/zed/agent/handlers.rs:420:14
|
420 | .map(std::sync::Arc::new);
| ^^^^^^^^^^^^^^^^^^^
|
= note: requested on the command line with `-W unused-qualifications`
help: remove the unnecessary path segments
|
420 - .map(std::sync::Arc::new);
420 + .map(Arc::new);
|

warning: unnecessary qualification
--> vtcode-acp/src/zed/agent/handlers.rs:626:26
|
626 | .map(std::sync::Arc::new);
| ^^^^^^^^^^^^^^^^^^^
|
help: remove the unnecessary path segments
|
626 - .map(std::sync::Arc::new);
626 + .map(Arc::new);
|

warning: unnecessary qualification
--> vtcode-acp/src/zed/agent/session_state.rs:130:27
|
130 | .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
| ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
|
help: remove the unnecessary path segments
|
130 - .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
130 + .fetch_add(1, Ordering::Relaxed);
|

warning: unnecessary qualification
--> vtcode-acp/src/zed/agent/tool_config.rs:265:21
|
265 | ) -> Result<(), crate::acp::Error> {
| ^^^^^^^^^^^^^^^^^
|
help: remove the unnecessary path segments
|
265 - ) -> Result<(), crate::acp::Error> {
265 + ) -> Result<(), acp::Error> {
|

warning: unnecessary qualification
--> vtcode-acp/src/zed/agent/updates.rs:66:21
|
66 | ) -> Result<(), crate::acp::Error> {
| ^^^^^^^^^^^^^^^^^
|
help: remove the unnecessary path segments
|
66 - ) -> Result<(), crate::acp::Error> {
66 + ) -> Result<(), acp::Error> {
|

warning: unused import: `agent::ZedAgent`
--> vtcode-acp/src/zed/mod.rs:12:16
|
12 | pub(crate) use agent::ZedAgent;
| ^^^^^^^^^^^^^^^

warning: unused imports: `Agent`, `Client`, and `Error as SdkError`
--> vtcode-acp/src/lib.rs:34:44
|
34 | pub(crate) use agent_client_protocol::{Agent, Client, Error as SdkError};
| ^^^^^ ^^^^^^ ^^^^^^^^^^^^^^^^^

warning: unnecessary qualification
--> vtcode-acp/src/lib.rs:69:37
|
69 | static ACP_CONNECTION: OnceLock<Arc<crate::zed::connection::ConnectionHandle>> = OnceLock::new();
| ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
|
help: remove the unnecessary path segments
|
69 - static ACP_CONNECTION: OnceLock<Arc<crate::zed::connection::ConnectionHandle>> = OnceLock::new();
69 + static ACP_CONNECTION: OnceLock<Arc<zed::connection::ConnectionHandle>> = OnceLock::new();
|

warning: unnecessary qualification
--> vtcode-acp/src/lib.rs:76:21
|
76 | connection: Arc<crate::zed::connection::ConnectionHandle>,
| ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
|
help: remove the unnecessary path segments
|
76 - connection: Arc<crate::zed::connection::ConnectionHandle>,
76 + connection: Arc<zed::connection::ConnectionHandle>,
|

warning: unnecessary qualification
--> vtcode-acp/src/lib.rs:77:21
|
77 | ) -> Result<(), Arc<crate::zed::connection::ConnectionHandle>> {
| ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
|
help: remove the unnecessary path segments
|
77 - ) -> Result<(), Arc<crate::zed::connection::ConnectionHandle>> {
77 + ) -> Result<(), Arc<zed::connection::ConnectionHandle>> {
|

warning: unnecessary qualification
--> vtcode-acp/src/lib.rs:82:39
|
82 | pub fn acp_connection() -> Option<Arc<crate::zed::connection::ConnectionHandle>> {
| ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
|
help: remove the unnecessary path segments
|
82 - pub fn acp_connection() -> Option<Arc<crate::zed::connection::ConnectionHandle>> {
82 + pub fn acp_connection() -> Option<Arc<zed::connection::ConnectionHandle>> {
|

warning: methods `should_send_tool_notice` and `mark_tool_notice_sent` are never used
--> vtcode-acp/src/zed/agent/session_state.rs:163:19
|
19 | impl ZedAgent {
| ------------- methods in this implementation
...
163 | pub(super) fn should_send_tool_notice(&self, session: &SessionHandle) -> bool {
| ^^^^^^^^^^^^^^^^^^^^^^^
...
171 | pub(super) fn mark_tool_notice_sent(&self, session: &SessionHandle) {
| ^^^^^^^^^^^^^^^^^^^^^
|
= note: `#[warn(dead_code)]` on by default

warning: methods `render_tool_disable_notice`, `log_tool_disable_reason`, and `send_tool_disable_notices` are never used
--> vtcode-acp/src/zed/agent/tool_config.rs:220:19
|
19 | impl ZedAgent {
| ------------- methods in this implementation
...
220 | pub(super) fn render_tool_disable_notice(
| ^^^^^^^^^^^^^^^^^^^^^^^^^^
...
237 | pub(super) fn log_tool_disable_reason(
| ^^^^^^^^^^^^^^^^^^^^^^^
...
261 | pub(super) async fn send_tool_disable_notices(
| ^^^^^^^^^^^^^^^^^^^^^^^^^

warning: constant `TOOL_DISABLED_PROVIDER_NOTICE` is never used
--> vtcode-acp/src/zed/constants.rs:9:18
|
9 | pub(crate) const TOOL_DISABLED_PROVIDER_NOTICE: &str =
| ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: constant `TOOL_DISABLED_CAPABILITY_NOTICE` is never used
--> vtcode-acp/src/zed/constants.rs:11:18
|
11 | pub(crate) const TOOL_DISABLED_CAPABILITY_NOTICE: &str =
| ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: constant `TOOL_DISABLED_PROVIDER_LOG_MESSAGE` is never used
--> vtcode-acp/src/zed/constants.rs:13:18
|
13 | pub(crate) const TOOL_DISABLED_PROVIDER_LOG_MESSAGE: &str =
| ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: constant `TOOL_DISABLED_CAPABILITY_LOG_MESSAGE` is never used
--> vtcode-acp/src/zed/constants.rs:15:18
|
15 | pub(crate) const TOOL_DISABLED_CAPABILITY_LOG_MESSAGE: &str =
| ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: fields `provider` and `model` are never read
--> vtcode-acp/src/zed/types.rs:18:16
|
18 | Provider { provider: &'a str, model: &'a str },
| -------- ^^^^^^^^ ^^^^^
| |
| fields in this variant
|
= note: `ToolDisableReason` has a derived impl for the trait `Clone`, but this is intentionally ignored during dead code analysis

warning: field `tool_notice_sent` is never read
--> vtcode-acp/src/zed/types.rs:169:16
|
166 | pub(crate) struct SessionData {
| ----------- field in this struct
...
169 | pub(crate) tool_notice_sent: AtomicBool,
| ^^^^^^^^^^^^^^^^

warning: unused `Result` that must be used
--> vtcode-acp/src/zed/session.rs:116:21
|
116 | pending::<agent*client_protocol::Result<()>>().await;
| ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
|
= note: this `Result` may be an `Err` variant, which should be handled
= note: `#[warn(unused_must_use)]` on by default
help: use `let * = ...` to ignore the resulting value
|
116 | let \_ = pending::<agent_client_protocol::Result<()>>().await;
| +++++++

warning: `vtcode-acp` (lib) generated 22 warnings (run `cargo fix --lib -p vtcode-acp` to apply 13 suggestions)
