//! JSON helper utilities for serializing and deserializing thread events.
use super::{ThreadEvent, VersionedThreadEvent};

/// Converts an event into a `serde_json::Value`.
pub fn to_value(event: &ThreadEvent) -> serde_json::Result<serde_json::Value> {
    serde_json::to_value(event)
}

/// Serializes an event into a JSON string.
pub(crate) fn to_string(event: &ThreadEvent) -> serde_json::Result<String> {
    serde_json::to_string(event)
}

/// Deserializes an event from a JSON string.
pub fn from_str(payload: &str) -> serde_json::Result<ThreadEvent> {
    serde_json::from_str(payload)
}

/// Serializes a [`VersionedThreadEvent`] wrapper.
pub(crate) fn versioned_to_string(event: &ThreadEvent) -> serde_json::Result<String> {
    serde_json::to_string(&VersionedThreadEvent::new(event.clone()))
}

/// Deserializes a [`VersionedThreadEvent`] wrapper.
pub(crate) fn versioned_from_str(payload: &str) -> serde_json::Result<VersionedThreadEvent> {
    serde_json::from_str(payload)
}
