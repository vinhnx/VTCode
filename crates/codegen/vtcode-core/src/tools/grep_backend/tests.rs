use super::*;

use std::fs;

use tempfile::tempdir;

#[tokio::test]
async fn literal_backend_leading_dash_paths_and_empty_matches() {
    let workspace = tempdir().expect("tempdir");
    fs::write(workspace.path().join("-guide.md"), "- [Sessions](#sessions)\nplain\n").expect("write guide");

    let found = search_literal_bounded("- [Sessions](#sessions)", workspace.path(), &[], 5)
        .await
        .expect("search runs");
    assert_eq!(found.candidates.len(), 1);
    assert_eq!(found.candidates[0].line, 1);
    assert_eq!(found.candidates[0].matched_text, "- [Sessions](#sessions)");
    assert!(!found.truncated);

    let empty = search_literal_bounded("absent", workspace.path(), &[], 5)
        .await
        .expect("search runs");
    assert!(empty.candidates.is_empty());
    assert!(!empty.truncated);

    let error = search_literal_bounded("plain", &workspace.path().join("missing"), &[], 5)
        .await
        .expect_err("a missing path must fail");
    assert!(error.to_string().contains("does not exist"), "{error}");
}

#[tokio::test]
async fn literal_backend_alternation_matches_any_term() {
    let workspace = tempdir().expect("tempdir");
    fs::write(workspace.path().join("a.txt"), "tokio\nasync-std\nruntime\n").expect("write");

    let found = search_literal_bounded("tokio|async-std", workspace.path(), &[], 5)
        .await
        .expect("search runs");

    assert_eq!(found.candidates.len(), 2, "both alternation terms match");
    assert!(found.candidates.iter().all(|candidate| candidate.matched_text != "runtime"));
}

#[tokio::test]
async fn literal_backend_respects_language_globs() {
    let workspace = tempdir().expect("tempdir");
    fs::write(workspace.path().join("a.rs"), "needle\n").expect("write rust");
    fs::write(workspace.path().join("b.txt"), "needle\n").expect("write text");

    let found = search_literal_bounded("needle", workspace.path(), &[CodeLanguage::Rust], 5)
        .await
        .expect("search runs");

    assert_eq!(found.candidates.len(), 1, "only the Rust file matches the language filter");
    assert!(found.candidates[0].path.ends_with("a.rs"));
}

#[tokio::test]
async fn literal_backend_reports_truncation_at_the_cap() {
    let workspace = tempdir().expect("tempdir");
    fs::write(workspace.path().join("a.txt"), "needle\nneedle\nneedle\n").expect("write");

    let found = search_literal_bounded("needle", workspace.path(), &[], 2)
        .await
        .expect("search runs");

    assert!(found.truncated, "reaching the cap marks the result truncated");
    assert_eq!(found.candidates.len(), 2);
}

#[tokio::test]
async fn search_backend_emits_begin_match_end_events() {
    let workspace = tempdir().expect("tempdir");
    fs::write(workspace.path().join("a.rs"), "fn main() {}\nlet needle = 1;\n").expect("write");

    let input = GrepSearchInput::with_defaults("needle".to_string(), workspace.path().to_string_lossy().into_owned());
    let events = run_search_backend(&input, None).await.expect("search runs");

    let types = events
        .iter()
        .filter_map(|event| event.get("type").and_then(Value::as_str))
        .collect::<Vec<_>>();
    assert_eq!(types, ["begin", "match", "end"], "the event stream keeps the begin/match/end shape");

    let match_event = events
        .iter()
        .find(|event| event.get("type").and_then(Value::as_str) == Some("match"))
        .expect("a match event");
    assert_eq!(match_event["data"]["line_number"], json!(2));
    assert_eq!(match_event["data"]["submatches"][0]["match"]["text"], json!("needle"));
}

#[tokio::test]
async fn search_backend_reports_ripgrep_shaped_offsets() {
    let workspace = tempdir().expect("tempdir");
    // "first line\n" is 11 bytes, so the second line starts at offset 11, and
    // "needle" starts 7 bytes into that line.
    fs::write(workspace.path().join("a.rs"), "first line\nsecond needle line\n").expect("write");

    let input = GrepSearchInput::with_defaults("needle".to_string(), workspace.path().to_string_lossy().into_owned());
    let events = run_search_backend(&input, None).await.expect("search runs");

    let match_event = events
        .iter()
        .find(|event| event.get("type").and_then(Value::as_str) == Some("match"))
        .expect("a match event");
    let data = &match_event["data"];
    assert_eq!(data["line_number"], json!(2));
    assert_eq!(data["absolute_offset"], json!(11), "the offset of the start of the line");
    assert_eq!(data["submatches"][0]["start"], json!(7), "the offset inside the line");
    assert_eq!(data["submatches"][0]["end"], json!(13));

    // The submatch offsets must index into the reported line text, which is
    // what `rg --json` guarantees.
    let lines = data["lines"]["text"].as_str().expect("line text");
    let start = data["submatches"][0]["start"].as_u64().expect("start") as usize;
    let end = data["submatches"][0]["end"].as_u64().expect("end") as usize;
    assert_eq!(lines.get(start..end), Some("needle"));
}

#[tokio::test]
async fn search_backend_fails_on_a_missing_path() {
    let input = GrepSearchInput::with_defaults("needle".to_string(), "/nonexistent/path".to_string());
    let error = run_search_backend(&input, None).await.expect_err("a missing path must fail");

    assert!(error.to_string().contains("path does not exist"), "{error}");
}

#[tokio::test]
async fn search_backend_honours_the_result_cap() {
    let workspace = tempdir().expect("tempdir");
    fs::write(workspace.path().join("a.txt"), "needle\nneedle\nneedle\n").expect("write");

    let mut input =
        GrepSearchInput::with_defaults("needle".to_string(), workspace.path().to_string_lossy().into_owned());
    input.max_results = Some(1);
    let events = run_search_backend(&input, None).await.expect("search runs");

    let matches = events
        .iter()
        .filter(|event| event.get("type").and_then(Value::as_str) == Some("match"))
        .count();
    assert_eq!(matches, 1, "the cap bounds the reported matches");
}

#[test]
fn split_alternation_handles_empty_and_single_queries() {
    assert!(split_alternation("").is_empty());
    assert!(split_alternation("   ").is_empty());
    assert_eq!(split_alternation("tokio"), vec!["tokio".to_string()]);
    assert_eq!(split_alternation("tokio|async-std"), vec!["tokio".to_string(), "async-std".to_string()]);
    assert_eq!(
        split_alternation("tokio||async-std"),
        vec!["tokio".to_string(), "async-std".to_string()],
        "empty alternation terms are dropped"
    );
}
