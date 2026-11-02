use super::*;

#[test]
fn chat_payload_rejects_file_url_content_parts() {
    let provider = native_openai_provider(models::openai::DEFAULT_MODEL);
    let request = provider::LLMRequest {
        messages: vec![provider::Message::user_with_parts(vec![
            provider::ContentPart::file_from_url("https://example.com/doc.pdf".to_string()),
        ])]
        .into(),
        model: models::openai::DEFAULT_MODEL.to_string(),
        ..Default::default()
    };
    let err = provider
        .convert_to_openai_format(&request)
        .expect_err("chat payload should reject file_url");
    match err {
        provider::LLMError::InvalidRequest { message, .. } => {
            assert!(message.contains("does not support file_url"));
        }
        other => panic!("unexpected error variant: {other:?}"),
    }
}

#[test]
fn responses_payload_serializes_user_input_file_by_id() {
    let provider = native_openai_provider(models::openai::GPT_5);
    let request = provider::LLMRequest {
        messages: vec![provider::Message::user_with_parts(vec![
            provider::ContentPart::text("Summarize this file".to_string()),
            provider::ContentPart::file_from_id("file-abc123".to_string()),
        ])]
        .into(),
        model: models::openai::GPT_5.to_string(),
        ..Default::default()
    };
    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");
    let input = get_input_array(&payload);
    let content = input[0]
        .get("content")
        .and_then(Value::as_array)
        .expect("user content should be an array");
    assert!(content.iter().any(|part| {
        part.get("type").and_then(Value::as_str) == Some("input_file")
            && part.get("file_id").and_then(Value::as_str) == Some("file-abc123")
    }));
}

#[test]
fn responses_payload_serializes_user_input_file_data() {
    let provider = native_openai_provider(models::openai::GPT_5);
    let request = provider::LLMRequest {
        messages: vec![provider::Message::user_with_parts(vec![
            provider::ContentPart::text("Summarize this file".to_string()),
            provider::ContentPart::file_from_data("report.pdf".to_string(), "aGVsbG8=".to_string()),
        ])]
        .into(),
        model: models::openai::GPT_5.to_string(),
        ..Default::default()
    };
    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");
    let input = get_input_array(&payload);
    let content = input[0]
        .get("content")
        .and_then(Value::as_array)
        .expect("user content should be an array");
    assert!(content.iter().any(|part| {
        part.get("type").and_then(Value::as_str) == Some("input_file")
            && part.get("filename").and_then(Value::as_str) == Some("report.pdf")
            && part.get("file_data").and_then(Value::as_str) == Some("data:application/pdf;base64,aGVsbG8=")
    }));
}

#[test]
fn responses_payload_normalizes_inline_files_without_mutating_history() {
    let provider = native_openai_provider(models::openai::GPT_6_LUNA);
    for (filename, data, expected) in [
        ("README.md", "IyBSZWFkbWUK", "data:text/markdown;base64,IyBSZWFkbWUK"),
        ("REPORT.PDF", "JVBERi0xLjcK", "data:application/pdf;base64,JVBERi0xLjcK"),
        ("main.rs", "Zm4gbWFpbigpIHt9", "data:text/plain;base64,Zm4gbWFpbigpIHt9"),
        ("data.csv", "YSxiCjEsMgo=", "data:text/csv;base64,YSxiCjEsMgo="),
        ("notes.txt", "data:text/plain;base64,aGVsbG8=", "data:text/plain;base64,aGVsbG8="),
        ("notes.txt", " aGVsbG8=\n", "data:text/plain;base64,aGVsbG8="),
        (
            "report.docx",
            "UEsDBA==",
            "data:application/vnd.openxmlformats-officedocument.wordprocessingml.document;base64,UEsDBA==",
        ),
        (
            "deck.pptx",
            "UEsDBA==",
            "data:application/vnd.openxmlformats-officedocument.presentationml.presentation;base64,UEsDBA==",
        ),
        (
            "sheet.xlsx",
            "UEsDBA==",
            "data:application/vnd.openxmlformats-officedocument.spreadsheetml.sheet;base64,UEsDBA==",
        ),
    ] {
        let request = provider::LLMRequest {
            messages: vec![provider::Message::user_with_parts(vec![
                provider::ContentPart::text("use plan mode, refine".to_string()),
                provider::ContentPart::file_from_data(filename.to_string(), data.to_string()),
            ])]
            .into(),
            model: models::openai::GPT_6_LUNA.to_string(),
            ..Default::default()
        };
        let original_messages = request.messages.clone();
        let payload = provider.convert_to_openai_responses_format(&request).expect("valid file input");
        let content = &get_input_array(&payload)[0]["content"];
        assert_eq!(content[0]["text"], "use plan mode, refine");
        assert_eq!(content[1]["type"], "input_file");
        assert_eq!(content[1]["filename"], filename);
        assert_eq!(content[1]["file_data"], expected);
        assert_eq!(request.messages, original_messages);
    }
}

#[test]
fn responses_payload_preserves_remote_file_url() {
    let provider = native_openai_provider(models::openai::GPT_6_LUNA);
    let request = provider::LLMRequest {
        messages: vec![provider::Message::user_with_parts(vec![
            provider::ContentPart::file_from_url("https://example.com/report.pdf".to_string()),
        ])]
        .into(),
        model: models::openai::GPT_6_LUNA.to_string(),
        ..Default::default()
    };
    let payload = provider.convert_to_openai_responses_format(&request).expect("valid URL input");
    let part = &get_input_array(&payload)[0]["content"][0];
    assert_eq!(part["type"], "input_file");
    assert_eq!(part["file_url"], "https://example.com/report.pdf");
    assert!(part.get("file_data").is_none());
    assert!(part.get("file_id").is_none());
}

#[test]
fn responses_validation_counts_data_url_bytes_without_the_prefix() {
    let request = provider::LLMRequest {
        messages: vec![provider::Message::user_with_parts(vec![
            provider::ContentPart::file_from_data(
                "notes.txt".to_string(),
                "data:text/plain;base64,aGVsbG8=".to_string(),
            ),
        ])]
        .into(),
        model: models::openai::GPT_6_LUNA.to_string(),
        ..Default::default()
    };
    OpenAIProvider::validate_inline_file_inputs_with_limit(&request, 5).expect("five decoded bytes fit");
    assert!(OpenAIProvider::validate_inline_file_inputs_with_limit(&request, 4).is_err());
}

#[test]
fn responses_validation_rejects_malformed_inline_file_data() {
    let provider = native_openai_provider(models::openai::GPT_6_LUNA);
    for data in ["not base64!", "data:text/markdown;base64,not base64!"] {
        let request = provider::LLMRequest {
            messages: vec![provider::Message::user_with_parts(vec![
                provider::ContentPart::file_from_data("README.md".to_string(), data.to_string()),
            ])]
            .into(),
            model: models::openai::GPT_6_LUNA.to_string(),
            ..Default::default()
        };
        assert!(matches!(
            provider.convert_to_openai_responses_format(&request),
            Err(provider::LLMError::InvalidRequest { .. })
        ));
    }
}

#[test]
fn responses_validation_rejects_single_inline_file_over_limit() {
    let request = provider::LLMRequest {
        messages: vec![provider::Message::user_with_parts(vec![
            provider::ContentPart::file_from_data("report.pdf".to_string(), "aGVsbG8=".to_string()),
        ])]
        .into(),
        model: models::openai::GPT_5.to_string(),
        ..Default::default()
    };
    let err = OpenAIProvider::validate_inline_file_inputs_with_limit(&request, 4)
        .expect_err("inline file should exceed limit");
    match err {
        provider::LLMError::InvalidRequest { message, .. } => {
            assert!(message.contains("50 MB request limit"));
            assert!(message.contains("report.pdf"));
        }
        other => panic!("unexpected error variant: {other:?}"),
    }
}

#[test]
fn responses_validation_rejects_combined_inline_files_over_limit() {
    let request = provider::LLMRequest {
        messages: vec![provider::Message::user_with_parts(vec![
            provider::ContentPart::file_from_data("a.txt".to_string(), "YWJj".to_string()),
            provider::ContentPart::file_from_data("b.txt".to_string(), "ZGVm".to_string()),
        ])]
        .into(),
        model: models::openai::GPT_5.to_string(),
        ..Default::default()
    };
    let err = OpenAIProvider::validate_inline_file_inputs_with_limit(&request, 5)
        .expect_err("combined inline files should exceed limit");
    match err {
        provider::LLMError::InvalidRequest { message, .. } => {
            assert!(message.contains("50 MB request limit"));
            assert!(message.contains("total inline file bytes = 6"));
        }
        other => panic!("unexpected error variant: {other:?}"),
    }
}
