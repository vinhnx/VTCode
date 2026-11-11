use super::*;
use vtcode_core::llm::provider::{LLMResponse, ToolCall};

fn tool_free_violated_response(content: Option<&str>) -> LLMResponse {
    let mut response = LLMResponse::new("test", content.unwrap_or_default());
    response.content = content.map(str::to_string);
    response.tool_calls = Some(vec![ToolCall::function(
        "forbidden".to_string(),
        "read_file".to_string(),
        r#"{"path":"README.md"}"#.to_string(),
    )]);
    response
}

#[test]
fn tool_free_violation_preserves_voided_prose_for_salvage() {
    let prose = "## Summary\nRefine the README overview.";
    let response = tool_free_violated_response(Some(prose));
    let mut renderer = AnsiRenderer::stdout();
    let result = process_llm_response(&response, &mut renderer, 0, true, false, false, false, None, None).unwrap();
    let TurnProcessingResult::Empty { facts } = result else {
        panic!("violated tool-free response must classify as empty, got text or tool calls");
    };
    assert!(facts.dropped_tool_calls);
    assert_eq!(facts.voided_text.as_deref(), Some(prose));
}

#[test]
fn tool_free_violation_without_prose_carries_no_salvage() {
    let response = tool_free_violated_response(None);
    let mut renderer = AnsiRenderer::stdout();
    let result = process_llm_response(&response, &mut renderer, 0, true, false, false, false, None, None).unwrap();
    let TurnProcessingResult::Empty { facts } = result else {
        panic!("content-free violation must classify as empty");
    };
    assert!(facts.dropped_tool_calls);
    assert!(facts.voided_text.is_none());
}
