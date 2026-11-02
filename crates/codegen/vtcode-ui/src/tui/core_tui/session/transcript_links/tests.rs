use super::*;
use std::borrow::Cow;
use std::fs;
use std::path::PathBuf;

use crate::tui::core_tui::types::InlineTheme;

#[test]
fn path_candidate_shortcut_preserves_decoded_paths_and_locations() {
    struct Case {
        raw: &'static str,
        path: &'static str,
        location: Option<&'static str>,
    }
    let root = tempfile::tempdir().expect("link workspace");
    fs::create_dir(root.path().join("src")).unwrap();
    for path in ["src/main.rs", "Cargo.toml", "東京.rs", "Cargo"] {
        fs::write(root.path().join(path), "fixture").unwrap();
    }
    let cases = [
        Case {
            raw: "src%2Fmain%2Ers:12:3",
            path: "src/main.rs",
            location: Some(":12:3"),
        },
        Case {
            raw: "Cargo%2Etoml(5,2)",
            path: "Cargo.toml",
            location: Some(":5:2"),
        },
        Case {
            raw: "./Cargo.toml:7",
            path: "Cargo.toml",
            location: Some(":7"),
        },
        Case {
            raw: "東京.rs:4",
            path: "東京.rs",
            location: Some(":4"),
        },
        Case {
            raw: "src/main.rs",
            path: "src/main.rs",
            location: None,
        },
    ];
    for case in cases {
        let target = resolve_transcript_file_target(case.raw, Some(root.path())).expect("file link");
        assert_eq!(target.path(), root.path().join(case.path), "{}", case.raw);
        assert_eq!(target.location_suffix(), case.location, "{}", case.raw);
    }
    for word in ["ordinary", "3999:", "inspect", "or", "Cargo", "Cargo(5,2)"] {
        assert!(resolve_transcript_file_target(word, Some(root.path())).is_none(), "{word}");
    }
}

#[test]
fn path_candidate_shortcut_keeps_filesystem_checks_live() {
    let root = tempfile::tempdir().expect("link workspace");
    let path = root.path().join("appearing.rs");
    assert!(resolve_transcript_file_target("appearing.rs:9", Some(root.path())).is_none());
    fs::write(&path, "created after detection").unwrap();
    let target = resolve_transcript_file_target("appearing.rs:9", Some(root.path())).expect("new file");
    assert_eq!(target.path(), path);
    assert_eq!(target.location_suffix(), Some(":9"));
    fs::remove_file(&path).unwrap();
    assert!(resolve_transcript_file_target("appearing.rs:9", Some(root.path())).is_none());
    fs::create_dir(&path).unwrap();
    assert!(resolve_transcript_file_target("appearing.rs:9", Some(root.path())).is_none());
}

#[test]
fn filename_candidates_ignore_sentence_punctuation() {
    assert!(may_contain_link_candidate_text("see Cargo.toml."));
    assert!(may_contain_link_candidate_text("open test.rs, then continue"));
    assert!(!may_contain_link_candidate_text("the end."));
    assert!(!may_contain_link_candidate_text("plain prose without names"));
    assert!(may_contain_link_candidate_text("src/main.rs"));
}

/// Walk up from `CARGO_MANIFEST_DIR` to the directory whose `Cargo.toml`
/// contains `[workspace]`. Hardcoding `parent()` breaks when crates are
/// nested (e.g. `crates/codegen/<crate>` — three levels deep, not one).
fn find_workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut current = manifest.as_path();
    while let Some(parent) = current.parent() {
        let cargo_toml = parent.join("Cargo.toml");
        if cargo_toml.is_file() && fs::read_to_string(&cargo_toml).is_ok_and(|c| c.contains("[workspace]")) {
            return parent.to_path_buf();
        }
        current = parent;
    }
    panic!("could not find workspace root from CARGO_MANIFEST_DIR");
}

#[test]
fn plain_text_prefilter_skips_link_detection() {
    assert!(!may_contain_link_candidate_text("plain text only"));
    assert!(may_contain_link_candidate_text("see src/main.rs"));
    assert!(may_contain_link_candidate_text("https://example.com"));
}

#[test]
fn borrowed_decoration_keeps_plain_text_undecorated() {
    let mut session = Session::new(InlineTheme::default(), None, 12);
    let lines = vec![TranscriptLine {
        line: Line::from("plain text only"),
        explicit_links: Vec::new(),
    }];

    let decorated = session.decorate_borrowed_transcript_links(&lines, Rect::new(0, 0, 20, 2));

    assert_eq!(decorated, vec![Line::from("plain text only")]);
    assert!(session.transcript_file_link_targets.is_empty());
}

#[test]
fn borrowed_and_visible_decoration_agree_on_mixed_links() {
    use crate::tui::ui::tui::types::InlineLinkTarget;

    // Asymmetric rows: explicit link first, raw URL last, plain middle —
    // row index feeds target areas, so order matters for the oracle.
    let root = tempfile::tempdir().expect("link workspace");
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/app.rs"), "fixture").unwrap();
    fs::write(root.path().join("Cargo.toml"), "fixture").unwrap();
    let area = Rect::new(0, 0, 60, 5);
    let explicit_text = "see explicit docs";
    let explicit_target = "https://example.com/explicit";
    let explicit_start = explicit_text.find("explicit").expect("explicit offset");
    let lines = vec![
        TranscriptLine {
            line: Line::from(explicit_text),
            explicit_links: vec![RenderedTranscriptLink {
                start: explicit_start,
                end: explicit_start + "explicit".len(),
                start_col: explicit_start,
                width: "explicit".len(),
                target: InlineLinkTarget::Url(explicit_target.to_string()),
            }],
        },
        TranscriptLine {
            line: Line::from("plain text only"),
            explicit_links: Vec::new(),
        },
        TranscriptLine {
            line: Line::from("open src/app.rs and https://example.com/raw"),
            explicit_links: Vec::new(),
        },
    ];

    let mut borrowed_session = Session::new(InlineTheme::default(), None, 60);
    borrowed_session.set_workspace_root(Some(root.path().to_path_buf()));
    let borrowed = borrowed_session.decorate_borrowed_transcript_links(&lines, area);
    let borrowed_targets = borrowed_session.transcript_file_link_targets.clone();

    let mut owned_session = Session::new(InlineTheme::default(), None, 60);
    owned_session.set_workspace_root(Some(root.path().to_path_buf()));
    let owned = owned_session.decorate_visible_transcript_links(lines.clone(), area);
    let owned_targets = owned_session.transcript_file_link_targets.clone();

    assert_eq!(borrowed, owned);
    assert_eq!(borrowed_targets, owned_targets);
    // Independently derived: explicit URL + raw file + raw URL = 3 targets
    // across rows 0 and 2; plain row 1 contributes none.
    assert_eq!(borrowed_targets.len(), 3);
    assert_eq!(borrowed_targets[0].area.y, area.y);
    assert_eq!(borrowed_targets[1].area.y, area.y + 2);
    assert_eq!(borrowed_targets[2].area.y, area.y + 2);

    // Cached (no raw detection) fast path: explicit survives, raw links vanish.
    let mut cached_borrowed = Session::new(InlineTheme::default(), None, 60);
    cached_borrowed.set_workspace_root(Some(root.path().to_path_buf()));
    let cached_borrowed_lines = cached_borrowed.decorate_borrowed_cached_transcript_links(&lines, area);
    let mut cached_owned = Session::new(InlineTheme::default(), None, 60);
    cached_owned.set_workspace_root(Some(root.path().to_path_buf()));
    let cached_owned_lines = cached_owned.decorate_visible_cached_transcript_links(lines, area);
    assert_eq!(cached_borrowed_lines, cached_owned_lines);
    assert_eq!(cached_borrowed.transcript_file_link_targets, cached_owned.transcript_file_link_targets);
    assert_eq!(cached_borrowed.transcript_file_link_targets.len(), 1);
}

#[test]
fn transcript_line_text_borrows_single_span_content() {
    let line = Line::from("plain text only");

    let text = transcript_line_text(&line);

    assert!(matches!(text, Cow::Borrowed("plain text only")));
}

#[test]
fn transcript_line_text_allocates_once_for_multi_span_content() {
    let line = Line::from(vec![Span::raw("plain "), Span::raw("text only")]);

    let text = transcript_line_text(&line);

    assert!(matches!(text, Cow::Owned(ref content) if content == "plain text only"));
}

#[test]
fn project_detected_links_preserves_char_boundaries_for_unicode_wraps() {
    let original_text = "crates/codegen/vtcode-core/src/tool_policy.rs — those files";
    let wrapped_lines = vec![
        Line::from("crates/codegen/vtcode-core/src/tool_policy.rs —"),
        Line::from(" those files"),
    ];

    let projected =
        project_detected_links_onto_wrapped_lines(&wrapped_lines, original_text, Some(&find_workspace_root()));

    assert_eq!(projected.len(), 2);
    assert_eq!(projected[0].len(), 1);
    assert_eq!(projected[0][0].start, 0);
    assert!(projected[0][0].end <= "crates/codegen/vtcode-core/src/tool_policy.rs —".len());
    assert!("crates/codegen/vtcode-core/src/tool_policy.rs —".is_char_boundary(projected[0][0].end));
}

#[test]
fn projected_links_align_with_wrapped_bullet_rows() {
    let root = find_workspace_root();
    // Long bullet item: the real wrapper wraps it across several display
    // rows with a hanging indent and drops the break whitespace, so naive
    // byte-length accumulation drifts and the underline spills into
    // neighboring words. Links sit on different rows so accumulating
    // drift would corrupt the later one more.
    let original = "• Verified the implementation across all of the modules in Cargo.toml and then confirmed the behavior in Cargo.lock, and continued checking the remaining pieces afterwards";
    let wrapped = wrapping::wrap_line_preserving_urls(Line::from(original), 60);
    assert!(wrapped.len() > 2, "expected multiple wrapped rows, got {}", wrapped.len());

    let projected = project_detected_links_onto_wrapped_lines(&wrapped, original, Some(&root));
    assert_eq!(projected.len(), wrapped.len());

    let mut found = Vec::new();
    for (row, links) in wrapped.iter().zip(projected.iter()) {
        let row_text = transcript_line_text(row);
        for link in links {
            let slice = &row_text.as_ref()[link.start..link.end];
            assert!(
                slice == "Cargo.toml" || slice == "Cargo.lock",
                "link underline spilled outside the path: {slice:?} in {row_text:?}"
            );
            assert_eq!(link.start_col, UnicodeWidthStr::width(&row_text.as_ref()[..link.start]));
            assert_eq!(link.width, UnicodeWidthStr::width(slice));
            found.push(slice.to_string());
        }
    }
    found.sort();
    assert_eq!(found, vec!["Cargo.lock".to_string(), "Cargo.toml".to_string()]);
}
