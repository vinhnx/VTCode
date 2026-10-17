//! Copilot prompt updates, completion conversion, and stream cancellation ownership.

use anyhow::anyhow;
use async_stream::stream;
use vtcode_core::copilot::{CopilotRuntimeRequest, PromptSession, PromptSessionCancelHandle, PromptUpdate};
use vtcode_core::llm::provider::{self as uni, LLMResponse, LLMStreamEvent, LLMStreamEvent::Completed};

use super::map_runtime_error;

pub(super) fn prompt_session_to_stream(
    model: String,
    prompt_session: PromptSession,
) -> (uni::LLMStream, tokio::sync::mpsc::UnboundedReceiver<CopilotRuntimeRequest>) {
    struct PromptCancellationGuard {
        cancel_handle: Option<PromptSessionCancelHandle>,
    }

    impl PromptCancellationGuard {
        fn new(cancel_handle: PromptSessionCancelHandle) -> Self {
            Self { cancel_handle: Some(cancel_handle) }
        }

        fn disarm(&mut self) {
            self.cancel_handle = None;
        }
    }

    impl Drop for PromptCancellationGuard {
        fn drop(&mut self) {
            if let Some(cancel_handle) = self.cancel_handle.take() {
                cancel_handle.cancel();
            }
        }
    }

    let (mut updates, runtime_requests, completion, cancel_handle) = prompt_session.into_parts();

    let stream = stream! {
        let mut cancellation_guard = PromptCancellationGuard::new(cancel_handle);
        let completion = completion;
        tokio::pin!(completion);

        let mut content = String::new();
        let mut reasoning = String::new();
        // Once the updates channel closes (all tokens delivered), disable that arm so
        // the select no longer spins on None and immediately picks `completion`.
        let mut updates_done = false;

        loop {
            tokio::select! {
                update = updates.recv(), if !updates_done => {
                    match update {
                        Some(PromptUpdate::Text(delta)) => {
                            content.push_str(&delta);
                            yield Ok(LLMStreamEvent::Token { delta });
                        }
                        Some(PromptUpdate::Thought(delta)) => {
                            let delta = normalize_copilot_reasoning_delta(&reasoning, delta);
                            reasoning.push_str(&delta);
                            yield Ok(LLMStreamEvent::Reasoning { delta });
                        }
                        None => {
                            // All tokens delivered; completion will be ready on next tick.
                            updates_done = true;
                        }
                    }
                }
                result = &mut completion => {
                    let completion = match result {
                        Ok(completion) => completion,
                        Err(err) => {
                            yield Err(map_runtime_error(anyhow!("copilot acp prompt task join failed: {err}")));
                            break;
                        }
                    };
                    let completion = match completion {
                        Ok(completion) => completion,
                        Err(err) => {
                            yield Err(map_runtime_error(err));
                            break;
                        }
                    };
                    while let Ok(update) = updates.try_recv() {
                        match update {
                            PromptUpdate::Text(delta) => {
                                content.push_str(&delta);
                                yield Ok(LLMStreamEvent::Token { delta });
                            }
                            PromptUpdate::Thought(delta) => {
                                let delta = normalize_copilot_reasoning_delta(&reasoning, delta);
                                reasoning.push_str(&delta);
                                yield Ok(LLMStreamEvent::Reasoning { delta });
                            }
                        }
                    }

                    let mut response = LLMResponse::new(model, content);
                    response.finish_reason =
                        map_copilot_finish_reason(&completion.stop_reason);
                    if !reasoning.is_empty() {
                        response.reasoning = Some(reasoning);
                    }
                    cancellation_guard.disarm();
                    yield Ok(Completed {
                        response: Box::new(response),
                    });
                    break;
                }
            }
        }
    };

    (Box::pin(stream), runtime_requests)
}

fn normalize_copilot_reasoning_delta(existing: &str, delta: String) -> String {
    let delta = collapse_reasoning_single_newlines(delta);
    if existing.is_empty()
        || existing.chars().last().is_some_and(char::is_whitespace)
        || delta.chars().next().is_some_and(char::is_whitespace)
        || delta.chars().next().is_some_and(is_reasoning_closing_punctuation)
    {
        delta
    } else {
        format!(" {delta}")
    }
}

fn collapse_reasoning_single_newlines(delta: String) -> String {
    let chars: Vec<char> = delta.chars().collect();
    let mut normalized = String::with_capacity(delta.len());

    for (index, ch) in chars.iter().copied().enumerate() {
        if ch != '\n' {
            normalized.push(ch);
            continue;
        }

        let prev = index.checked_sub(1).and_then(|idx| chars.get(idx)).copied();
        let next = chars.get(index + 1).copied();

        if prev.is_some() && next.is_some() && prev != Some('\n') && next != Some('\n') {
            if next.is_some_and(char::is_whitespace) || prev.is_some_and(char::is_whitespace) {
                continue;
            }
            if next.is_some_and(is_reasoning_closing_punctuation) {
                continue;
            }
            normalized.push(' ');
            continue;
        }

        normalized.push('\n');
    }

    normalized
}

fn is_reasoning_closing_punctuation(ch: char) -> bool {
    matches!(ch, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}')
}

fn map_copilot_finish_reason(stop_reason: &str) -> vtcode_core::llm::provider::FinishReason {
    match stop_reason.trim() {
        "end_turn" => vtcode_core::llm::provider::FinishReason::Stop,
        "max_tokens" | "length" => vtcode_core::llm::provider::FinishReason::Length,
        "refusal" => vtcode_core::llm::provider::FinishReason::Refusal,
        "cancelled" => vtcode_core::llm::provider::FinishReason::Error("cancelled".to_string()),
        other => vtcode_core::llm::provider::FinishReason::Error(other.to_string()),
    }
}

#[cfg(test)]
mod tests;
