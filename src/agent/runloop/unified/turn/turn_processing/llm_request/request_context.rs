//! Append-only request context persisted in canonical history.
//!
//! Context derived at request time (few-shot examples selected for the latest
//! user query) used to be spliced into every request at a moving position:
//! few-shot after the newest message. On routes that bind replayed thinking
//! to the exact prior prefix (Claude Sonnet 5.5, Claude Opus 5.5, Claude
//! Fable 5.1) and on every prompt cache, a block that moves, changes, or
//! disappears between requests invalidates everything after it.
//!
//! Instead, the block is written into canonical history at the first request
//! of a user turn, and later requests replay it unchanged at the same
//! position, so each request only appends to the previous one: few-shot
//! context goes directly after the user message, once per turn, as a typed
//! turn-scoped system message.
//!
//! Canonical history keeps it as a system-role message so user-turn logic
//! (session titles, rewind points, intent extraction) never mistakes it for
//! user input. [`translate_request_context_for_wire`] shapes it per route:
//! - on routes with turn-scoped system messages, few-shot context keeps
//!   `role: "system"` and `clear_at: "next_user_message"`, so the provider
//!   stops applying it once the next user turn arrives while the prefix stays
//!   byte-identical;
//! - other routes receive few-shot context as a user-role message. Their
//!   adapters fold mid-history system messages into the top-level system
//!   prompt, which would rewrite the cached system prefix whenever the
//!   selection changes.

use crate::agent::runloop::unified::turn::context::last_user_message_index;
use vtcode_core::llm::provider as uni;
use vtcode_core::prompts::FEW_SHOT_SECTION_HEADER;

fn is_few_shot_context_message(message: &uni::Message) -> bool {
    message.role == uni::MessageRole::System && message.content.as_text().starts_with(FEW_SHOT_SECTION_HEADER)
}

/// Index of the latest user message when no assistant or tool message follows
/// it, i.e. when the provider has not produced any output for this turn yet.
/// Context may only be placed around such a message without editing a prefix
/// an earlier request already sent.
fn unanswered_user_turn_index(history: &[uni::Message]) -> Option<usize> {
    let user_index = last_user_message_index(history)?;
    let answered = history[user_index + 1..]
        .iter()
        .any(|message| matches!(message.role, uni::MessageRole::Assistant | uni::MessageRole::Tool));
    (!answered).then_some(user_index)
}

/// Persist the few-shot block selected for the current user turn.
///
/// Runs on every request but only writes while the turn is unanswered: the
/// block is inserted right after the user message, or refreshed in place when
/// a retry of the same unanswered turn selects different examples. Once the
/// provider has answered, the persisted block is left untouched so every
/// later request replays the same prefix.
pub(super) fn persist_turn_few_shot_context(history: &mut Vec<uni::Message>, few_shot_context: Option<String>) {
    let Some(few_shot_context) = few_shot_context.filter(|text| !text.trim().is_empty()) else {
        return;
    };
    let Some(user_index) = unanswered_user_turn_index(history) else {
        return;
    };

    if let Some(existing) = history[user_index + 1..]
        .iter_mut()
        .find(|message| is_few_shot_context_message(message))
    {
        if existing.content.as_text().as_ref() != few_shot_context {
            *existing = uni::Message::turn_scoped_system(few_shot_context);
        }
        return;
    }

    history.insert(user_index + 1, uni::Message::turn_scoped_system(few_shot_context));
}

pub(super) fn request_context_needs_wire_translation(
    messages: &[uni::Message],
    turn_scoped_system_messages: bool,
) -> bool {
    !turn_scoped_system_messages
        && messages
            .iter()
            .any(|message| message.clear_at.is_some() || is_few_shot_context_message(message))
}

/// Shape persisted request context for the active route. Canonical history is
/// never modified; callers pass a request-only copy.
pub(super) fn translate_request_context_for_wire(messages: &mut [uni::Message], turn_scoped_system_messages: bool) {
    for message in messages {
        if turn_scoped_system_messages {
            continue;
        }
        if is_few_shot_context_message(message) {
            // Keep the block where it was persisted; a user-role message is
            // never folded into the top-level system prompt.
            message.role = uni::MessageRole::User;
        }
        // Translate the provider-specific lifecycle field of typed turn-scoped
        // markers into an ordinary directive for routes without native
        // support.
        message.clear_at = None;
    }
}

#[cfg(test)]
mod tests {
    use vtcode_core::llm::provider as uni;

    use super::*;

    fn few_shot(text: &str) -> String {
        format!("{FEW_SHOT_SECTION_HEADER}\n{text}")
    }

    #[test]
    fn few_shot_is_inserted_after_the_unanswered_user_message() {
        let mut history = vec![
            uni::Message::user("first".to_string()),
            uni::Message::assistant("done".to_string()),
            uni::Message::user("second".to_string()),
        ];

        persist_turn_few_shot_context(&mut history, Some(few_shot("example")));

        assert_eq!(history.len(), 4);
        assert_eq!(history[2], uni::Message::user("second".to_string()));
        assert_eq!(history[3], uni::Message::turn_scoped_system(few_shot("example")));
    }

    #[test]
    fn few_shot_is_not_moved_or_duplicated_once_the_turn_is_answered() {
        let mut history = vec![uni::Message::user("task".to_string())];
        persist_turn_few_shot_context(&mut history, Some(few_shot("example")));
        history.push(uni::Message::assistant_with_tools(
            String::new(),
            vec![uni::ToolCall::function(
                "call_1".to_string(),
                "read_file".to_string(),
                "{}".to_string(),
            )],
        ));
        history.push(uni::Message::tool_response("call_1".to_string(), "contents".to_string()));
        let answered = history.clone();

        persist_turn_few_shot_context(&mut history, Some(few_shot("example")));
        persist_turn_few_shot_context(&mut history, Some(few_shot("different")));

        assert_eq!(history, answered, "an answered turn's prefix must stay byte-identical");
    }

    #[test]
    fn retry_of_unanswered_turn_refreshes_few_shot_in_place() {
        let mut history = vec![uni::Message::user("task".to_string())];
        persist_turn_few_shot_context(&mut history, Some(few_shot("old")));
        persist_turn_few_shot_context(&mut history, Some(few_shot("old")));
        assert_eq!(history.len(), 2);

        persist_turn_few_shot_context(&mut history, Some(few_shot("new")));
        assert_eq!(history.len(), 2);
        assert_eq!(history[1], uni::Message::turn_scoped_system(few_shot("new")));
    }

    #[test]
    fn earlier_turn_few_shot_is_kept_when_a_new_turn_starts() {
        let mut history = vec![uni::Message::user("first".to_string())];
        persist_turn_few_shot_context(&mut history, Some(few_shot("one")));
        history.push(uni::Message::assistant("done".to_string()));
        history.push(uni::Message::user("second".to_string()));
        persist_turn_few_shot_context(&mut history, Some(few_shot("two")));

        assert_eq!(
            history,
            vec![
                uni::Message::user("first".to_string()),
                uni::Message::turn_scoped_system(few_shot("one")),
                uni::Message::assistant("done".to_string()),
                uni::Message::user("second".to_string()),
                uni::Message::turn_scoped_system(few_shot("two")),
            ]
        );
    }

    #[test]
    fn wire_translation_keeps_turn_scoped_system_on_supported_routes() {
        let mut messages = vec![
            uni::Message::user("task".to_string()),
            uni::Message::turn_scoped_system(few_shot("example")),
        ];
        let original = messages.clone();

        assert!(!request_context_needs_wire_translation(&messages, true));
        translate_request_context_for_wire(&mut messages, true);

        assert_eq!(messages, original);
    }

    #[test]
    fn wire_translation_sends_few_shot_as_user_context_elsewhere() {
        let notice = uni::Message::turn_scoped_system("notice".to_string());
        let mut messages = vec![
            uni::Message::user("task".to_string()),
            uni::Message::turn_scoped_system(few_shot("example")),
            notice,
        ];

        assert!(request_context_needs_wire_translation(&messages, false));
        translate_request_context_for_wire(&mut messages, false);

        assert_eq!(messages[1], uni::Message::user(few_shot("example")));
        assert_eq!(messages[2], uni::Message::system("notice".to_string()));
    }
}
