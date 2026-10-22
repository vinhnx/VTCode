use serde::{Deserialize, Deserializer, Serialize};

use crate::llm::provider::{AssistantPhase, Message, MessageClearAt, MessageContent, MessageRole, ToolCall};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionMessage {
    pub role: MessageRole,
    pub content: MessageContent,
    // Keep sparse metadata boxed so long histories don't pay large inline Option<T> costs.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_boxed_non_empty_string_opt"
    )]
    pub reasoning: Option<Box<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_boxed_non_empty_vec_opt"
    )]
    pub reasoning_details: Option<Box<Vec<serde_json::Value>>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_boxed_non_empty_vec_opt"
    )]
    pub tool_calls: Option<Box<Vec<ToolCall>>>,
    #[serde(default, deserialize_with = "deserialize_boxed_non_empty_string_opt")]
    pub tool_call_id: Option<Box<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<AssistantPhase>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_boxed_non_empty_string_opt"
    )]
    pub origin_tool: Option<Box<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_boxed_metadata_opt"
    )]
    pub metadata: Option<Box<crate::core::message_metadata::MessageMetadata>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clear_at: Option<MessageClearAt>,
}

impl Eq for SessionMessage {}

#[expect(
    clippy::box_collection,
    reason = "Intentional compatibility, platform, test, or API-shape suppression."
)]
#[inline]
fn boxed_non_empty_string(value: Option<String>) -> Option<Box<String>> {
    value.and_then(|value| (!value.is_empty()).then_some(Box::new(value)))
}

#[expect(
    clippy::box_collection,
    reason = "Intentional compatibility, platform, test, or API-shape suppression."
)]
#[inline]
fn boxed_non_empty_vec<T>(value: Option<Vec<T>>) -> Option<Box<Vec<T>>> {
    value.and_then(|value| (!value.is_empty()).then_some(Box::new(value)))
}

fn clone_non_empty_string(value: Option<&str>) -> Option<String> {
    value.filter(|value| !value.is_empty()).map(str::to_owned)
}

fn deserialize_boxed_metadata_opt<'de, D>(
    deserializer: D,
) -> Result<Option<Box<crate::core::message_metadata::MessageMetadata>>, D::Error>
where
    D: Deserializer<'de>,
{
    crate::core::message_metadata::MessageMetadata::deserialize(deserializer).map(|m| Some(Box::new(m)))
}

fn clone_non_empty_vec<T: Clone>(value: Option<&[T]>) -> Option<Vec<T>> {
    value.filter(|value| !value.is_empty()).map(<[T]>::to_vec)
}

#[expect(
    clippy::box_collection,
    reason = "Intentional compatibility, platform, test, or API-shape suppression."
)]
fn deserialize_boxed_non_empty_string_opt<'de, D>(deserializer: D) -> Result<Option<Box<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(boxed_non_empty_string)
}

#[expect(
    clippy::box_collection,
    reason = "Intentional compatibility, platform, test, or API-shape suppression."
)]
fn deserialize_boxed_non_empty_vec_opt<'de, D, T>(deserializer: D) -> Result<Option<Box<Vec<T>>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<Vec<T>>::deserialize(deserializer).map(boxed_non_empty_vec)
}

impl SessionMessage {
    fn base(role: MessageRole, content: MessageContent) -> Self {
        Self {
            role,
            content,
            reasoning: None,
            reasoning_details: None,
            tool_calls: None,
            tool_call_id: None,
            phase: None,
            origin_tool: None,
            metadata: None,
            clear_at: None,
        }
    }

    pub fn new(role: MessageRole, content: impl Into<String>) -> Self {
        Self::base(role, MessageContent::Text(content.into()))
    }

    pub fn with_content(role: MessageRole, content: MessageContent) -> Self {
        Self::base(role, content)
    }

    pub fn with_tool_call_id(role: MessageRole, content: impl Into<String>, tool_call_id: Option<String>) -> Self {
        Self::with_tool_call_id_content(role, MessageContent::Text(content.into()), tool_call_id)
    }

    pub fn with_tool_call_id_content(role: MessageRole, content: MessageContent, tool_call_id: Option<String>) -> Self {
        let mut message = Self::base(role, content);
        message.tool_call_id = boxed_non_empty_string(tool_call_id);
        message
    }
}

impl From<&Message> for SessionMessage {
    fn from(message: &Message) -> Self {
        Self {
            role: message.role,
            content: message.content.clone(),
            reasoning: boxed_non_empty_string(message.reasoning.clone()),
            reasoning_details: boxed_non_empty_vec(message.reasoning_details.clone()),
            tool_calls: boxed_non_empty_vec(message.tool_calls.clone()),
            tool_call_id: boxed_non_empty_string(message.tool_call_id.clone()),
            phase: message.phase,
            origin_tool: boxed_non_empty_string(message.origin_tool.clone()),
            metadata: message.metadata.clone().map(Box::new),
            clear_at: message.clear_at,
        }
    }
}

impl From<&SessionMessage> for Message {
    fn from(message: &SessionMessage) -> Self {
        Self {
            role: message.role,
            content: message.content.clone(),
            reasoning: clone_non_empty_string(message.reasoning.as_deref().map(String::as_str)),
            reasoning_details: clone_non_empty_vec(message.reasoning_details.as_deref().map(Vec::as_slice)),
            tool_calls: clone_non_empty_vec(message.tool_calls.as_deref().map(Vec::as_slice)),
            tool_call_id: clone_non_empty_string(message.tool_call_id.as_deref().map(String::as_str)),
            phase: message.phase,
            origin_tool: clone_non_empty_string(message.origin_tool.as_deref().map(String::as_str)),
            metadata: message.metadata.as_deref().cloned(),
            clear_at: message.clear_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SessionMessage;
    use crate::llm::provider::Message;

    #[test]
    fn borrowed_archive_conversion_preserves_sparse_metadata_boundaries() {
        let mut archived = SessionMessage::from(&Message::assistant("body".to_string()));
        archived.reasoning = Some(Box::new(String::new()));
        archived.reasoning_details = Some(Box::new(Vec::new()));
        archived.tool_calls = Some(Box::new(Vec::new()));
        archived.tool_call_id = Some(Box::new(String::new()));
        archived.origin_tool = Some(Box::new(String::new()));
        let empty = Message::from(&archived);
        assert!(empty.reasoning.is_none());
        assert!(empty.reasoning_details.is_none());
        assert!(empty.tool_calls.is_none());
        assert!(empty.tool_call_id.is_none());
        assert!(empty.origin_tool.is_none());

        archived.reasoning = Some(Box::new(" \t".to_string()));
        archived.reasoning_details = Some(Box::new(vec![
            serde_json::json!({"text": "second"}),
            serde_json::json!({"text": "first"}),
        ]));
        archived.tool_call_id = Some(Box::new("call-7".to_string()));
        archived.origin_tool = Some(Box::new("read_file".to_string()));
        let restored = Message::from(&archived);
        assert_eq!(restored.reasoning.as_deref(), Some(" \t"));
        assert_eq!(
            restored.reasoning_details,
            Some(vec![
                serde_json::json!({"text": "second"}),
                serde_json::json!({"text": "first"})
            ])
        );
        assert_eq!(restored.tool_call_id.as_deref(), Some("call-7"));
        assert_eq!(restored.origin_tool.as_deref(), Some("read_file"));
        assert!(restored.tool_calls.is_none());
    }
}
