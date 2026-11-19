//! Raw assistant content-block order, recorded on receipt and honored on replay.
//!
//! The universal [`Message`] keeps an assistant turn as separate channels:
//! text in `content`, tool calls in `tool_calls`, and thinking, advisor, and
//! compaction blocks in `reasoning_details`. Replaying from those channels
//! yields the fixed order compaction, thinking, text, advisor, tool use. That
//! order is wrong whenever the model interleaves blocks, for example
//! `thinking, text, thinking, tool_use` when Claude Opus 5.5 writes a progress
//! update between tool calls. Models with preserved thinking (Claude Opus 5.5,
//! Claude Fable 5.1) bind each thinking signature to the exact prefix, so a
//! reordered replay is rejected or silently drops the reasoning.
//!
//! When a response's order differs from that fixed order, or it carries more
//! than one text block, the parser and stream decoder append one compact
//! `anthropic_block_order` entry to `reasoning_details`. It stores only the
//! block kinds, each text block's byte length within `content`, and tool-use
//! ids, never duplicated payloads. Replay rebuilds the blocks in that order and
//! falls back to the fixed order when the recorded shape no longer matches the
//! message (for example when the runtime edited the text or dropped a tool
//! call), so a stale record can never produce a malformed request.

use hashbrown::HashMap;
use serde_json::{Value, json};
use std::collections::BTreeMap;

use crate::provider::{Message, MessageContent};
use crate::providers::anthropic_types::{AnthropicContentBlock, AnthropicStreamDelta, AnthropicStreamEvent};
use crate::providers::common::normalize_reasoning_detail_object;

pub(crate) const BLOCK_ORDER_DETAIL_TYPE: &str = "anthropic_block_order";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BlockSlot {
    Compaction,
    /// A `thinking` or `redacted_thinking` block that replay re-emits.
    Reasoning,
    /// A text block; `len` is its byte length within the joined `content`.
    Text {
        len: usize,
    },
    /// An advisor `server_tool_use` or `advisor_tool_result` block.
    Advisor,
    ToolUse {
        id: String,
    },
}

impl BlockSlot {
    /// Position of the kind in the fixed reconstruction order.
    fn rank(&self) -> u8 {
        match self {
            Self::Compaction => 0,
            Self::Reasoning => 1,
            Self::Text { .. } => 2,
            Self::Advisor => 3,
            Self::ToolUse { .. } => 4,
        }
    }

    fn to_value(&self) -> Value {
        match self {
            Self::Compaction => json!({ "kind": "compaction" }),
            Self::Reasoning => json!({ "kind": "thinking" }),
            Self::Text { len } => json!({ "kind": "text", "len": len }),
            Self::Advisor => json!({ "kind": "advisor" }),
            Self::ToolUse { id } => json!({ "kind": "tool_use", "id": id }),
        }
    }

    fn from_value(value: &Value) -> Option<Self> {
        match value.get("kind")?.as_str()? {
            "compaction" => Some(Self::Compaction),
            "thinking" => Some(Self::Reasoning),
            "text" => Some(Self::Text {
                len: usize::try_from(value.get("len")?.as_u64()?).ok()?,
            }),
            "advisor" => Some(Self::Advisor),
            "tool_use" => Some(Self::ToolUse { id: value.get("id")?.as_str()?.to_owned() }),
            _ => None,
        }
    }
}

/// Collects the block order of one non-streaming response.
#[derive(Debug, Default)]
pub(crate) struct BlockOrderRecorder {
    slots: Vec<BlockSlot>,
}

impl BlockOrderRecorder {
    pub(crate) fn push(&mut self, slot: BlockSlot) {
        if matches!(slot, BlockSlot::Text { len: 0 }) {
            return;
        }
        self.slots.push(slot);
    }

    /// The serialized `reasoning_details` entry, or `None` when the fixed
    /// reconstruction order already reproduces the response exactly.
    pub(crate) fn into_detail(self) -> Option<String> {
        let text_blocks = self.slots.iter().filter(|slot| matches!(slot, BlockSlot::Text { .. })).count();
        let in_fixed_order = self.slots.windows(2).all(|pair| pair[0].rank() <= pair[1].rank());
        if text_blocks <= 1 && in_fixed_order {
            return None;
        }

        let blocks: Vec<Value> = self.slots.iter().map(BlockSlot::to_value).collect();
        Some(json!({ "type": BLOCK_ORDER_DETAIL_TYPE, "blocks": blocks }).to_string())
    }
}

enum StreamSlot {
    Recorded(BlockSlot),
    /// A thinking block with no text or signature so far. Replay skips such
    /// blocks, so it only counts once it receives content.
    PendingThinking,
}

/// Collects the block order of a streamed response, keyed by block index.
#[derive(Default)]
pub(crate) struct StreamBlockOrder {
    slots: BTreeMap<usize, StreamSlot>,
}

impl StreamBlockOrder {
    pub(crate) fn observe(&mut self, event: &AnthropicStreamEvent) {
        match event {
            AnthropicStreamEvent::ContentBlockStart { index, content_block } => {
                let slot = match content_block {
                    // The decoder only accumulates text from deltas.
                    AnthropicContentBlock::Text { .. } => StreamSlot::Recorded(BlockSlot::Text { len: 0 }),
                    AnthropicContentBlock::Thinking { thinking, signature, .. } => {
                        if thinking.is_empty() && signature.as_deref().is_none_or(|value| value.trim().is_empty()) {
                            StreamSlot::PendingThinking
                        } else {
                            StreamSlot::Recorded(BlockSlot::Reasoning)
                        }
                    }
                    AnthropicContentBlock::RedactedThinking { .. } => StreamSlot::Recorded(BlockSlot::Reasoning),
                    AnthropicContentBlock::Compaction { .. } => StreamSlot::Recorded(BlockSlot::Compaction),
                    AnthropicContentBlock::ToolUse(tool_use) => {
                        StreamSlot::Recorded(BlockSlot::ToolUse { id: tool_use.id.clone() })
                    }
                    AnthropicContentBlock::ServerToolUse { name, .. } if name == "advisor" => {
                        StreamSlot::Recorded(BlockSlot::Advisor)
                    }
                    AnthropicContentBlock::AdvisorToolResult { .. } => StreamSlot::Recorded(BlockSlot::Advisor),
                    _ => return,
                };
                self.slots.insert(*index, slot);
            }
            AnthropicStreamEvent::ContentBlockDelta { index, delta } => match delta {
                AnthropicStreamDelta::TextDelta { text } => {
                    if let StreamSlot::Recorded(BlockSlot::Text { len }) = self
                        .slots
                        .entry(*index)
                        .or_insert(StreamSlot::Recorded(BlockSlot::Text { len: 0 }))
                    {
                        *len += text.len();
                    }
                }
                AnthropicStreamDelta::ThinkingDelta { thinking: fragment } => {
                    self.note_thinking_content(*index, !fragment.is_empty());
                }
                AnthropicStreamDelta::SignatureDelta { signature } => {
                    self.note_thinking_content(*index, !signature.trim().is_empty());
                }
                AnthropicStreamDelta::CompactionDelta { .. } => {
                    self.slots.entry(*index).or_insert(StreamSlot::Recorded(BlockSlot::Compaction));
                }
                AnthropicStreamDelta::InputJsonDelta { .. } | AnthropicStreamDelta::Unknown => {}
            },
            _ => {}
        }
    }

    fn note_thinking_content(&mut self, index: usize, has_content: bool) {
        let slot = self.slots.entry(index).or_insert(StreamSlot::PendingThinking);
        if has_content && matches!(slot, StreamSlot::PendingThinking) {
            *slot = StreamSlot::Recorded(BlockSlot::Reasoning);
        }
    }

    /// Drops the slots a mid-output fallback at `boundary` discards: thinking
    /// and tool use before it, plus the given unpaired advisor blocks.
    pub(crate) fn discard_declined_partial(&mut self, boundary: usize, unpaired_advisor_indices: &[usize]) {
        self.slots.retain(|index, slot| {
            *index >= boundary
                || !(matches!(
                    slot,
                    StreamSlot::PendingThinking
                        | StreamSlot::Recorded(BlockSlot::Reasoning | BlockSlot::ToolUse { .. })
                ) || unpaired_advisor_indices.contains(index))
        });
    }

    pub(crate) fn into_detail(self) -> Option<String> {
        let mut recorder = BlockOrderRecorder::default();
        for slot in self.slots.into_values() {
            if let StreamSlot::Recorded(slot) = slot {
                recorder.push(slot);
            }
        }
        recorder.into_detail()
    }
}

fn recorded_block_order(message: &Message) -> Option<Vec<BlockSlot>> {
    let details = message.reasoning_details.as_ref()?;
    let record = details
        .iter()
        .filter_map(normalize_reasoning_detail_object)
        .rfind(|detail| detail.get("type").and_then(Value::as_str) == Some(BLOCK_ORDER_DETAIL_TYPE))?;
    record.get("blocks")?.as_array()?.iter().map(BlockSlot::from_value).collect()
}

/// Assistant blocks rebuilt from the separate [`Message`] channels.
pub(crate) struct AssistantBlockParts {
    pub(crate) compaction: Vec<AnthropicContentBlock>,
    pub(crate) reasoning: Vec<AnthropicContentBlock>,
    pub(crate) content: Vec<AnthropicContentBlock>,
    pub(crate) advisor: Vec<AnthropicContentBlock>,
    pub(crate) tool_use: Vec<AnthropicContentBlock>,
}

impl AssistantBlockParts {
    fn into_fixed_order(self) -> Vec<AnthropicContentBlock> {
        let mut blocks = self.compaction;
        blocks.extend(self.reasoning);
        blocks.extend(self.content);
        blocks.extend(self.advisor);
        blocks.extend(self.tool_use);
        blocks
    }
}

/// Assistant content blocks for replay: the recorded order when the message
/// still matches its record, otherwise the fixed reconstruction order.
pub(crate) fn assemble_assistant_blocks(message: &Message, parts: AssistantBlockParts) -> Vec<AnthropicContentBlock> {
    let Some(slots) = recorded_block_order(message) else {
        return parts.into_fixed_order();
    };
    if !record_matches_message(&slots, message, &parts) {
        tracing::debug!(
            "assistant block-order record no longer matches the message; replaying in the default block order"
        );
        return parts.into_fixed_order();
    }

    let text = message.content.as_text();
    let mut text_offset = 0;
    let mut compaction = parts.compaction.into_iter();
    let mut reasoning = parts.reasoning.into_iter();
    let mut advisor = parts.advisor.into_iter();
    let mut tool_use: HashMap<String, AnthropicContentBlock> = parts
        .tool_use
        .into_iter()
        .filter_map(|block| {
            let id = tool_use_id(&block)?.to_owned();
            Some((id, block))
        })
        .collect();

    let mut blocks = Vec::with_capacity(slots.len());
    for slot in slots {
        let block = match slot {
            BlockSlot::Compaction => compaction.next(),
            BlockSlot::Reasoning => reasoning.next(),
            BlockSlot::Advisor => advisor.next(),
            BlockSlot::ToolUse { id } => tool_use.remove(&id),
            BlockSlot::Text { len } => {
                let segment = &text[text_offset..text_offset + len];
                text_offset += len;
                Some(AnthropicContentBlock::Text {
                    text: segment.to_owned(),
                    citations: None,
                    cache_control: None,
                })
            }
        };
        // `record_matches_message` guarantees every slot has its block.
        blocks.extend(block);
    }
    blocks
}

fn tool_use_id(block: &AnthropicContentBlock) -> Option<&str> {
    match block {
        AnthropicContentBlock::ToolUse(tool_use) => Some(tool_use.id.as_str()),
        _ => None,
    }
}

fn record_matches_message(slots: &[BlockSlot], message: &Message, parts: &AssistantBlockParts) -> bool {
    let count = |predicate: fn(&BlockSlot) -> bool| slots.iter().filter(|slot| predicate(slot)).count();
    if count(|slot| matches!(slot, BlockSlot::Compaction)) != parts.compaction.len()
        || count(|slot| matches!(slot, BlockSlot::Reasoning)) != parts.reasoning.len()
        || count(|slot| matches!(slot, BlockSlot::Advisor)) != parts.advisor.len()
    {
        return false;
    }

    // Tool uses must match one-to-one by id.
    let recorded_ids: Vec<&str> = slots
        .iter()
        .filter_map(|slot| match slot {
            BlockSlot::ToolUse { id } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    if recorded_ids.len() != parts.tool_use.len() {
        return false;
    }
    let mut available: HashMap<&str, usize> = HashMap::new();
    for block in &parts.tool_use {
        let Some(id) = tool_use_id(block) else {
            return false;
        };
        *available.entry(id).or_default() += 1;
    }
    for id in recorded_ids {
        match available.get_mut(id) {
            Some(remaining) if *remaining > 0 => *remaining -= 1,
            _ => return false,
        }
    }

    // Text segments must tile the stored content exactly, on char boundaries.
    let text = match &message.content {
        MessageContent::Text(text) => text.as_str(),
        MessageContent::Parts(parts) if parts.is_empty() => "",
        MessageContent::Parts(_) => return false,
    };
    let mut offset = 0usize;
    for slot in slots {
        if let BlockSlot::Text { len } = slot {
            let Some(end) = offset.checked_add(*len) else {
                return false;
            };
            if *len == 0 || end > text.len() || !text.is_char_boundary(end) {
                return false;
            }
            offset = end;
        }
    }
    offset == text.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::anthropic_types::AnthropicToolUseBlock;

    fn thinking(signature: &str) -> AnthropicContentBlock {
        AnthropicContentBlock::Thinking {
            thinking: String::new(),
            signature: Some(signature.to_owned()),
            cache_control: None,
        }
    }

    fn tool_use(id: &str) -> AnthropicContentBlock {
        AnthropicContentBlock::ToolUse(Box::new(AnthropicToolUseBlock::function(
            id.to_owned(),
            "read_file".to_owned(),
            json!({}),
        )))
    }

    fn kinds(blocks: &[AnthropicContentBlock]) -> Vec<String> {
        blocks
            .iter()
            .map(|block| match block {
                AnthropicContentBlock::Thinking { signature, .. } => {
                    format!("thinking:{}", signature.as_deref().unwrap_or(""))
                }
                AnthropicContentBlock::Text { text, .. } => format!("text:{text}"),
                AnthropicContentBlock::ToolUse(tool_use) => format!("tool_use:{}", tool_use.id),
                other => format!("{other:?}"),
            })
            .collect()
    }

    fn record(slots: Vec<BlockSlot>) -> Option<String> {
        let mut recorder = BlockOrderRecorder::default();
        for slot in slots {
            recorder.push(slot);
        }
        recorder.into_detail()
    }

    #[test]
    fn fixed_order_responses_carry_no_record() {
        assert!(
            record(vec![
                BlockSlot::Reasoning,
                BlockSlot::Reasoning,
                BlockSlot::Text { len: 3 },
                BlockSlot::ToolUse { id: "a".to_owned() },
            ])
            .is_none()
        );
    }

    #[test]
    fn record_is_invisible_to_reasoning_text_extraction() {
        let detail = record(vec![BlockSlot::Text { len: 1 }, BlockSlot::Reasoning]).expect("record");
        assert!(crate::providers::common::extract_reasoning_text_from_serialized_details(&[detail]).is_none());
    }

    #[test]
    fn interleaved_blocks_replay_in_recorded_order() {
        let detail = record(vec![
            BlockSlot::Reasoning,
            BlockSlot::Text { len: "Checking.".len() },
            BlockSlot::Reasoning,
            BlockSlot::ToolUse { id: "toolu_1".to_owned() },
            BlockSlot::Text { len: " Done.".len() },
        ])
        .expect("interleaved order needs a record");
        let message =
            Message::assistant("Checking. Done.".to_owned()).with_reasoning_details(Some(vec![json!(detail)]));

        let blocks = assemble_assistant_blocks(
            &message,
            AssistantBlockParts {
                compaction: Vec::new(),
                reasoning: vec![thinking("s1"), thinking("s2")],
                content: vec![AnthropicContentBlock::Text {
                    text: "Checking. Done.".to_owned(),
                    citations: None,
                    cache_control: None,
                }],
                advisor: Vec::new(),
                tool_use: vec![tool_use("toolu_1")],
            },
        );

        assert_eq!(
            kinds(&blocks),
            [
                "thinking:s1",
                "text:Checking.",
                "thinking:s2",
                "tool_use:toolu_1",
                "text: Done."
            ]
        );
    }

    #[test]
    fn stale_record_falls_back_to_fixed_order() {
        let detail = record(vec![
            BlockSlot::Reasoning,
            BlockSlot::Text { len: 4 },
            BlockSlot::Reasoning,
            BlockSlot::ToolUse { id: "toolu_1".to_owned() },
        ])
        .expect("record");
        // The runtime cleared the text after the response was recorded.
        let message = Message::assistant(String::new()).with_reasoning_details(Some(vec![json!(detail)]));

        let blocks = assemble_assistant_blocks(
            &message,
            AssistantBlockParts {
                compaction: Vec::new(),
                reasoning: vec![thinking("s1"), thinking("s2")],
                content: Vec::new(),
                advisor: Vec::new(),
                tool_use: vec![tool_use("toolu_1")],
            },
        );

        assert_eq!(kinds(&blocks), ["thinking:s1", "thinking:s2", "tool_use:toolu_1"]);
    }

    #[test]
    fn record_with_unknown_tool_id_falls_back() {
        let detail = record(vec![
            BlockSlot::Text { len: 2 },
            BlockSlot::Reasoning,
            BlockSlot::ToolUse { id: "gone".to_owned() },
        ])
        .expect("record");
        let message = Message::assistant("hi".to_owned()).with_reasoning_details(Some(vec![json!(detail)]));

        let blocks = assemble_assistant_blocks(
            &message,
            AssistantBlockParts {
                compaction: Vec::new(),
                reasoning: vec![thinking("s1")],
                content: vec![AnthropicContentBlock::Text {
                    text: "hi".to_owned(),
                    citations: None,
                    cache_control: None,
                }],
                advisor: Vec::new(),
                tool_use: vec![tool_use("toolu_2")],
            },
        );

        assert_eq!(kinds(&blocks), ["thinking:s1", "text:hi", "tool_use:toolu_2"]);
    }

    #[test]
    fn text_segments_must_end_on_char_boundaries() {
        let detail = record(vec![
            BlockSlot::Text { len: 1 },
            BlockSlot::Reasoning,
            BlockSlot::Text { len: 1 },
        ])
        .expect("record");
        let message = Message::assistant("é".to_owned()).with_reasoning_details(Some(vec![json!(detail)]));
        let blocks = assemble_assistant_blocks(
            &message,
            AssistantBlockParts {
                compaction: Vec::new(),
                reasoning: vec![thinking("s1")],
                content: vec![AnthropicContentBlock::Text {
                    text: "é".to_owned(),
                    citations: None,
                    cache_control: None,
                }],
                advisor: Vec::new(),
                tool_use: Vec::new(),
            },
        );
        assert_eq!(kinds(&blocks), ["thinking:s1", "text:é"]);
    }
}
