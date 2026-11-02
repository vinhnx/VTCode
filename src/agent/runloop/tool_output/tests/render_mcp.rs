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
async fn render_tool_output_mcp_ui_scheme_renders_app_linkage_row() {
    for (content, expected) in [
        (
            json!({"type": "resource", "resource": {
                "uri": "ui://get-time/mcp-app.html", "mimeType": "text/html;profile=mcp-app"}}),
            "[app: get-time] ui://get-time/mcp-app.html",
        ),
        (
            json!({"type": "resource_link", "uri": "ui://charts/dashboard.html", "name": "Dashboard",
                "mimeType": "text/html;profile=mcp-app"}),
            "[app: charts] ui://charts/dashboard.html",
        ),
        (
            json!({"type": "resource_link", "uri": "ui://weather-dashboard"}),
            "[app: weather-dashboard] ui://weather-dashboard",
        ),
        (json!({"type": "resource_link", "uri": "ui:///orphan.html"}), "[app: orphan.html] ui:///orphan.html"),
    ] {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
        let payload = json!({"content": [content]});

        render_tool_output(&mut renderer, Some("mcp_example_lookup"), &payload, None)
            .await
            .unwrap();

        let output = collect_inline_output(&mut receiver);
        assert_eq!(output.matches(expected).count(), 1, "{output:?}");
        assert!(!output.contains("[resource: ui://"), "{output:?}");
        assert!(
            output.contains("not rendered: interactive apps require the desktop or browser client"),
            "{output:?}"
        );
    }
}
#[tokio::test]
async fn render_tool_output_mcp_app_and_resource_labels_differ_by_scheme_only() {
    let payload = json!({"content": [
        {"type": "resource", "resource": {"uri": "ui://get-time/mcp-app.html"}},
        {"type": "resource", "resource": {"uri": "https://example.com/mcp-app.html"}},
    ]});
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());

    render_tool_output(&mut renderer, Some("mcp_example_lookup"), &payload, None)
        .await
        .unwrap();

    let output = collect_inline_output(&mut receiver);
    assert!(output.contains("[app: get-time] ui://get-time/mcp-app.html"), "{output:?}");
    assert!(output.contains("[resource: https://example.com/mcp-app.html]"), "{output:?}");
}
#[tokio::test]
async fn render_tool_output_mcp_app_disclosure_block_shows_sandbox_posture() {
    let payload = json!({
        "content": [
            {"type": "text", "text": "at 10:00"},
            {"type": "resource_link", "uri": "ui://get-time/mcp-app.html", "name": "Time",
                "mimeType": "text/html;profile=mcp-app"},
        ],
        "uiDisclosure": {
            "ui://get-time/mcp-app.html": {
                "uri": "ui://get-time/mcp-app.html",
                "mimeType": "text/html;profile=mcp-app",
                "bytes": 4312,
                "blocks": 1,
                "ui": {
                    "connectDomains": ["https://api.example.com", "https://cdn.jsdelivr.net"],
                    "resourceDomains": [],
                    "frameDomains": [],
                    "baseUriDomains": [],
                    "permissions": ["camera"],
                    "domain": null,
                },
            }
        },
    });
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());

    render_tool_output(&mut renderer, Some("mcp_example_lookup"), &payload, None)
        .await
        .unwrap();

    let output = collect_inline_output(&mut receiver);
    let mut previous_end = 0;
    for expected in [
        "at 10:00",
        "[app: get-time] ui://get-time/mcp-app.html",
        "name: Time",
        "type: text/html;profile=mcp-app (4.2KB)",
        "sandbox: csp connect=https://api.example.com, https://cdn.jsdelivr.net · permissions=camera",
        "not rendered: interactive apps require the desktop or browser client",
    ] {
        assert_eq!(output.matches(expected).count(), 1, "{output:?}");
        let position = output.find(expected).unwrap();
        assert!(position >= previous_end, "out-of-order {expected:?}: {output:?}");
        previous_end = position + expected.len();
    }
}
#[tokio::test]
async fn render_tool_output_mcp_app_without_disclosure_omits_sandbox_but_keeps_footer() {
    let payload = json!({"content": [
        {"type": "resource", "resource": {
            "uri": "ui://get-time/mcp-app.html", "mimeType": "text/html;profile=mcp-app", "text": "SHORT_BODY"}},
    ]});
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());

    render_tool_output(&mut renderer, Some("mcp_example_lookup"), &payload, None)
        .await
        .unwrap();

    let output = collect_inline_output(&mut receiver);
    assert!(output.contains("[app: get-time] ui://get-time/mcp-app.html"), "{output:?}");
    assert!(output.contains("type: text/html;profile=mcp-app (10B)"), "{output:?}");
    assert!(!output.contains("sandbox:"), "{output:?}");
    assert!(!output.contains("SHORT_BODY"), "{output:?}");
    assert!(
        output.contains("not rendered: interactive apps require the desktop or browser client"),
        "{output:?}"
    );
}
#[tokio::test]
async fn render_tool_output_mcp_app_without_mime_shows_unknown_type_with_size() {
    // Embedded body without any declared MIME type: size still discloses.
    let payload = json!({"content": [
        {"type": "resource", "resource": {"uri": "ui://demo/app.html", "text": "0123456789"}},
    ]});
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());

    render_tool_output(&mut renderer, Some("mcp_example_lookup"), &payload, None)
        .await
        .unwrap();

    let output = collect_inline_output(&mut receiver);
    assert!(output.contains("type: unknown (10B)"), "{output:?}");

    // Bare link with no MIME and no body: no type row at all.
    let payload = json!({"content": [{"type": "resource_link", "uri": "ui://demo/app.html"}]});
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());

    render_tool_output(&mut renderer, Some("mcp_example_lookup"), &payload, None)
        .await
        .unwrap();

    let output = collect_inline_output(&mut receiver);
    assert!(output.contains("[app: demo] ui://demo/app.html"), "{output:?}");
    assert!(!output.contains("type:"), "{output:?}");
}

#[tokio::test]
async fn render_tool_output_mcp_app_disclosure_wins_over_block_fields() {
    let payload = json!({
        "content": [
            {"type": "resource", "resource": {
                "uri": "ui://demo/app.html", "mimeType": "text/plain", "text": "stale"}},
        ],
        "uiDisclosure": {
            "ui://demo/app.html": {
                "uri": "ui://demo/app.html",
                "mimeType": "text/html;profile=mcp-app",
                "bytes": 2048,
                "blocks": 1,
                "ui": null,
            }
        },
    });
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());

    render_tool_output(&mut renderer, Some("mcp_example_lookup"), &payload, None)
        .await
        .unwrap();

    let output = collect_inline_output(&mut receiver);
    assert!(output.contains("type: text/html;profile=mcp-app (2.0KB)"), "{output:?}");
    assert!(!output.contains("text/plain"), "{output:?}");
    assert!(!output.contains("stale"), "{output:?}");
    // Null posture discloses nothing: no sandbox line.
    assert!(!output.contains("sandbox:"), "{output:?}");
}
#[tokio::test]
async fn render_tool_output_mcp_app_empty_posture_states_no_external_access() {
    let payload = json!({
        "content": [{"type": "resource_link", "uri": "ui://locked/app.html"}],
        "uiDisclosure": {
            "ui://locked/app.html": {
                "uri": "ui://locked/app.html",
                "mimeType": "text/html;profile=mcp-app",
                "bytes": 64,
                "blocks": 1,
                "ui": {
                    "connectDomains": [],
                    "resourceDomains": [],
                    "frameDomains": [],
                    "baseUriDomains": [],
                    "permissions": [],
                    "domain": null,
                },
            }
        },
    });
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());

    render_tool_output(&mut renderer, Some("mcp_example_lookup"), &payload, None)
        .await
        .unwrap();

    let output = collect_inline_output(&mut receiver);
    assert!(output.contains("sandbox: no external domains or permissions declared"), "{output:?}");
}
#[tokio::test]
async fn render_tool_output_mcp_app_block_neutralizes_untrusted_text() {
    let payload = json!({
        "content": [
            {"type": "resource_link", "uri": "ui://evil/\nINJECTED\x07", "name": "Na`me` $x",
                "mimeType": "text/html\x1b[31m"},
        ],
        "uiDisclosure": {
            "ui://evil/\nINJECTED\x07": {
                "uri": "ui://evil/\nINJECTED\x07",
                "mimeType": "text/html\x1b[31m",
                "bytes": 8,
                "blocks": 1,
                "ui": {
                    "connectDomains": ["https://a.example\nB", "https://$b.example/`c`"],
                    "resourceDomains": [],
                    "frameDomains": [],
                    "baseUriDomains": [],
                    "permissions": [],
                    "domain": null,
                },
            }
        },
    });
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());

    render_tool_output(&mut renderer, Some("mcp_example_lookup"), &payload, None)
        .await
        .unwrap();

    let output = collect_inline_output(&mut receiver);
    // Controls become U+FFFD; no injected row and no bell/escape bytes survive.
    assert!(output.contains("ui://evil/�INJECTED�"), "{output:?}");
    assert!(!output.lines().any(|line| line.trim_start().starts_with("INJECTED")), "{output:?}");
    assert!(!output.contains('\x07'), "{output:?}");
    assert!(!output.contains('\x1b'), "{output:?}");
    assert!(output.contains("https://a.example�B"), "{output:?}");
    // Shell metacharacters are inert in terminal output and preserved.
    assert!(output.contains("Na`me` $x"), "{output:?}");
    assert!(output.contains("https://$b.example/`c`"), "{output:?}");
}
#[tokio::test]
async fn render_tool_output_mcp_plain_resource_rows_neutralize_untrusted_text() {
    let payload = json!({"content": [
        {"type": "resource_link", "uri": "https://example.com/\nINJECTED\x07", "name": "Na\nme",
            "mimeType": "text/html\x1b[31m"},
        {"type": "resource", "resource": {
            "uri": "file:///ok.txt", "mimeType": "text/plain", "text": "fine"}},
    ]});
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());

    render_tool_output(&mut renderer, Some("mcp_example_lookup"), &payload, None)
        .await
        .unwrap();

    let output = collect_inline_output(&mut receiver);
    assert!(output.contains("[resource: https://example.com/�INJECTED�]"), "{output:?}");
    assert!(output.contains("name: Na�me"), "{output:?}");
    assert!(output.contains("type: text/html�[31m"), "{output:?}");
    assert!(!output.lines().any(|line| line.trim_start().starts_with("INJECTED")), "{output:?}");
    assert!(!output.contains('\x07'), "{output:?}");
    assert!(!output.contains('\x1b'), "{output:?}");
    // Clean rows pass through byte-identical.
    assert!(output.contains("[resource: file:///ok.txt]"), "{output:?}");
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
