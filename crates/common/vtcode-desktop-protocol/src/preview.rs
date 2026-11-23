//! Ephemeral computer-use screenshot preview DTOs.
//!
//! Transcript rows carry a `computer-preview://` reference (row id plus byte
//! metadata, no pixels); the client resolves it through `computer/preview/read`
//! while the service still retains the bytes. Retention is memory-only and
//! bounded, so reads answer either the PNG body or an explicit expiry —
//! never an error the client should retry.

use serde::{Deserialize, Serialize};

/// URI scheme for computer-preview references. The remainder is the outer
/// tool row id verbatim; no filesystem or network resource is named.
pub const COMPUTER_PREVIEW_URI_SCHEME: &str = "computer-preview://";

/// Longest accepted reference. Row ids are short harness handles; anything
/// longer is rejected before the lookup.
pub const MAX_COMPUTER_PREVIEW_URI_BYTES: usize = 512;

/// Parse and validate a preview reference, returning the row id. Rejects
/// foreign schemes, empty ids, whitespace, and overlong input before any
/// lookup; a well-formed but unknown id is a lookup miss (expired), not a
/// validation failure.
pub fn parse_preview_uri(uri: &str) -> Option<&str> {
    let id = uri.strip_prefix(COMPUTER_PREVIEW_URI_SCHEME)?;
    if id.is_empty() || id.len() > MAX_COMPUTER_PREVIEW_URI_BYTES || id.chars().any(char::is_whitespace) {
        return None;
    }
    Some(id)
}

/// Reference URI for a row's screenshot.
pub fn preview_uri(item_id: &str) -> String {
    format!("{COMPUTER_PREVIEW_URI_SCHEME}{item_id}")
}

/// Request one retained screenshot body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerPreviewReadParams {
    /// `computer-preview://` URI to read.
    pub uri: String,
    /// Opaque client identity used to reject superseded reads.
    pub request_id: String,
}

/// One retained screenshot body, or its expiry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerPreviewReadResult {
    /// URI that was read.
    pub uri: String,
    /// Echoed request identity.
    pub request_id: String,
    /// MIME type of the retained bytes.
    pub mime_type: Option<String>,
    /// Base64 PNG body; absent when the bytes expired.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    /// Served PNG bytes (post-scaling).
    pub total_bytes: u64,
    /// True when the served body was downscaled to fit the frame budget.
    #[serde(default)]
    pub downscaled: bool,
    /// True when the bytes are gone (evicted, cleared, or unknown): the
    /// client renders the expired state and never retries.
    pub expired: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_uris_round_trip_and_reject_foreign_shapes() {
        assert_eq!(preview_uri("row-1"), "computer-preview://row-1");
        assert_eq!(parse_preview_uri("computer-preview://row-1"), Some("row-1"));
        assert_eq!(parse_preview_uri("computer-preview://call_1:ab12"), Some("call_1:ab12"));
        for bad in [
            String::new(),
            "row-1".to_string(),
            "ui://row-1".to_string(),
            "computer-preview://".to_string(),
            "computer-preview://has space".to_string(),
            "computer-preview://has\nnewline".to_string(),
            format!("computer-preview://{}", "x".repeat(MAX_COMPUTER_PREVIEW_URI_BYTES + 1)),
        ] {
            assert!(parse_preview_uri(&bad).is_none(), "must reject {bad:?}");
        }
    }

    #[test]
    fn preview_result_marks_expiry_without_a_body() {
        let expired = ComputerPreviewReadResult {
            uri: preview_uri("gone"),
            request_id: "req-1".to_string(),
            mime_type: None,
            data: None,
            total_bytes: 0,
            downscaled: false,
            expired: true,
        };
        let json = serde_json::to_value(&expired).unwrap();
        assert_eq!(json["expired"], serde_json::json!(true));
        assert!(json.get("data").is_none(), "expired reads carry no body: {json}");
        assert_eq!(serde_json::from_value::<ComputerPreviewReadResult>(json).unwrap(), expired);
    }
}
