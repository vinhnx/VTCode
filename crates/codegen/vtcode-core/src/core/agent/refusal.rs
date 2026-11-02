//! Provider refusal handling shared by the interactive turn loop and the
//! headless runner.
//!
//! A refusal (`FinishReason::Refusal`) is terminal for the prompt that caused
//! it: resending the same prompt is refused again, and any partial output was
//! cut off by the provider. Both loops stop on a refusal and report the notice
//! built here, so the wording and the facts it states stay identical across
//! the TUI, `exec` JSON output, and subagent results.

use serde_json::Value;

use crate::llm::provider::{FinishReason, LLMResponse};

/// Leading text of every refusal notice. Gates that must never treat a
/// refusal as a recoverable block (auto-continue, recovery turns) match it via
/// [`is_refusal_notice`].
pub const REFUSAL_NOTICE_PREFIX: &str = "The model declined this request";

/// Upper bound, in characters, on explanation text copied from the response.
const MAX_EXPLANATION_CHARS: usize = 400;

const NEXT_STEP_SENTENCE: &str = "The request was not retried; rephrase it or switch models.";

/// Whether `response` ended with a provider refusal.
pub fn is_refusal(response: &LLMResponse) -> bool {
    matches!(response.finish_reason, FinishReason::Refusal)
}

/// Whether `text` is a notice produced by [`refusal_reason`].
pub fn is_refusal_notice(text: &str) -> bool {
    text.trim_start().starts_with(REFUSAL_NOTICE_PREFIX)
}

/// User-facing explanation for a provider refusal.
///
/// Anthropic reports the refusal category and explanation in a
/// `stop_details` reasoning detail, and reports server-side fallback activity
/// as a `fallback` reasoning detail, `fallback_message` usage iterations, or a
/// `recommended_model` when the fallback could not run. Other providers only
/// report the finish reason, sometimes with a refusal message in the content;
/// that content (trimmed and bounded) is the explanation when no
/// `stop_details` explanation exists.
pub fn refusal_reason(response: &LLMResponse) -> String {
    let details: Vec<Value> = response
        .reasoning_details
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter_map(|detail| serde_json::from_str::<Value>(detail).ok())
        .collect();
    let stop_details = details.iter().find(|detail| detail_type(detail) == Some("stop_details"));
    let stop_field = |name: &str| {
        stop_details
            .and_then(|detail| detail.get(name))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    };

    let mut reason = match stop_field("category") {
        Some(category) => format!("{REFUSAL_NOTICE_PREFIX} (category: {category})"),
        None => REFUSAL_NOTICE_PREFIX.to_string(),
    };

    let explanation = stop_field("explanation").map(str::to_string).or_else(|| {
        response
            .content
            .as_deref()
            .map(str::trim)
            .filter(|content| !content.is_empty())
            .map(bounded_explanation)
    });
    match explanation {
        Some(explanation) => {
            reason.push_str(": ");
            reason.push_str(&vtcode_commons::formatting::lowercase_leading_word(&explanation));
            if !ends_with_terminal_punctuation(&reason) {
                reason.push('.');
            }
        }
        None => reason.push('.'),
    }

    if let Some(sentence) = fallback_sentence(response, &details, stop_field("recommended_model")) {
        reason.push(' ');
        reason.push_str(&sentence);
    }
    reason.push(' ');
    reason.push_str(NEXT_STEP_SENTENCE);
    reason
}

fn detail_type(detail: &Value) -> Option<&str> {
    detail.get("type").and_then(Value::as_str)
}

/// States fallback activity only when the response shows fallbacks were
/// carried or attempted; a refusal without fallback signals says nothing.
fn fallback_sentence(response: &LLMResponse, details: &[Value], recommended_model: Option<&str>) -> Option<String> {
    let fallback_served = details.iter().any(|detail| detail_type(detail) == Some("fallback"))
        || response
            .usage
            .as_ref()
            .and_then(|usage| usage.iterations.as_deref())
            .is_some_and(|iterations| {
                iterations
                    .iter()
                    .any(|iteration| detail_type(iteration) == Some("fallback_message"))
            });
    if fallback_served {
        return Some("The configured fallback model also declined.".to_string());
    }
    recommended_model.map(|model| format!("A fallback to `{model}` was recommended but could not run."))
}

fn bounded_explanation(content: &str) -> String {
    // Collapse whitespace so a multi-paragraph refusal message reads as one
    // sentence inside the notice.
    let collapsed = vtcode_commons::formatting::collapse_whitespace(content);
    vtcode_commons::formatting::truncate_within(&collapsed, MAX_EXPLANATION_CHARS, "...")
}

fn ends_with_terminal_punctuation(text: &str) -> bool {
    text.trim_end().chars().last().is_some_and(|c| matches!(c, '.' | '!' | '?')) || text.trim_end().ends_with("...")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::provider::Usage;
    use serde_json::json;

    fn refusal(content: Option<&str>, details: Vec<Value>) -> LLMResponse {
        LLMResponse {
            content: content.map(str::to_string),
            finish_reason: FinishReason::Refusal,
            reasoning_details: (!details.is_empty())
                .then(|| details.into_iter().map(|detail| detail.to_string()).collect()),
            ..LLMResponse::default()
        }
    }

    #[test]
    fn reports_category_and_explanation_without_fallback_claim() {
        let response = refusal(
            None,
            vec![
                json!({"type": "stop_details", "category": "cyber", "explanation": "This looks like malware development."}),
            ],
        );
        let reason = refusal_reason(&response);
        assert_eq!(
            reason,
            "The model declined this request (category: cyber): this looks like malware development. \
             The request was not retried; rephrase it or switch models."
        );
        assert!(!reason.contains(".."));
        assert!(!reason.contains("fallback"));
        assert!(is_refusal_notice(&reason));
    }

    #[test]
    fn mentions_fallback_only_when_a_fallback_model_ran() {
        let mut response = refusal(None, vec![json!({"type": "stop_details", "category": "cyber"})]);
        response.usage = Some(Usage {
            iterations: Some(vec![
                json!({"type": "message", "input_tokens": 10, "output_tokens": 0}),
                json!({"type": "fallback_message", "input_tokens": 10, "output_tokens": 0}),
            ]),
            ..Usage::default()
        });
        assert_eq!(
            refusal_reason(&response),
            "The model declined this request (category: cyber). The configured fallback model also declined. \
             The request was not retried; rephrase it or switch models."
        );

        let served = refusal(
            None,
            vec![
                json!({"type": "fallback", "from": {"model": "a"}, "to": {"model": "b"}}),
                json!({"type": "stop_details", "category": "bio"}),
            ],
        );
        assert!(refusal_reason(&served).contains("The configured fallback model also declined."));
    }

    #[test]
    fn reports_recommended_model_that_could_not_run() {
        let response = refusal(
            None,
            vec![json!({"type": "stop_details", "category": "cyber", "recommended_model": "claude-opus-4-8"})],
        );
        assert_eq!(
            refusal_reason(&response),
            "The model declined this request (category: cyber). A fallback to `claude-opus-4-8` was recommended \
             but could not run. The request was not retried; rephrase it or switch models."
        );
    }

    #[test]
    fn uses_bounded_content_when_stop_details_are_absent() {
        let response = refusal(Some("  I can't help with that.\n\nPlease ask something else.  "), Vec::new());
        assert_eq!(
            refusal_reason(&response),
            "The model declined this request: I can't help with that. Please ask something else. \
             The request was not retried; rephrase it or switch models."
        );

        let long = "Sorry ".repeat(200);
        let bounded = refusal_reason(&refusal(Some(&long), Vec::new()));
        assert!(bounded.contains("sorry Sorry"));
        assert!(bounded.contains("..."));
        assert!(!bounded.contains("...."));
        assert!(bounded.chars().count() < MAX_EXPLANATION_CHARS + 150);
    }

    #[test]
    fn stop_details_explanation_takes_precedence_over_content() {
        let response = refusal(
            Some("partial answer that was cut off"),
            vec![json!({"type": "stop_details", "category": "cyber", "explanation": "Policy violation"})],
        );
        let reason = refusal_reason(&response);
        assert!(reason.contains(": policy violation."));
        assert!(!reason.contains("partial answer"));
    }

    #[test]
    fn plain_refusal_has_no_category_or_fallback() {
        assert_eq!(
            refusal_reason(&refusal(None, Vec::new())),
            "The model declined this request. The request was not retried; rephrase it or switch models."
        );
        assert!(is_refusal(&refusal(None, Vec::new())));
        assert!(!is_refusal(&LLMResponse::default()));
    }

    #[test]
    fn keeps_case_of_acronyms_after_colon() {
        let response = refusal(Some("CSAM content is not allowed"), Vec::new());
        assert!(refusal_reason(&response).contains(": CSAM content is not allowed."));
    }
}
