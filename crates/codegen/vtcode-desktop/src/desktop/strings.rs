//! Shared string vocabulary for the desktop client.
//!
//! Every literal here is used from at least two production sites, or belongs
//! to a protocol/bundle contract table whose keys must stay spelled as one
//! (a typo breaks the service silently). Other single-use literals stay at
//! their use site; test fixtures keep their own literals as independent
//! oracles.

/// JSON-RPC envelope and protocol param keys.
///
/// Read/write keys are shared by the view and the transport; single-side keys
/// face the backend's typed params. A typo anywhere breaks request handling
/// silently, so all sides reference this table. Wire-format test fixtures
/// keep literals.
pub(super) mod keys {
    pub const JSONRPC: &str = "jsonrpc";
    pub const JSONRPC_VERSION: &str = "2.0";
    pub const ID: &str = "id";
    pub const METHOD: &str = "method";
    pub const PARAMS: &str = "params";
    pub const RESULT: &str = "result";
    pub const ERROR: &str = "error";
    pub const CODE: &str = "code";
    pub const MESSAGE: &str = "message";
    pub const SESSION_ID: &str = "sessionId";
    pub const REQUEST_ID: &str = "requestId";
    pub const TURN_ID: &str = "turnId";
    pub const INTERACTION_ID: &str = "interactionId";
    pub const CANCELLED: &str = "cancelled";
    pub const PROMPT: &str = "prompt";
    pub const ATTACHMENTS: &str = "attachments";
    pub const BEFORE_INDEX: &str = "beforeIndex";
    pub const LIMIT: &str = "limit";
    pub const ITEM_ID: &str = "itemId";
    pub const OFFSET: &str = "offset";
    pub const URI: &str = "uri";
    pub const STATUS: &str = "status";
    pub const SCOPE: &str = "scope";
    pub const INCLUDE_UNTRACKED: &str = "includeUntracked";
}

/// App identity strings shown in menus, window chrome, notifications, and
/// transcript titles.
pub(super) mod app {
    pub const NAME: &str = "VT Code";
}

/// GPUI key-context names shared by bindings, key-context scopes, and
/// tooltip actions. A typo detaches shortcuts silently.
pub(super) mod key_context {
    pub const DESKTOP: &str = "VTCodeDesktop";
    pub const SETTINGS: &str = "VTCodeSettings";
    pub const DIFF_REVIEW: &str = "VTCodeDiffReview";
}

/// macOS app-bundle layout fragments shared by backend resolution and
/// editor detection.
pub(super) mod bundle {
    pub const MACOS_DIR: &str = "MacOS";
    pub const CONTENTS_DIR: &str = "Contents";
    pub const APP_EXTENSION: &str = "app";
    pub const INFO_PLIST: &str = "Info.plist";
    pub const HELPERS_VTCODE: &str = "Helpers/vtcode";
}
