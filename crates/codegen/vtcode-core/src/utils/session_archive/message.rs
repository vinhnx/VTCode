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

#[expect(
    clippy::box_collection,
    reason = "Intentional compatibility, platform, test, or API-shape suppression."
)]
fn clone_non_empty_boxed_string(value: &Option<Box<String>>) -> Option<String> {
    value
        .as_deref()
        .and_then(|value| (!value.is_empty()).then_some(value.to_owned()))
}

fn deserialize_boxed_metadata_opt<'de, D>(
    deserializer: D,
) -> Result<Option<Box<crate::core::message_metadata::MessageMetadata>>, D::Error>
where
    D: Deserializer<'de>,
{
    crate::core::message_metadata::MessageMetadata::deserialize(deserializer).map(|m| Some(Box::new(m)))
}

#[expect(
    clippy::box_collection,
    reason = "Intentional compatibility, platform, test, or API-shape suppression."
)]
fn clone_non_empty_boxed_vec<T: Clone>(value: &Option<Box<Vec<T>>>) -> Option<Vec<T>> {
    value.as_deref().and_then(|value| (!value.is_empty()).then_some(value.clone()))
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
            reasoning: clone_non_empty_boxed_string(&message.reasoning),
            reasoning_details: clone_non_empty_boxed_vec(&message.reasoning_details),
            tool_calls: clone_non_empty_boxed_vec(&message.tool_calls),
            tool_call_id: clone_non_empty_boxed_string(&message.tool_call_id),
            phase: message.phase,
            origin_tool: clone_non_empty_boxed_string(&message.origin_tool),
            metadata: message.metadata.as_deref().cloned(),
            clear_at: message.clear_at,
        }
    }
}
