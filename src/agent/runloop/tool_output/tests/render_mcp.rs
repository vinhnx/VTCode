use super::*;

#[tokio::test]
async fn render_tool_output_mcp_structured_fallback_and_content_precedence() {
    for (payload, expected, absent) in [
        (json!({"structuredContent": {"answer": 42}}), "answer: 42", "unused"),
        (
            json!({"content": [{"type": "text", "text": " \n"}],
                "structuredContent": {"status": "ready"}}),
            "status: ready",
            "unused",
        ),
        (
            json!({"content": [{"type": "text", "text": ""}],
                   "structuredContent": {"status": "empty text"}}),
            "status: empty text",
            "unused",
        ),
        (
            json!({"content": [{"type": "text", "text":
                r#"{"answer":5,"_meta":"legacy hidden","model":"legacy hidden","isError":false}"#}],
                "structuredContent": {"duplicate": 91}}),
            "answer: 5",
            "legacy hidden",
        ),
        (
            json!({"content": [{"type": "image", "data": "aW1hZ2U=", "mimeType": "image/png"}],
                "structuredContent": {"answer": 91}}),
            "[image content]",
            "answer",
        ),
        (json!({"structuredContent": {}}), "{}", "unused"),
        (json!({"structuredContent": []}), "[]", "unused"),
        (json!({"structuredContent": null}), "null", "unused"),
        (
            json!({"structuredContent": {"_meta": "public application data"}}),
            "public application data",
            "unused",
        ),
        (
            json!({"content": [{"type": "text", "text": "ordinary content"}],
                "structuredContent": {"answer": 91}}),
            "ordinary content",
            "answer",
        ),
    ] {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
        render_tool_output(&mut renderer, Some("mcp_example_lookup"), &payload, None)
            .await
            .unwrap();
        let output = collect_inline_output(&mut receiver);
        assert_eq!(output.matches(expected).count(), 1, "{output:?}");
        assert!(!output.contains(absent), "{output:?}");
    }
}
#[tokio::test]
async fn render_tool_output_mcp_structured_preserves_mixed_application_fields() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({"structuredContent": {
        "answer": 42, "_meta": "public application data", "model": "application model", "isError": true
    }});

    render_tool_output(&mut renderer, Some("mcp_example_lookup"), &payload, None)
        .await
        .unwrap();

    let output = collect_inline_output(&mut receiver);
    for expected in [
        "answer: 42",
        "_meta: public application data",
        "model: application model",
        "isError: true",
    ] {
        assert_eq!(output.matches(expected).count(), 1, "{output:?}");
    }
}
#[tokio::test]
async fn render_tool_output_mcp_blank_rows_do_not_hide_structured_results() {
    for (payload, expected) in [
        (json!({"structuredContent": ""}), "\"\""),
        (json!({"structuredContent": "\n\t"}), "\"\\n\\t\""),
        (
            json!({"content": [{"type": "text", "text": "\"\""}],
                "structuredContent": {"answer": 17}}),
            "answer: 17",
        ),
    ] {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());

        render_tool_output(&mut renderer, Some("mcp_example_lookup"), &payload, None)
            .await
            .unwrap();

        let output = collect_inline_output(&mut receiver);
        assert_eq!(output.matches(expected).count(), 1, "{output:?}");
    }
}
#[tokio::test]
async fn render_tool_output_mcp_custom_profile_keeps_precedence() {
    use vtcode_core::config::loader::VTCodeConfig;
    use vtcode_core::config::mcp::McpRendererProfile;

    let mut config = VTCodeConfig::default();
    config
        .mcp
        .ui
        .renderers
        .insert("example".to_owned(), McpRendererProfile::SequentialThinking);
    let payload = json!({"summary": "custom summary", "structuredContent": {"answer": 73}});
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());

    render_tool_output(&mut renderer, Some("mcp_example_lookup"), &payload, Some(&config))
        .await
        .unwrap();

    let output = collect_inline_output(&mut receiver);
    assert_eq!(output.matches("custom summary").count(), 1, "{output:?}");
    assert!(!output.contains("answer"), "{output:?}");
}
#[tokio::test]
async fn render_tool_output_mcp_resources_and_media_preserve_content_order() {
    let payload = json!({"content": [
        {"type": "text", "text": "first public text"},
        {"type": "resource", "resource": {
            "uri": "file:///report.txt", "mimeType": "text/plain", "text": "embedded report body"}},
        {"type": "audio", "mimeType": "audio/wav", "data": "PRIVATE_AUDIO_PAYLOAD"},
        {"type": "resource_link", "uri": "https://example.com/manual", "name": "Public manual",
            "mimeType": "text/html"},
        {"type": "resource", "resource": {
            "uri": "file:///archive.bin", "mimeType": "application/octet-stream", "blob": "PRIVATE_BLOB_PAYLOAD"}},
        {"type": "image", "mimeType": "image/png", "data": "PRIVATE_IMAGE_PAYLOAD"},
        {"type": "text", "text": "last public text"}
    ], "structuredContent": {"duplicate_fallback": true}});
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());

    render_tool_output(&mut renderer, Some("mcp_example_lookup"), &payload, None)
        .await
        .unwrap();

    let output = collect_inline_output(&mut receiver);
    let mut previous_end = 0;
    for expected in [
        "first public text",
        "[resource: file:///report.txt]",
        "type: text/plain",
        "embedded report body",
        "[audio content]",
        "type: audio/wav",
        "[resource: https://example.com/manual]",
        "name: Public manual",
        "type: text/html",
        "[resource: file:///archive.bin]",
        "type: application/octet-stream",
        "[image content]",
        "type: image/png",
        "last public text",
    ] {
        assert_eq!(output.matches(expected).count(), 1, "{output:?}");
        let position = output.find(expected).unwrap();
        assert!(position >= previous_end, "out-of-order {expected:?}: {output:?}");
        previous_end = position + expected.len();
    }
    assert!(!output.contains("PRIVATE_"), "{output:?}");
    assert!(!output.contains("duplicate_fallback"), "{output:?}");
}
#[tokio::test]
async fn render_tool_output_mcp_media_only_results_take_precedence_over_structured_fallback() {
    for (content, expected) in [
        (json!({"type": "audio", "mimeType": "audio/ogg", "data": "PRIVATE_AUDIO"}), "[audio content]"),
        (
            json!({"type": "resource_link", "uri": "https://example.com/link", "name": "Link"}),
            "[resource: https://example.com/link]",
        ),
        (
            json!({"type": "resource", "resource": {"uri": "file:///empty.txt", "text": ""}}),
            "[resource: file:///empty.txt]",
        ),
        (json!({"type": "resource", "uri": "file:///legacy.txt"}), "[resource: file:///legacy.txt]"),
    ] {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
        let payload = json!({"content": [content], "structuredContent": {"duplicate_fallback": true}});

        render_tool_output(&mut renderer, Some("mcp_example_lookup"), &payload, None)
            .await
            .unwrap();

        let output = collect_inline_output(&mut receiver);
        assert_eq!(output.matches(expected).count(), 1, "{output:?}");
        assert!(!output.contains("duplicate_fallback"), "{output:?}");
        assert!(!output.contains("PRIVATE_"), "{output:?}");
    }
}
#[tokio::test]
async fn render_tool_output_mcp_resource_text_reuses_json_and_code_formatting() {
    for (text, expected, absent) in [
        (r#"{"count":7}"#, "count: 7", "{\"count\""),
        ("```rust\nlet count = 3;\n```", "let count = 3;", "```"),
    ] {
        let payload = json!({"content": [{"type": "resource", "resource": {
            "uri": "file:///sample.txt", "text": text
        }}]});
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());

        render_tool_output(&mut renderer, Some("mcp_example_lookup"), &payload, None)
            .await
            .unwrap();

        let output = collect_inline_output(&mut receiver);
        assert_eq!(output.matches(expected).count(), 1, "{output:?}");
        assert!(!output.contains(absent), "{output:?}");
        assert!(output.contains("[resource: file:///sample.txt]"), "{output:?}");
    }
}
