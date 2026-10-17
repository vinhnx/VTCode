use super::*;

fn plan(content: &str, old: &str, new: &str) -> Result<ReplacementPlan, EditFailure> {
    ReplacementPlan::build(Path::new("/fixture/doc.md"), content, old, new)
}

#[test]
fn exact_precedes_whitespace_and_noop_succeeds() {
    let source = "  same  \nsame\ntail";
    let replacement = plan(source, "same", "changed").unwrap_err();
    assert_eq!(replacement.failure_kind, EditFailureKind::AmbiguousMatch);
    let source = "  same  \nsame\ntail";
    let replacement = plan(source, "same\n", "changed\n").unwrap();
    assert_eq!(replacement.match_kind, MatchKind::Exact);
    assert_eq!(replacement.line_range, 1..2);
    assert_eq!(replacement.apply(source), "  same  \nchanged\ntail");
    let noop = plan("αβ\n", "αβ", "αβ").unwrap();
    assert_eq!(noop.apply("αβ\n"), "αβ\n");
    assert_eq!(noop.line_range, 0..1);
}

#[test]
fn whitespace_splice_preserves_untouched_bytes_and_endings() {
    for (source, old, new, expected) in [
        (
            "prefix\r\n  one \r\n two\t\r\nlast",
            "one\ntwo",
            "first\nsecond",
            "prefix\r\nfirst\r\nsecond\r\nlast",
        ),
        ("prefix\n  é \n尾", "é\n尾", "改\nend", "prefix\n改\nend"),
        ("prefix\n  one \nlast\n", "one\t", "new", "prefix\nnew\nlast\n"),
    ] {
        let replacement = plan(source, old, new).unwrap();
        assert_eq!(replacement.match_kind, MatchKind::LineEdgeWhitespace);
        assert_eq!(replacement.apply(source), expected);
        assert_eq!(replacement.line_range.start, 1);
    }
}

#[test]
fn internal_whitespace_and_unicode_punctuation_are_significant() {
    for (source, old) in [
        ("let s = \"a  b\";\n", "let s = \"a b\";"),
        ("a\tb\n", "a b"),
        ("“quoted”\n", "\"quoted\""),
    ] {
        let failure = plan(source, old, "bad").unwrap_err();
        assert_eq!(failure.failure_kind, EditFailureKind::NoMatch);
        assert!(failure.candidate_locations.is_empty());
        assert!(failure.next_read_args.is_none());
        assert!(failure.next_search_args.is_some());
        assert!(!failure.to_string().contains(source.trim()));
    }
    let blank = plan("nonblank", " \t", "new").unwrap_err();
    assert_eq!(blank.failure_kind, EditFailureKind::NoMatch);
    assert!(blank.next_search_args.is_none(), "never suggest an invalid empty search");
}

#[test]
fn ambiguous_matches_are_bounded_at_the_selected_level() {
    for (source, old) in [
        ("aaa", "aa"),
        ("  same \n\tsame\n", "same\t\n"),
        ("x\n".repeat(100).as_str(), "x"),
    ] {
        let failure = plan(source, old, "new").unwrap_err();
        assert_eq!(failure.failure_kind, EditFailureKind::AmbiguousMatch);
        assert!(failure.candidate_locations.len() <= 5);
        assert!(failure.next_read_args.as_ref().unwrap()["limit"].as_u64().unwrap() <= 200);
    }
    assert_eq!(plan("aaa", "aa", "new").unwrap_err().candidate_locations.len(), 2);
}

#[test]
fn exact_byte_splice_handles_crlf_and_partial_unicode_lines() {
    let source = "pré\r\n  αβ γ\r\nend";
    let replacement = plan(source, "β", "δ").unwrap();
    assert_eq!(replacement.line_range, 1..2);
    assert_eq!(replacement.apply(source), "pré\r\n  αδ γ\r\nend");
    let replacement = plan(source, "αβ γ\r\n", "δ\r\n").unwrap();
    assert_eq!(replacement.line_range, 1..2);
    assert_eq!(replacement.apply(source), "pré\r\n  δ\r\nend");
}

#[test]
fn joining_lines_includes_the_surviving_suffix_in_coverage() {
    let source = "prefix\nbeta\ngamma\ntail";
    let join = plan(source, "beta\n", "X").unwrap();
    assert_eq!(join.line_range, 1..3);
    assert_eq!(join.apply(source), "prefix\nXgamma\ntail");
    let deletion = plan(source, "beta\n", "").unwrap();
    assert_eq!(deletion.line_range, 1..2);
    assert_eq!(deletion.apply(source), "prefix\ngamma\ntail");
    let partial = plan(source, "eta\n", "").unwrap();
    assert_eq!(partial.line_range, 1..3);
    assert_eq!(partial.apply(source), "prefix\nbgamma\ntail");
}

#[tokio::test]
async fn literal_failure_refresh_corrected_edit_and_shared_quota() {
    use crate::config::constants::tools;
    use crate::tools::editing::PatchError;
    use crate::tools::registry::{ToolExecutionError, ToolRegistry};
    use serde_json::json;

    let dir = tempfile::tempdir().unwrap();
    let original = "# Guide\nCurrent text has  two spaces.\nUntouched footer\n";
    std::fs::write(dir.path().join("guide.md"), original).unwrap();
    let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
    registry.allow_all_tools().await.unwrap();
    let args = json!({"path":"guide.md", "old_str":"Current text has two spaces.", "new_str":"Corrected text."});
    // The public dispatch surface returns typed diagnostics without writing bytes.
    let output = registry.execute_tool(tools::EDIT_FILE, args.clone()).await.unwrap();
    assert_eq!(output["error"]["edit_failure"]["failure_kind"], "no_match", "{output}");
    assert_eq!(std::fs::read_to_string(dir.path().join("guide.md")).unwrap(), original);
    let failure: EditFailure = serde_json::from_value(output["error"]["edit_failure"].clone()).unwrap();
    assert!(failure.candidate_locations.is_empty());
    assert!(failure.next_read_args.is_none());
    assert_eq!(
        failure.path,
        vtcode_commons::canonicalize(dir.path().join("guide.md"))
            .unwrap()
            .to_string_lossy()
    );
    let search = registry
        .execute_tool(tools::CODE_SEARCH, *failure.next_search_args.unwrap())
        .await
        .unwrap();
    assert!(search.get("error").is_none(), "{search}");
    let read = json!({"path":"guide.md", "offset":2, "limit":1, "condense":false});
    assert!(registry.has_patch_recovery_read(tools::READ_FILE, &read));
    let fresh = registry.execute_tool(tools::READ_FILE, read.clone()).await.unwrap();
    assert!(fresh.to_string().contains("Current text has  two spaces."), "{fresh}");
    assert!(!registry.has_patch_recovery_read(tools::READ_FILE, &read));
    // Repeated identical input and then a patch mismatch do not refill the quota.
    let again = registry.execute_tool(tools::EDIT_FILE, args).await.unwrap();
    assert_eq!(again["error"]["edit_failure"]["failure_kind"], "no_match");
    let source = anyhow::Error::new(PatchError::ContextNotFound { path: "guide.md".into(), context: "absent".into() });
    let patch_error = ToolExecutionError::from_anyhow(tools::APPLY_PATCH, &source, 0, false, false, None);
    registry.grant_patch_recovery_read(&patch_error).await;
    assert!(!registry.has_patch_recovery_read(tools::READ_FILE, &read));
    let corrected = registry
        .execute_tool(
            tools::EDIT_FILE,
            json!({"path":"guide.md", "old_str":"Current text has  two spaces.", "new_str":"Corrected text."}),
        )
        .await
        .unwrap();
    assert!(corrected.get("error").is_none(), "{corrected}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("guide.md")).unwrap(),
        "# Guide\nCorrected text.\nUntouched footer\n"
    );

    // Existing serialized errors lacking the optional field remain readable.
    let typed = ToolExecutionError::from_anyhow(
        tools::EDIT_FILE,
        &plan(original, "absent", "new").unwrap_err().into(),
        0,
        true,
        false,
        None,
    )
    .with_tool_call_context(tools::EDIT_FILE, &json!({"path":"guide.md"}));
    assert!(!typed.retryable);
    assert!(!typed.partial_state_possible);
    let mut serialized = serde_json::to_value(&typed).unwrap();
    let restored: ToolExecutionError = serde_json::from_value(serialized.clone()).unwrap();
    assert_eq!(restored.edit_failure.unwrap().failure_kind, EditFailureKind::NoMatch);
    serialized.as_object_mut().unwrap().remove("edit_failure");
    let legacy: ToolExecutionError = serde_json::from_value::<ToolExecutionError>(serialized).unwrap();
    assert!(legacy.edit_failure.is_none());
    assert!(serde_json::to_value(legacy).unwrap().get("edit_failure").is_none());
}

#[cfg(unix)]
#[tokio::test]
async fn literal_alias_and_patch_share_one_canonical_read() {
    use crate::config::constants::tools;
    use crate::tools::registry::ToolRegistry;
    use serde_json::json;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("doc.md"), "same\nother\nsame\n").unwrap();
    std::os::unix::fs::symlink(dir.path().join("doc.md"), dir.path().join("alias.md")).unwrap();
    let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
    registry.allow_all_tools().await.unwrap();
    let args = json!({"path":"alias.md", "old_str":"same", "new_str":"bad"});
    let failure = registry.execute_tool(tools::EDIT_FILE, args.clone()).await.unwrap();
    let diagnostics = &failure["error"]["edit_failure"];
    assert_eq!(diagnostics["failure_kind"], "ambiguous_match", "{failure}");
    assert_eq!(
        diagnostics["candidate_locations"],
        json!([{"start_line":1,"end_line":1},{"start_line":3,"end_line":3}])
    );
    assert_eq!(std::fs::read_to_string(dir.path().join("doc.md")).unwrap(), "same\nother\nsame\n");
    let read = diagnostics["next_read_args"].clone();
    assert!(registry.consume_patch_recovery_read(tools::READ_FILE, &read));
    let again = registry.execute_tool(tools::EDIT_FILE, args).await.unwrap();
    assert_eq!(again["error"]["edit_failure"]["failure_kind"], "ambiguous_match");
    assert!(!registry.has_patch_recovery_read(tools::READ_FILE, &json!({"path":"doc.md","limit":200})));
    assert!(!registry.has_patch_recovery_read(tools::READ_FILE, &json!({"path":"alias.md","limit":200})));
}

#[tokio::test]
async fn literal_edit_preserves_external_conflict_before_match_failure() {
    use crate::config::constants::tools;
    use crate::tools::registry::ToolRegistry;
    use serde_json::json;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("doc.md");
    std::fs::write(&file, "original\n").unwrap();
    let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
    registry.allow_all_tools().await.unwrap();
    let read = json!({"path":"doc.md", "limit":1, "condense":false});
    registry.execute_tool(tools::READ_FILE, read.clone()).await.unwrap();
    std::fs::write(&file, "external owner change\n").unwrap();
    let conflict = registry
        .edit_file(json!({"path":"doc.md", "old_str":"original", "new_str":"agent change"}))
        .await
        .unwrap();
    assert_eq!(conflict["conflict_detected"], true, "{conflict}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "external owner change\n");
    assert!(!registry.has_patch_recovery_read(tools::READ_FILE, &read));
}

#[tokio::test]
async fn literal_noop_dispatch_reports_no_modified_paths() {
    use crate::config::constants::tools;
    use crate::tools::registry::ToolRegistry;
    use serde_json::json;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("doc.md");
    std::fs::write(&file, "same\r\n尾").unwrap();
    let registry = ToolRegistry::new(dir.path().to_path_buf()).await;
    registry.allow_all_tools().await.unwrap();
    let output = registry
        .execute_tool(tools::EDIT_FILE, json!({"path":"doc.md", "old_str":"same", "new_str":"same"}))
        .await
        .unwrap();
    assert!(output.get("error").is_none(), "{output}");
    assert_eq!(output["modified_files"], json!([]));
    assert_eq!(std::fs::read_to_string(file).unwrap(), "same\r\n尾");
}

#[test]
fn single_line_whitespace_match_keeps_its_selected_line_ending() {
    for (source, expected) in [
        ("prefix\r\n  target\r\nsuffix\r\n", "prefix\r\nfirst\r\nsecond\r\nsuffix\r\n"),
        ("prefix\r\n  target\nsuffix\r\n", "prefix\r\nfirst\nsecond\nsuffix\r\n"),
    ] {
        let replacement = plan(source, "target\t", "first\nsecond").unwrap();
        assert_eq!(replacement.match_kind, MatchKind::LineEdgeWhitespace);
        assert_eq!(replacement.line_range, 1..2);
        assert_eq!(replacement.apply(source), expected);
    }
}
