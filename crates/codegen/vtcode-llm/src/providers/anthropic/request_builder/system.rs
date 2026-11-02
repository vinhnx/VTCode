use crate::provider::{LLMRequest, Message, MessageRole};
use crate::providers::anthropic_types::CacheControl;
use crate::providers::shared::split_dynamic_prompt_suffix;
use serde_json::{Value, json};

pub(crate) struct SystemPromptBuildResult {
    pub system_value: Option<Value>,
    pub breakpoints_used: usize,
    pub has_uncached_runtime_context: bool,
}

/// Where history `role: system` messages are rendered on the Anthropic wire.
///
/// Every history system message is emitted in exactly one place: either
/// folded into the top-level `system` prompt or kept in `messages[]`, never
/// both. Duplicating it would bill it twice and, worse, rewrite the top-level
/// system prompt whenever a new directive is appended, which changes the
/// prefix ahead of every earlier message (cache miss, and invalidated thinking
/// blocks on preserved-thinking models).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HistorySystemPlacement {
    /// The route rejects mid-conversation system messages: every history
    /// system message (turn-scoped ones included) is folded into the system
    /// prompt and `build_messages` drops them from `messages[]`.
    FoldAll,
    /// The route accepts mid-conversation system messages: they stay in
    /// `messages[]`, appended after the history they follow, so the system
    /// prompt stays byte-identical as directives accumulate. Only the leading
    /// run before the first conversational message is folded, because a
    /// system message may not be `messages[0]`.
    FoldLeadingOnly,
}

impl HistorySystemPlacement {
    pub(crate) fn for_route(allow_mid_conversation_system: bool) -> Self {
        if allow_mid_conversation_system {
            Self::FoldLeadingOnly
        } else {
            Self::FoldAll
        }
    }

    /// Number of messages at the front of `messages` that are folded into
    /// the system prompt and must therefore be skipped when building
    /// `messages[]`. Only meaningful for [`Self::FoldLeadingOnly`]; under
    /// [`Self::FoldAll`] `build_messages` drops system messages itself.
    pub(crate) fn leading_folded_count(self, messages: &[Message]) -> usize {
        match self {
            Self::FoldAll => 0,
            Self::FoldLeadingOnly => leading_system_message_count(messages),
        }
    }
}

fn leading_system_message_count(messages: &[Message]) -> usize {
    messages
        .iter()
        .take_while(|message| message.role == MessageRole::System)
        .count()
}

// Stable/dynamic cut shared with the OpenAI wire and the core stable hash;
// the header list and match semantics live in `crate::providers::shared`.
const RUNTIME_CONTEXT_SECTION_HEADER: &str = "[Runtime Context]";
const HISTORY_DIRECTIVES_SECTION_HEADER: &str = "[History Directives]";
const RUNTIME_CONTEXT_NEWLINE: &str = concat!("[Runtime Context]", "\n");
const NEWLINE_RUNTIME_CONTEXT_NEWLINE: &str = concat!("\n", "[Runtime Context]", "\n");
const NEWLINE_HISTORY_DIRECTIVES_NEWLINE: &str = concat!("\n", "[History Directives]", "\n");
const HISTORY_DIRECTIVES_NEWLINE: &str = concat!("[History Directives]", "\n");

fn has_runtime_context_section(prompt: &str) -> bool {
    prompt.starts_with(RUNTIME_CONTEXT_NEWLINE)
        || prompt.contains(NEWLINE_RUNTIME_CONTEXT_NEWLINE)
        || prompt.starts_with(HISTORY_DIRECTIVES_NEWLINE)
        || prompt.contains(NEWLINE_HISTORY_DIRECTIVES_NEWLINE)
}

fn append_history_system_directives(
    final_system_prompt: &mut String,
    request: &LLMRequest,
    placement: HistorySystemPlacement,
) {
    let folded: &[Message] = match placement {
        HistorySystemPlacement::FoldAll => request.messages.as_slice(),
        HistorySystemPlacement::FoldLeadingOnly => &request.messages[..leading_system_message_count(&request.messages)],
    };
    let directives: Vec<String> = folded
        .iter()
        .filter(|message| message.role == MessageRole::System)
        .map(|message| message.content.as_text().trim().to_string())
        .filter(|text| !text.is_empty())
        .collect();

    if directives.is_empty() {
        return;
    }

    if !has_runtime_context_section(final_system_prompt) {
        if !final_system_prompt.is_empty() && !final_system_prompt.ends_with('\n') {
            final_system_prompt.push('\n');
        }
        final_system_prompt.push_str(RUNTIME_CONTEXT_SECTION_HEADER);
        final_system_prompt.push('\n');
    } else if !final_system_prompt.ends_with('\n') {
        final_system_prompt.push('\n');
    }

    final_system_prompt.push_str(HISTORY_DIRECTIVES_SECTION_HEADER);
    final_system_prompt.push('\n');
    for directive in directives {
        final_system_prompt.push_str("- ");
        final_system_prompt.push_str(&directive);
        final_system_prompt.push('\n');
    }
}

fn split_runtime_context_section(prompt: &str) -> Option<(String, String)> {
    // Cut at the earliest dynamic header so earlier runtime sections
    // (planning, harness limits, tool catalog, environment) stay out of the
    // cached prefix.
    let (stable_prefix, dynamic) = split_dynamic_prompt_suffix(prompt);
    let runtime_section = dynamic.filter(|section| !section.is_empty())?;
    if stable_prefix.is_empty() && runtime_section.is_empty() {
        return None;
    }
    Some((stable_prefix, runtime_section))
}

pub(crate) fn build_system_prompt(
    request: &LLMRequest,
    cache_control: Option<&CacheControl>,
    breakpoints_remaining: usize,
    history_system_placement: HistorySystemPlacement,
) -> SystemPromptBuildResult {
    let mut final_system_prompt = request
        .system_prompt
        .as_ref()
        .map(|s| s.as_ref())
        .unwrap_or_default()
        .to_string();

    append_history_system_directives(&mut final_system_prompt, request, history_system_placement);

    if final_system_prompt.is_empty() {
        return SystemPromptBuildResult {
            system_value: None,
            breakpoints_used: 0,
            has_uncached_runtime_context: false,
        };
    }

    if let Some((stable_prefix, runtime_section)) = split_runtime_context_section(&final_system_prompt) {
        let should_cache_stable_prefix =
            cache_control.is_some() && breakpoints_remaining > 0 && !stable_prefix.is_empty();
        let mut blocks = Vec::new();

        if !stable_prefix.is_empty() {
            if should_cache_stable_prefix {
                if let Some(cc) = cache_control {
                    blocks.push(json!({
                        "type": "text",
                        "text": stable_prefix,
                        "cache_control": cc
                    }));
                }
            } else {
                blocks.push(json!({
                    "type": "text",
                    "text": stable_prefix
                }));
            }
        }

        blocks.push(json!({
            "type": "text",
            "text": runtime_section
        }));

        return SystemPromptBuildResult {
            system_value: Some(Value::Array(blocks)),
            breakpoints_used: usize::from(should_cache_stable_prefix),
            has_uncached_runtime_context: true,
        };
    }

    let should_cache = cache_control.is_some() && breakpoints_remaining > 0;

    if should_cache && let Some(cc) = cache_control {
        let block = json!({
            "type": "text",
            "text": final_system_prompt.trim(),
            "cache_control": cc
        });
        return SystemPromptBuildResult {
            system_value: Some(Value::Array(vec![block])),
            breakpoints_used: 1,
            has_uncached_runtime_context: false,
        };
    }

    SystemPromptBuildResult {
        system_value: Some(Value::String(final_system_prompt.trim().to_string())),
        breakpoints_used: 0,
        has_uncached_runtime_context: false,
    }
}
