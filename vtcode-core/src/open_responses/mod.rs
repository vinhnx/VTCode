//! Open Responses specification conformance layer.
//!
//! This module implements the [Open Responses](https://www.openresponses.org/) specification
//! for vendor-neutral LLM interfaces. It provides:
//!
//! - Unified item types with state machine semantics
//! - Semantic streaming events (not raw token deltas)
//! - Response objects with standardized structure
//! - Error handling with structured error types
//! - Extension points for VT Code-specific item types
//!
//! The implementation bridges VT Code's internal event system (`ThreadEvent`)
//! to Open Responses-compliant structures while maintaining backwards compatibility.

mod bridge;
mod content;
mod error;
mod events;
mod integration;
mod items;
mod request;
mod response;
mod status;
mod usage;

pub use bridge::{DualEventEmitter, ResponseBuilder};
pub use content::{ContentPart, ContentPartId, ImageDetail, InputFileContent, InputImageContent};
pub use error::{OpenResponseError, OpenResponseErrorCode, OpenResponseErrorType};
pub use events::{ResponseStreamEvent, SequencedEvent, StreamEventEmitter, VecStreamEmitter};
pub use integration::{
    OpenResponsesCallback, OpenResponsesIntegration, OpenResponsesProvider, ToOpenResponse,
};
pub use items::{
    CustomItem, FunctionCallItem, FunctionCallOutputItem, MessageItem, MessageRole, OutputItem,
    OutputItemId, ReasoningItem,
};
pub use request::{Request, SpecificToolChoice, ToolChoice, ToolChoiceMode};
pub use response::{
    IncompleteDetails, IncompleteReason, Response, ResponseId, ResponseStatus, generate_item_id,
    generate_response_id,
};
pub use status::ItemStatus;
pub use usage::{InputTokensDetails, OpenUsage, OutputTokensDetails};

/// VT Code extension prefix for custom item types and events.
pub const VTCODE_EXTENSION_PREFIX: &str = "vtcode";

/// Validates that a custom type follows the Open Responses extension naming convention.
/// Custom types must be prefixed with an implementor slug (e.g., `vtcode:file_change`).
pub fn is_valid_extension_type(type_name: &str) -> bool {
    if let Some((prefix, name)) = type_name.split_once(':') {
        !prefix.is_empty()
            && !name.is_empty()
            && prefix
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_extension_types() {
        assert!(is_valid_extension_type("vtcode:file_change"));
        assert!(is_valid_extension_type("acme:search_result"));
        assert!(is_valid_extension_type("openai:web_search_call"));
    }

    #[test]
    fn test_invalid_extension_types() {
        assert!(!is_valid_extension_type("file_change"));
        assert!(!is_valid_extension_type(":file_change"));
        assert!(!is_valid_extension_type("vtcode:"));
        assert!(!is_valid_extension_type("vt-code:file_change"));
        assert!(!is_valid_extension_type(""));
    }
}
