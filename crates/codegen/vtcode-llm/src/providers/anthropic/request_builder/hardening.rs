//! Wire-hardening passes for the Anthropic `/messages` request body.
//!
//! Ported from XLI's `codex-rs/provider-anthropic/src/wire.rs` (S-005, S-014,
//! S-021, S-022). These fix requests that are well-formed per Anthropic's
//! direct API but get rejected on Vertex AI-backed routes (litellm proxies,
//! including NetApp's), or that can arise from interrupted tool loops,
//! mid-turn compaction, or truncated/edited history regardless of route.
//!
//! All four passes are unconditionally safe: they only ever repair requests
//! that would otherwise 400, never change behavior for an already-valid
//! request. They run last, directly on the assembled `AnthropicMessage`
//! list, after cache_control anchors have already been placed.

use hashbrown::HashSet;

use crate::providers::anthropic_types::{AnthropicContentBlock, AnthropicMessage};

/// Strips `tool_use`/`tool_result` blocks whose id has no counterpart
/// anywhere else in the message list. Unpaired blocks trigger a 400 from
/// the Anthropic API. Mirrors XLI's `clean_orphaned_tool_calls` (S-005),
/// adapted to run post-build on `AnthropicMessage` instead of pre-build on
/// the internal `ResponseItem` representation.
pub(crate) fn strip_globally_orphaned_tool_blocks(messages: &mut Vec<AnthropicMessage>) {
    let mut tool_use_ids: HashSet<String> = HashSet::new();
    let mut tool_result_ids: HashSet<String> = HashSet::new();

    for msg in messages.iter() {
        for block in &msg.content {
            match block {
                AnthropicContentBlock::ToolUse(b) => {
                    tool_use_ids.insert(b.id.clone());
                }
                AnthropicContentBlock::ToolResult(b) => {
                    tool_result_ids.insert(b.tool_use_id.clone());
                }
                _ => {}
            }
        }
    }

    let paired: HashSet<&String> = tool_use_ids.intersection(&tool_result_ids).collect();

    for msg in messages.iter_mut() {
        msg.content.retain(|block| match block {
            AnthropicContentBlock::ToolUse(b) => paired.contains(&b.id),
            AnthropicContentBlock::ToolResult(b) => paired.contains(&b.tool_use_id),
            _ => true,
        });
    }

    messages.retain(|msg| !msg.content.is_empty());
}

/// Enforces Anthropic's stricter adjacency rule: every `tool_use` block in
/// an assistant message must have its matching `tool_result` in the
/// *immediately following* message, and vice versa. Global pairing
/// (`strip_globally_orphaned_tool_blocks`) is necessary but not sufficient —
/// a pair can exist but be separated by an intervening message (e.g. from
/// history compaction or interleaved reasoning items). Mirrors XLI's
/// `clean_orphaned_tool_blocks_by_adjacency` (S-021).
pub(crate) fn enforce_tool_use_result_adjacency(messages: &mut Vec<AnthropicMessage>) {
    let len = messages.len();

    for i in 0..len {
        if messages[i].role != "assistant" {
            continue;
        }
        let tool_use_ids: HashSet<String> = messages[i]
            .content
            .iter()
            .filter_map(|b| match b {
                AnthropicContentBlock::ToolUse(u) => Some(u.id.clone()),
                _ => None,
            })
            .collect();
        if tool_use_ids.is_empty() {
            continue;
        }

        let next_result_ids: HashSet<String> = if i + 1 < len && messages[i + 1].role == "user" {
            messages[i + 1]
                .content
                .iter()
                .filter_map(|b| match b {
                    AnthropicContentBlock::ToolResult(r) => Some(r.tool_use_id.clone()),
                    _ => None,
                })
                .collect()
        } else {
            HashSet::new()
        };

        let matched: HashSet<&String> = tool_use_ids.intersection(&next_result_ids).collect();
        if matched.len() < tool_use_ids.len() {
            messages[i].content.retain(|b| match b {
                AnthropicContentBlock::ToolUse(u) => matched.contains(&u.id),
                _ => true,
            });
        }
    }

    for i in 0..messages.len() {
        if messages[i].role != "user" {
            continue;
        }
        let tool_result_ids: HashSet<String> = messages[i]
            .content
            .iter()
            .filter_map(|b| match b {
                AnthropicContentBlock::ToolResult(r) => Some(r.tool_use_id.clone()),
                _ => None,
            })
            .collect();
        if tool_result_ids.is_empty() {
            continue;
        }

        let prev_use_ids: HashSet<String> = if i > 0 && messages[i - 1].role == "assistant" {
            messages[i - 1]
                .content
                .iter()
                .filter_map(|b| match b {
                    AnthropicContentBlock::ToolUse(u) => Some(u.id.clone()),
                    _ => None,
                })
                .collect()
        } else {
            HashSet::new()
        };

        let matched: HashSet<&String> = tool_result_ids.intersection(&prev_use_ids).collect();
        if matched.len() < tool_result_ids.len() {
            messages[i].content.retain(|b| match b {
                AnthropicContentBlock::ToolResult(r) => matched.contains(&r.tool_use_id),
                _ => true,
            });
        }
    }

    messages.retain(|msg| !msg.content.is_empty());
}

/// Stable-partitions each user message so all `tool_result` blocks appear
/// before any other block, preserving relative order within each group.
///
/// Direct Anthropic API tolerates `tool_result` anywhere in a user message.
/// Vertex AI's Claude endpoint enforces a stricter constraint: when an
/// assistant message ends with `tool_use`, the following user message must
/// *begin* with the matching `tool_result` block(s), or Vertex rejects it
/// with `tool_use ids were found without tool_result blocks immediately
/// after`. Mirrors XLI's `hoist_tool_results_to_front` (S-022).
pub(crate) fn hoist_tool_results_to_front(messages: &mut [AnthropicMessage]) {
    for msg in messages.iter_mut() {
        if msg.role != "user" {
            continue;
        }

        let any_tool_result = msg.content.iter().any(|b| matches!(b, AnthropicContentBlock::ToolResult(_)));
        if !any_tool_result {
            continue;
        }

        let total_results = msg
            .content
            .iter()
            .filter(|b| matches!(b, AnthropicContentBlock::ToolResult(_)))
            .count();
        let leading_results = msg
            .content
            .iter()
            .take_while(|b| matches!(b, AnthropicContentBlock::ToolResult(_)))
            .count();
        if leading_results == total_results {
            continue;
        }

        let owned = std::mem::take(&mut msg.content);
        let (results, rest): (Vec<_>, Vec<_>) = owned
            .into_iter()
            .partition(|b| matches!(b, AnthropicContentBlock::ToolResult(_)));
        msg.content = results;
        msg.content.extend(rest);
    }
}

/// Appends a synthetic user turn when the conversation ends on an assistant
/// message and the target model/route does not support assistant-message
/// prefill (Vertex AI rejects a trailing assistant message outright; direct
/// Anthropic tolerates it as intentional prefill). Without this, an
/// interrupted tool loop, mid-turn compaction, or a fork/resume snapshot
/// that ends on an assistant boundary 400s on Vertex-backed routes. Mirrors
/// XLI's trailing-assistant guard (S-014).
pub(crate) fn guard_trailing_assistant_message(messages: &mut Vec<AnthropicMessage>, supports_assistant_prefill: bool) {
    if supports_assistant_prefill {
        return;
    }
    let Some(last) = messages.last() else {
        return;
    };
    if last.role != "assistant" {
        return;
    }

    let has_tool_use = last.content.iter().any(|b| matches!(b, AnthropicContentBlock::ToolUse(_)));
    let sentinel = if has_tool_use {
        "[Awaiting tool result]"
    } else {
        "[Continue]"
    };

    messages.push(AnthropicMessage {
        role: "user".to_string(),
        content: vec![AnthropicContentBlock::Text {
            text: sentinel.to_string(),
            citations: None,
            cache_control: None,
        }],
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::anthropic_types::{AnthropicToolResultBlock, AnthropicToolUseBlock};
    use serde_json::json;

    fn text_msg(role: &str, text: &str) -> AnthropicMessage {
        AnthropicMessage {
            role: role.to_string(),
            content: vec![AnthropicContentBlock::Text {
                text: text.to_string(),
                citations: None,
                cache_control: None,
            }],
        }
    }

    fn tool_use_msg(id: &str) -> AnthropicMessage {
        AnthropicMessage {
            role: "assistant".to_string(),
            content: vec![AnthropicContentBlock::ToolUse(Box::new(AnthropicToolUseBlock {
                id: id.to_string(),
                name: "shell".to_string(),
                input: json!({}),
                cache_control: None,
            }))],
        }
    }

    fn tool_result_msg(id: &str) -> AnthropicMessage {
        AnthropicMessage {
            role: "user".to_string(),
            content: vec![AnthropicContentBlock::ToolResult(Box::new(AnthropicToolResultBlock {
                tool_use_id: id.to_string(),
                content: json!("ok"),
                is_error: None,
                cache_control: None,
            }))],
        }
    }

    #[test]
    fn strip_orphaned_drops_unmatched_tool_use_and_result() {
        let mut messages = vec![
            text_msg("user", "hi"),
            tool_use_msg("toolu_1"),
            tool_result_msg("toolu_2"), // mismatched id -> both sides orphaned
        ];
        strip_globally_orphaned_tool_blocks(&mut messages);
        // Both the tool_use and tool_result messages become empty and are removed.
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "user");
    }

    #[test]
    fn strip_orphaned_keeps_matched_pair() {
        let mut messages = vec![tool_use_msg("toolu_1"), tool_result_msg("toolu_1")];
        strip_globally_orphaned_tool_blocks(&mut messages);
        assert_eq!(messages.len(), 2);
    }

    #[test]
    fn adjacency_strips_pair_separated_by_intervening_message() {
        let mut messages = vec![
            tool_use_msg("toolu_1"),
            text_msg("assistant", "aside"),
            tool_result_msg("toolu_1"),
        ];
        enforce_tool_use_result_adjacency(&mut messages);
        // tool_use has no user message immediately after -> stripped; the
        // orphaned tool_result (now unmatched) is also stripped by its own check.
        assert!(messages.iter().all(|m| {
            !m.content
                .iter()
                .any(|b| matches!(b, AnthropicContentBlock::ToolUse(_) | AnthropicContentBlock::ToolResult(_)))
        }));
    }

    #[test]
    fn adjacency_keeps_immediately_adjacent_pair() {
        let mut messages = vec![tool_use_msg("toolu_1"), tool_result_msg("toolu_1")];
        enforce_tool_use_result_adjacency(&mut messages);
        assert_eq!(messages.len(), 2);
    }

    #[test]
    fn hoist_moves_tool_result_ahead_of_text_in_user_message() {
        let mut messages = vec![AnthropicMessage {
            role: "user".to_string(),
            content: vec![
                AnthropicContentBlock::Text {
                    text: "note".to_string(),
                    citations: None,
                    cache_control: None,
                },
                AnthropicContentBlock::ToolResult(Box::new(AnthropicToolResultBlock {
                    tool_use_id: "toolu_1".to_string(),
                    content: json!("ok"),
                    is_error: None,
                    cache_control: None,
                })),
            ],
        }];
        hoist_tool_results_to_front(&mut messages);
        assert!(matches!(messages[0].content[0], AnthropicContentBlock::ToolResult(_)));
        assert!(matches!(messages[0].content[1], AnthropicContentBlock::Text { .. }));
    }

    #[test]
    fn hoist_is_noop_when_already_leading() {
        let mut messages = vec![tool_result_msg("toolu_1")];
        let before = messages[0].content.len();
        hoist_tool_results_to_front(&mut messages);
        assert_eq!(messages[0].content.len(), before);
    }

    #[test]
    fn guard_appends_sentinel_when_prefill_unsupported_and_trailing_assistant() {
        let mut messages = vec![text_msg("user", "hi"), text_msg("assistant", "thinking...")];
        guard_trailing_assistant_message(&mut messages, false);
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[2].role, "user");
    }

    #[test]
    fn guard_uses_awaiting_tool_result_sentinel_when_tool_use_pending() {
        let mut messages = vec![tool_use_msg("toolu_1")];
        guard_trailing_assistant_message(&mut messages, false);
        assert_eq!(messages.len(), 2);
        match &messages[1].content[0] {
            AnthropicContentBlock::Text { text, .. } => assert_eq!(text, "[Awaiting tool result]"),
            other => panic!("expected text sentinel, got {other:?}"),
        }
    }

    #[test]
    fn guard_noop_when_prefill_supported() {
        let mut messages = vec![text_msg("assistant", "prefill")];
        guard_trailing_assistant_message(&mut messages, true);
        assert_eq!(messages.len(), 1);
    }

    #[test]
    fn guard_noop_when_last_message_is_user() {
        let mut messages = vec![text_msg("assistant", "hi"), text_msg("user", "hello")];
        guard_trailing_assistant_message(&mut messages, false);
        assert_eq!(messages.len(), 2);
    }
}
