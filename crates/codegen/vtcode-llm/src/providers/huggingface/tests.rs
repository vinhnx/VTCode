use super::HuggingFaceProvider;
use crate::provider::{LLMRequest, Message, ToolDefinition};
use crate::providers::common::{is_minimax_m2_model, normalize_reasoning_detail_object};
use serde_json::json;
use std::sync::Arc;

#[test]
fn minimax_model_detection_handles_variants() {
    assert!(is_minimax_m2_model("MiniMaxAI/MiniMax-M2.5:novita"));
    assert!(is_minimax_m2_model("minimax-m2.5"));
    assert!(!is_minimax_m2_model("deepseek-r1"));
}

#[test]
fn normalize_reasoning_detail_decodes_stringified_object() {
    let parsed = normalize_reasoning_detail_object(&json!("{\"type\":\"reasoning.text\",\"text\":\"step\"}"))
        .expect("expected a parsed reasoning detail object");
    assert!(parsed.is_object());
    assert_eq!(parsed["type"], "reasoning.text");
}

#[test]
fn serialize_messages_normalizes_minimax_reasoning_details() {
    let provider = HuggingFaceProvider::with_model("test-key".to_string(), "MiniMaxAI/MiniMax-M2.5:novita".to_string());
    let request = LLMRequest {
        model: "MiniMaxAI/MiniMax-M2.5:novita".to_string(),
        messages: vec![
            Message::assistant("answer".to_string())
                .with_reasoning_details(Some(vec![json!("{\"type\":\"reasoning.text\",\"text\":\"chain\"}")])),
        ]
        .into(),
        ..Default::default()
    };

    let messages = provider
        .serialize_messages_huggingface_chat(&request)
        .expect("message serialization should succeed");
    assert!(messages[0]["reasoning_details"].is_array());
    assert!(messages[0]["reasoning_details"][0].is_object());
}

#[test]
fn serialize_messages_rehydrates_glm_interleaved_history_into_content() {
    let provider = HuggingFaceProvider::with_model("test-key".to_string(), "zai-org/GLM-5.1:novita".into());
    let request = LLMRequest {
        model: "zai-org/GLM-5.1:novita".to_string(),
        messages: vec![Message::assistant("done".to_string()).with_reasoning(Some("trace".to_string()))].into(),
        ..Default::default()
    };

    let messages = provider
        .serialize_messages_huggingface_chat(&request)
        .expect("message serialization should succeed");

    assert_eq!(messages[0]["content"], json!("<think>trace</think>done"));
}

#[test]
fn format_for_chat_completions_keeps_apply_patch_as_function_tool() {
    let provider =
        HuggingFaceProvider::with_model("test-key".to_string(), "Qwen/Qwen3-Coder-480B-A35B-Instruct".to_string());
    let request = LLMRequest {
        model: "Qwen/Qwen3-Coder-480B-A35B-Instruct".to_string(),
        messages: vec![Message::user("apply a patch".to_string())].into(),
        tools: Some(Arc::new(vec![ToolDefinition::apply_patch("Apply patches".to_string())])),
        ..Default::default()
    };

    let payload = provider
        .format_for_chat_completions(&request)
        .expect("payload should serialize");

    assert_eq!(payload["tools"][0]["type"], "function");
    assert_eq!(payload["tools"][0]["function"]["name"], "apply_patch");
}
