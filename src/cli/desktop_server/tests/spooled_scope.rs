use super::*;

#[test]
fn restored_activity_reconstructs_interleaved_interrupted_output_chunks() {
    use vtcode_core::core::agent::events::{tool_output_delta_event, tool_output_started_event};
    use vtcode_core::exec::events::ThreadItemDetails;
    let dir = tempfile::tempdir().unwrap();
    let workspace = vtcode_commons::canonicalize(dir.path()).unwrap();
    let log = vtcode_memory::open(&workspace, "interrupted-output", vtcode_memory::DEFAULT_MAX_EVENTS).unwrap();
    for event in [
        tool_output_started_event("a".into(), None),
        tool_output_delta_event("a".into(), None, "first "),
        tool_output_delta_event("orphan".into(), None, "tail"),
        tool_output_delta_event("a".into(), None, "second"),
        tool_output_started_event("controls".into(), None),
        tool_output_delta_event("controls".into(), None, "\0".repeat(12_000)),
    ] {
        log.append(&event).unwrap();
    }
    log.flush().unwrap();
    let history = restored_inspector_history(&workspace, "interrupted-output").unwrap();
    let output = |id: &str| {
        history
            .activity_items
            .iter()
            .find_map(|item| {
                if item.id == id
                    && let ThreadItemDetails::ToolOutput(output) = &item.details
                {
                    Some(output.output.as_str())
                } else {
                    None
                }
            })
            .unwrap()
    };
    assert_eq!(output("a:output"), "first second");
    assert_eq!(output("orphan:output"), format!("tail{RESTORED_OUTPUT_TRUNCATION_SUFFIX}"));
    assert!(output("controls:output").ends_with(RESTORED_OUTPUT_TRUNCATION_SUFFIX));
    assert!(output("controls:output").starts_with('\0'));
    assert!(history.activity_incomplete);
}

#[cfg(unix)]
#[test]
fn restored_activity_recovers_bounded_spooled_tool_output() {
    use vtcode_core::exec::events::*;

    let dir = tempfile::tempdir().unwrap();
    let workspace = vtcode_commons::canonicalize(dir.path()).unwrap();
    let spool_file = workspace.join(".vtcode/context/tool_outputs/call-1.txt");
    let spool_path = spool_file.to_string_lossy().into_owned();
    std::fs::create_dir_all(spool_file.parent().unwrap()).unwrap();
    std::fs::write(&spool_file, "complete captured tool output\nsecond line").unwrap();

    let log = vtcode_memory::open(&workspace, "spooled-output", vtcode_memory::DEFAULT_MAX_EVENTS).unwrap();
    log.append(&ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: tool_output_item("tool-output-1", Some(&spool_path), "bounded preview"),
    }))
    .unwrap();
    log.flush().unwrap();

    let restored = restored_inspector_history(&workspace, "spooled-output").unwrap();
    assert!(!restored.activity_incomplete);
    let ThreadItemDetails::ToolOutput(output) = &restored.activity_items[0].details else {
        panic!("expected a restored tool output item");
    };
    assert_eq!(output.output, "complete captured tool output\nsecond line");
    assert_eq!(output.spool_path, None, "local paths must not cross the protocol boundary");
}
#[cfg(unix)]
#[test]
fn restored_activity_rejects_unavailable_or_unsafe_spools_and_reports_partial_history() {
    use vtcode_core::exec::events::*;

    let dir = tempfile::tempdir().unwrap();
    let workspace = vtcode_commons::canonicalize(dir.path()).unwrap();
    let outside = tempfile::tempdir().unwrap();
    let outside_file = outside.path().join("private.txt");
    std::fs::write(&outside_file, "outside secret").unwrap();
    std::fs::write(workspace.join("private.txt"), "workspace secret").unwrap();
    let spool_dir = workspace.join(".vtcode/context/tool_outputs");
    std::fs::create_dir_all(&spool_dir).unwrap();
    std::os::unix::fs::symlink(&outside_file, spool_dir.join("linked.txt")).unwrap();

    let log = vtcode_memory::open(&workspace, "unsafe-spool", vtcode_memory::DEFAULT_MAX_EVENTS).unwrap();
    for (id, path, preview) in [
        ("symlink", ".vtcode/context/tool_outputs/linked.txt", "symlink preview"),
        ("missing", ".vtcode/context/tool_outputs/missing.txt", "missing preview"),
        ("escape", ".vtcode/context/tool_outputs/../../../private.txt", "escape preview"),
    ] {
        assert!(read_restored_spool_page(&workspace, path, 0).is_err());
        log.append(&ThreadEvent::ItemCompleted(ItemCompletedEvent { item: tool_output_item(id, Some(path), preview) }))
            .unwrap();
    }
    log.flush().unwrap();

    let restored = restored_inspector_history(&workspace, "unsafe-spool").unwrap();
    assert!(restored.activity_incomplete);
    assert_eq!(restored.activity_items.len(), 3);
    for (item, expected_preview) in
        restored
            .activity_items
            .iter()
            .zip(["symlink preview", "missing preview", "escape preview"])
    {
        let ThreadItemDetails::ToolOutput(output) = &item.details else {
            panic!("expected the inline preview to be retained");
        };
        assert_eq!(output.output, expected_preview);
        assert_eq!(output.spool_path, None);
        assert!(!output.output.contains("outside secret"));
        assert!(!output.output.contains("workspace secret"));
    }
}
#[cfg(unix)]
#[test]
fn malformed_spooled_output_uses_lossy_text_and_marks_history_incomplete() {
    use vtcode_core::exec::events::*;

    let dir = tempfile::tempdir().unwrap();
    let workspace = vtcode_commons::canonicalize(dir.path()).unwrap();
    let spool_path = ".vtcode/context/tool_outputs/malformed.txt";
    let spool_file = workspace.join(spool_path);
    std::fs::create_dir_all(spool_file.parent().unwrap()).unwrap();
    std::fs::write(&spool_file, [b'a', 0xff, b'b']).unwrap();

    let log = vtcode_memory::open(&workspace, "malformed-spool", vtcode_memory::DEFAULT_MAX_EVENTS).unwrap();
    log.append(&ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: tool_output_item("malformed-output", Some(spool_path), "preview"),
    }))
    .unwrap();
    log.flush().unwrap();

    let restored = restored_inspector_history(&workspace, "malformed-spool").unwrap();
    assert!(restored.activity_incomplete);
    assert!(restored.activity_output_cursors.is_empty());
    let ThreadItemDetails::ToolOutput(output) = &restored.activity_items[0].details else {
        panic!("expected a restored tool output item");
    };
    assert_eq!(output.output, format!("a�b{RESTORED_OUTPUT_TRUNCATION_SUFFIX}"));
}
#[cfg(unix)]
#[test]
fn restored_activity_truncates_large_spooled_output_with_visible_notice() {
    use vtcode_core::exec::events::*;

    let dir = tempfile::tempdir().unwrap();
    let workspace = vtcode_commons::canonicalize(dir.path()).unwrap();
    let spool_path = ".vtcode/context/tool_outputs/large.txt";
    let spool_file = workspace.join(spool_path);
    std::fs::create_dir_all(spool_file.parent().unwrap()).unwrap();
    std::fs::write(&spool_file, "x".repeat(MAX_RESTORED_SPOOLED_OUTPUT_BYTES + 1)).unwrap();

    let log = vtcode_memory::open(&workspace, "large-spool", vtcode_memory::DEFAULT_MAX_EVENTS).unwrap();
    log.append(&ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: tool_output_item("tool-output-1", Some(spool_path), "preview"),
    }))
    .unwrap();
    log.flush().unwrap();

    let restored = restored_inspector_history(&workspace, "large-spool").unwrap();
    assert!(restored.activity_incomplete);
    let ThreadItemDetails::ToolOutput(output) = &restored.activity_items[0].details else {
        panic!("expected a restored tool output item");
    };
    assert!(output.output.starts_with(&"x".repeat(MAX_RESTORED_SPOOLED_OUTPUT_BYTES - 64)));
    assert!(output.output.ends_with(RESTORED_OUTPUT_TRUNCATION_SUFFIX));
    assert_eq!(restored.activity_output_cursors.len(), 1);
    let cursor = &restored.activity_output_cursors[0];
    assert_eq!(cursor.item_id, "tool-output-1");
    assert!(cursor.next_offset > 0 && cursor.next_offset < cursor.total_bytes);
    assert_eq!(restored.output_spools.get(&cursor.item_id).map(String::as_str), Some(spool_path));
}
#[cfg(unix)]
#[test]
fn restored_spooled_output_pages_resume_on_utf8_boundaries() {
    use vtcode_core::exec::events::*;

    let dir = tempfile::tempdir().unwrap();
    let workspace = vtcode_commons::canonicalize(dir.path()).unwrap();
    let spool_path = ".vtcode/context/tool_outputs/multibyte.txt";
    let spool_file = workspace.join(spool_path);
    std::fs::create_dir_all(spool_file.parent().unwrap()).unwrap();
    let full_output = format!("{}{}", "a".repeat(MAX_RESTORED_SPOOLED_OUTPUT_BYTES - 1), "界".repeat(27_000));
    std::fs::write(&spool_file, &full_output).unwrap();

    let log = vtcode_memory::open(&workspace, "paged-spool", vtcode_memory::DEFAULT_MAX_EVENTS).unwrap();
    log.append(&ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: tool_output_item("tool-output-paged", Some(spool_path), "preview"),
    }))
    .unwrap();
    log.flush().unwrap();

    let restored = restored_inspector_history(&workspace, "paged-spool").unwrap();
    let ThreadItemDetails::ToolOutput(output) = &restored.activity_items[0].details else {
        panic!("expected a restored tool output item");
    };
    let mut assembled = output
        .output
        .strip_suffix(RESTORED_OUTPUT_TRUNCATION_SUFFIX)
        .unwrap()
        .to_owned();
    let cursor = &restored.activity_output_cursors[0];
    let mut offset = cursor.next_offset;
    assert_eq!(cursor.total_bytes, full_output.len() as u64);

    while offset < cursor.total_bytes {
        let page = read_restored_spool_page(&workspace, &restored.output_spools[&cursor.item_id], offset).unwrap();
        assert!(page.text.len() <= protocol::MAX_SESSION_OUTPUT_PAGE_BYTES);
        assert!(page.next_offset > offset);
        if page.next_offset < page.total_bytes {
            assert_eq!(page.next_offset - offset, page.text.len() as u64);
        }
        assembled.push_str(&page.text);
        offset = page.next_offset;
    }

    assert_eq!(assembled, full_output);
    assert!(read_restored_spool_page(&workspace, spool_path, cursor.total_bytes + 1).is_err());
}
#[cfg(unix)]
#[test]
fn restored_spooled_output_rejects_an_incomplete_utf8_sequence_at_eof() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = vtcode_commons::canonicalize(dir.path()).unwrap();
    let spool_path = ".vtcode/context/tool_outputs/incomplete-utf8.txt";
    let spool_file = workspace.join(spool_path);
    std::fs::create_dir_all(spool_file.parent().unwrap()).unwrap();
    std::fs::write(&spool_file, b"valid\xe2\x82").unwrap();

    let error = read_restored_spool_page(&workspace, spool_path, 0)
        .err()
        .expect("an incomplete UTF-8 tail must not be reported as a complete page");
    assert!(error.to_string().contains("incomplete UTF-8 sequence"));

    let mut item = tool_output_item("incomplete", Some(spool_path), "preview");
    let restored = restore_spooled_tool_output(&workspace, &mut item, 4096).unwrap();
    assert!(restored.incomplete);
    assert!(restored.cursor.is_none(), "an incomplete EOF cannot be paged");
    assert!(restored.spool_path.is_none());
    let vtcode_core::exec::events::ThreadItemDetails::ToolOutput(output) = item.details else {
        panic!("tool output");
    };
    assert!(output.output.contains("valid"));
    assert!(output.output.ends_with(RESTORED_OUTPUT_TRUNCATION_SUFFIX));

    std::fs::write(&spool_file, "valid界 tail").unwrap();
    let prefix = read_restored_spool_text(&workspace, spool_path, 6).unwrap();
    assert!(prefix.pageable, "a partial character before EOF is recoverable");
    assert_eq!(prefix.text, "valid");
    assert!(prefix.truncated);
    assert_eq!(read_restored_spool_page(&workspace, spool_path, 5).unwrap().text, "界 tail");
}
#[cfg(unix)]
#[test]
fn restored_activity_fits_json_escaped_spool_text_to_the_encoded_budget() {
    use vtcode_core::exec::events::*;

    let dir = tempfile::tempdir().unwrap();
    let workspace = vtcode_commons::canonicalize(dir.path()).unwrap();
    let spool_path = ".vtcode/context/tool_outputs/escaped.txt";
    let spool_file = workspace.join(spool_path);
    std::fs::create_dir_all(spool_file.parent().unwrap()).unwrap();
    std::fs::write(&spool_file, "\u{0001}".repeat(MAX_RESTORED_SPOOLED_OUTPUT_BYTES)).unwrap();

    let log = vtcode_memory::open(&workspace, "escaped-spool", vtcode_memory::DEFAULT_MAX_EVENTS).unwrap();
    log.append(&ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: tool_output_item("tool-output-1", Some(spool_path), "preview"),
    }))
    .unwrap();
    log.flush().unwrap();

    let restored = restored_inspector_history(&workspace, "escaped-spool").unwrap();
    assert!(restored.activity_incomplete);
    let item = restored.activity_items.first().expect("tool output should remain visible");
    let ThreadItemDetails::ToolOutput(output) = &item.details else {
        panic!("expected a restored tool output item");
    };
    assert!(output.output.starts_with('\u{0001}'));
    assert!(output.output.ends_with(RESTORED_OUTPUT_TRUNCATION_SUFFIX));
    assert!(serde_json::to_vec(item).unwrap().len() <= 64 * 1024);
}
#[test]
fn review_scope_is_bounded_to_existing_workspace_directories() {
    let workspace = tempfile::tempdir().unwrap();
    let root = vtcode_commons::canonicalize(workspace.path()).unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::create_dir(root.join(" spaced ")).unwrap();
    std::fs::write(root.join("file"), "not a directory").unwrap();
    assert_eq!(dispatch::resolve_review_scope(&root, None).unwrap(), root);
    assert_eq!(dispatch::resolve_review_scope(&root, Some("src")).unwrap(), root.join("src"));
    assert_eq!(dispatch::resolve_review_scope(&root, Some(" spaced ")).unwrap(), root.join(" spaced "));
    assert!(dispatch::resolve_review_scope(&root, Some("file")).is_err());
    assert!(dispatch::resolve_review_scope(&root, Some(outside.path().to_str().unwrap())).is_err());
    assert!(dispatch::resolve_review_scope(&root, Some(&"x".repeat(4097))).is_err());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(outside.path(), root.join("escape")).unwrap();
        assert!(dispatch::resolve_review_scope(&root, Some("escape")).is_err());
    }
}
#[test]
fn public_errors_remove_control_characters_and_limit_length() {
    let sanitized = sanitize_public_error(&format!("bad\u{001b}[31m{}", "x".repeat(300)));
    assert!(!sanitized.chars().any(char::is_control));
    assert!(sanitized.chars().count() <= 240);
}
#[test]
fn public_errors_redact_provider_secrets() {
    let sanitized = sanitize_public_error("request failed with api_key=supersecretvalue and sk-1234567890abcdef");
    assert!(!sanitized.contains("supersecretvalue"));
    assert!(!sanitized.contains("sk-1234567890abcdef"));
    assert!(sanitized.contains("[REDACTED_SECRET]"));
}
#[test]
fn session_workspace_labels_are_file_names() {
    assert_eq!(workspace_label(Path::new("/tmp/example-workspace")), "example-workspace");
    assert_eq!(workspace_label(Path::new("/")), "workspace");
}
