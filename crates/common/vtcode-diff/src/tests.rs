use super::*;
use std::time::{Duration, Instant};

#[test]
fn asymmetric_replacement_has_correct_line_numbers() {
    let document = DiffDocument::between("a\nb\nc\n", "a\nx\ny\nc\n", DiffOptions::default());
    let lines: Vec<_> = document.hunks.iter().flat_map(|hunk| &hunk.lines).collect();
    assert!(
        lines
            .iter()
            .any(|line| { line.kind == DiffLineKind::Deletion && line.old_line == Some(2) && line.new_line.is_none() })
    );
    assert!(
        lines
            .iter()
            .any(|line| { line.kind == DiffLineKind::Addition && line.old_line.is_none() && line.new_line == Some(3) })
    );
    assert_eq!(document.stats.additions, 2);
    assert_eq!(document.stats.deletions, 1);
}

#[test]
fn repeated_lines_keep_the_changed_anchor() {
    let document = DiffDocument::between("same\nold\nsame\n", "same\nnew\nsame\n", DiffOptions::default());
    assert_eq!(document.stats.additions, 1);
    assert_eq!(document.stats.deletions, 1);
    assert_eq!(document.hunks[0].old_start, 1);
}

#[test]
fn zero_context_hunks_preserve_empty_side_anchors() {
    let options = DiffOptions { context_lines: 0, ..DiffOptions::default() };
    let insertion = DiffDocument::between("a\nc\n", "a\nb\nc\n", options.clone());
    assert_eq!(insertion.hunks[0].old_start, 2);
    assert_eq!(insertion.hunks[0].new_start, 2);

    let deletion = DiffDocument::between("a\nb\nc\n", "a\nc\n", options);
    assert_eq!(deletion.hunks[0].old_start, 2);
    assert_eq!(deletion.hunks[0].new_start, 2);
}

#[test]
fn preserves_crlf_cr_and_missing_final_newline() {
    let crlf = DiffDocument::between("a\r\nb\r\n", "a\r\nx\r\n", DiffOptions::default());
    assert!(crlf.hunks[0].lines.iter().any(|line| line.text == "a\r\n"));
    let cr = DiffDocument::between("a\rb\r", "a\rx\r", DiffOptions::default());
    assert!(cr.hunks[0].lines.iter().any(|line| line.text == "a\r"));
    let eof = DiffDocument::between("a\n", "a", DiffOptions::default());
    assert_eq!(eof.stats.additions, 1);
    assert_eq!(eof.stats.deletions, 1);
}

#[test]
fn parser_rejects_body_before_hunk() {
    let error = DiffDocument::from_unified("-old\n+new\n").expect_err("body must need a hunk");
    assert_eq!(error.to_string(), "diff body appears before a hunk header");
}

#[test]
fn parser_accepts_standard_git_metadata_before_hunk() {
    let document = DiffDocument::from_unified(
        "diff --git a/file.txt b/file.txt\nindex 1111111..2222222 100644\n--- a/file.txt\n+++ b/file.txt\n@@ -1 +1 @@\n-old\n+new\n",
    )
    .expect("standard git metadata is not a body");
    assert_eq!(document.stats.deletions, 1);
    assert_eq!(document.stats.additions, 1);
}

#[test]
fn parser_accepts_metadata_between_multiple_file_hunks() {
    let document = DiffDocument::from_unified(
        "diff --git a/one.txt b/one.txt\nnew file mode 100644\n--- /dev/null\n+++ b/one.txt\n@@ -0,0 +1 @@\n+one\ndiff --git a/two.txt b/two.txt\nnew file mode 100644\n--- /dev/null\n+++ b/two.txt\n@@ -0,0 +1 @@\n+two\n",
    )
    .expect("metadata between files is not a hunk body");
    assert_eq!(document.hunks.len(), 2);
    assert_eq!(document.stats.additions, 2);
}

#[test]
fn parsed_body_lines_preserve_original_terminators() {
    let document = DiffDocument::from_unified("@@ -1 +1 @@\r\n-old\r\n+new\r\n").expect("valid CRLF diff");
    assert_eq!(document.hunks[0].lines[0].text, "old\r\n");
    assert_eq!(document.hunks[0].lines[1].text, "new\r\n");

    let document = DiffDocument::from_unified("@@ -1 +1 @@\r-old\r+new\r").expect("valid CR diff");
    assert_eq!(document.hunks[0].lines[0].text, "old\r");
    assert_eq!(document.hunks[0].lines[1].text, "new\r");
}

#[test]
fn parser_rejects_incomplete_hunk_body() {
    let error = DiffDocument::from_unified("@@ -1,2 +1,2 @@\n-old\n+new\n")
        .expect_err("hunk body must satisfy the declared ranges");
    assert_eq!(error.to_string(), "hunk line counts do not match header");
}

#[test]
fn parser_rejects_unknown_backslash_metadata_inside_hunk() {
    let error = DiffDocument::from_unified("@@ -1 +1 @@\n-old\n\\ unexpected marker\n+new\n")
        .expect_err("only Git's no-newline marker is valid inside a hunk");
    assert_eq!(error.to_string(), "invalid unified diff body line");
}

#[test]
fn parser_tracks_asymmetric_hunk_numbers() {
    let document = DiffDocument::from_unified("@@ -4,1 +8,2 @@\n-old\n+new\n+extra\n").expect("valid diff");
    assert_eq!(document.hunks[0].old_start, 4);
    assert_eq!(document.hunks[0].new_start, 8);
    assert_eq!(document.hunks[0].lines[2].new_line, Some(9));
}

#[test]
fn parser_keeps_context_lines_that_look_like_omission_markers() {
    let document = DiffDocument::from_unified("@@ -1 +1 @@\n ... 4 lines omitted ...\n").expect("valid context line");
    assert_eq!(document.hunks[0].lines[0].kind, DiffLineKind::Context);
    assert_eq!(document.stats.omitted_rows, 0);
}

#[test]
fn parsed_omission_advances_both_line_counters() {
    let lines = display_lines_from_unified_diff("@@ -10,6 +20,6 @@\n same\n... 4 lines omitted ...\n-old\n+new\n");
    let deletion = lines
        .iter()
        .find(|line| line.kind == DiffDisplayKind::Deletion)
        .expect("deletion after omission");
    let addition = lines
        .iter()
        .find(|line| line.kind == DiffDisplayKind::Addition)
        .expect("addition after omission");
    assert_eq!(deletion.old_line, Some(15));
    assert_eq!(addition.new_line, Some(25));
}

#[test]
fn truncated_hunk_keeps_tail_additions_as_diff_lines() {
    let mut input = String::from("@@ -1,201 +1,201 @@\n");
    for index in 0..95 {
        input.push_str(&format!("-old-{index}\n"));
    }
    input.push_str("... 243 lines omitted ...\n");
    for index in 137..201 {
        input.push_str(&format!("+new-{index}\n"));
    }

    let lines = display_lines_from_unified_diff(&input);
    let additions: Vec<_> = lines.iter().filter(|line| line.kind == DiffDisplayKind::Addition).collect();
    assert_eq!(additions.len(), 64);
    assert_eq!(additions.last().map(|line| line.text.as_str()), Some("new-200"));
}

#[test]
fn truncated_hunk_numbers_tail_from_declared_end() {
    let mut input = String::from("@@ -1,201 +1,201 @@\n");
    for index in 0..93 {
        input.push_str(&format!("-old-{index}\n"));
    }
    input.push_str("... 277 lines omitted ...\n");
    for index in 169..201 {
        input.push_str(&format!("+new-{index}\n"));
    }

    let lines = display_lines_from_unified_diff(&input);
    let additions: Vec<_> = lines.iter().filter(|line| line.kind == DiffDisplayKind::Addition).collect();
    assert_eq!(additions.first().and_then(|line| line.new_line), Some(170));
    assert_eq!(additions.last().and_then(|line| line.new_line), Some(201));
}

#[test]
fn parsed_metadata_stays_outside_hunk_semantics() {
    let display = display_lines_from_unified_diff("--- a/file\n+++ b/file\n@@ -1 +1 @@\n-old\n+new\n");
    let rows = layout_display_lines(&display, LayoutOptions::default());
    assert_eq!(rows[0].kind, DiffRowKind::Metadata);
    assert_eq!(rows[1].kind, DiffRowKind::Metadata);
    assert_eq!(rows[2].kind, DiffRowKind::HunkHeader);
    assert_eq!(rows[2].hunk_index, Some(0));
}

#[test]
fn unified_parser_accepts_omission_and_advances_numbers() {
    let document = DiffDocument::from_unified("@@ -10,6 +20,6 @@\n same\n... 4 lines omitted ...\n-old\n+new\n")
        .expect("omission marker is valid bounded preview metadata");
    assert_eq!(document.hunks[0].lines[1].old_line, Some(15));
    assert_eq!(document.hunks[0].lines[2].new_line, Some(25));
    assert_eq!(document.hunks[0].old_lines, 6);
    assert_eq!(document.hunks[0].new_lines, 6);
    assert_eq!(document.stats.omitted_rows, 4);
}

#[test]
fn unified_parser_accepts_asymmetric_bounded_omission() {
    let document =
        DiffDocument::from_unified("@@ -1,4 +1,8 @@\n-old-1\n... 4 lines omitted ...\n+new-6\n+new-7\n+new-8\n")
            .expect("bounded omission may hide different old/new line counts");
    assert_eq!(document.stats.omitted_rows, 4);
    assert_eq!(document.hunks[0].old_lines, 4);
    assert_eq!(document.hunks[0].new_lines, 6);
}

#[test]
fn unified_parser_numbers_one_sided_omitted_tails_from_hunk_end() {
    let added = DiffDocument::from_unified("@@ -0,0 +1,5 @@\n+one\n... 3 lines omitted ...\n+five\n")
        .expect("bounded addition is valid");
    assert_eq!(added.hunks[0].lines[1].new_line, Some(5));

    let deleted = DiffDocument::from_unified("@@ -1,5 +0,0 @@\n-one\n... 3 lines omitted ...\n-five\n")
        .expect("bounded deletion is valid");
    assert_eq!(deleted.hunks[0].lines[1].old_line, Some(5));
}

#[test]
fn intraline_ranges_are_utf8_boundaries() {
    let (old, new) = word_level_changed_ranges("café rouge", "café bleu");
    for (start, end) in old {
        assert!("café rouge".is_char_boundary(start));
        assert!("café rouge".is_char_boundary(end));
    }
    for (start, end) in new {
        assert!("café bleu".is_char_boundary(start));
        assert!("café bleu".is_char_boundary(end));
    }
}

#[test]
fn unicode_width_wrap_keeps_numbers_only_on_first_row() {
    let document = DiffDocument::between("", "界界界a\n", DiffOptions::default());
    let rows = document.layout(LayoutOptions { width: 12, ..LayoutOptions::default() });
    let additions: Vec<_> = rows.iter().filter(|row| row.kind == DiffRowKind::Addition).collect();
    assert!(additions.len() >= 2);
    assert_eq!(additions[0].left.as_ref().and_then(|cell| cell.new_line), Some(1));
    assert!(additions[1].continuation);
    assert_eq!(additions[1].left.as_ref().and_then(|cell| cell.new_line), None);
    assert_eq!(additions[1].marker, '+');
}

#[test]
fn layout_ignores_invalid_intraline_boundaries() {
    let display = [DiffDisplayLine {
        kind: DiffDisplayKind::Addition,
        old_line: None,
        new_line: Some(1),
        text: "café".to_owned(),
        changed: vec![(2, 4)],
    }];
    let rows = layout_display_lines(&display, LayoutOptions { width: 20, ..LayoutOptions::default() });
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].left.as_ref().expect("cell").segments[0].text, "café");
}

#[test]
fn narrow_side_by_side_falls_back_to_unified() {
    let document = DiffDocument::between("old\n", "new\n", DiffOptions::default());
    let rows = document.layout(LayoutOptions {
        layout: DiffLayout::SideBySide,
        width: 40,
        ..LayoutOptions::default()
    });
    assert!(
        rows.iter()
            .filter(|row| row.kind != DiffRowKind::HunkHeader)
            .all(|row| row.right.is_none())
    );
}

#[test]
fn responsive_gutter_policy_preserves_source_room_at_the_boundary() {
    assert_eq!(diff_gutter_width(5), 9);
    assert!(diff_gutter_fits(29, 5));
    assert!(!diff_gutter_fits(28, 5));
    assert_eq!(diff_layout_width(29, 5, true), 29);
    assert_eq!(diff_layout_width(28, 5, false), 37);
}

#[test]
fn side_by_side_policy_has_a_stable_resize_boundary() {
    assert!(!diff_side_by_side_fits(Some(59)));
    assert!(diff_side_by_side_fits(Some(DIFF_MIN_SIDE_BY_SIDE_WIDTH)));
    assert!(diff_side_by_side_fits(None));
}

#[test]
fn side_by_side_wrapping_leaves_shorter_side_empty() {
    let document = DiffDocument::between("short\n", "this is a much longer replacement line\n", DiffOptions::default());
    let rows = document.layout(LayoutOptions {
        layout: DiffLayout::SideBySide,
        width: 80,
        ..LayoutOptions::default()
    });
    let continuation = rows.iter().find(|row| row.continuation).expect("long replacement should wrap");
    assert!(continuation.left.is_none());
    assert!(continuation.right.is_some());
}

#[test]
fn bounded_rows_keep_head_tail_and_exact_omission() {
    let old = (0..20).map(|index| format!("old-{index}\n")).collect::<String>();
    let new = (0..20).map(|index| format!("new-{index}\n")).collect::<String>();
    let rows = DiffDocument::between(&old, &new, DiffOptions::default())
        .layout(LayoutOptions { max_rows: 5, ..LayoutOptions::default() });
    assert_eq!(rows.len(), 5);
    let omission = rows.iter().find(|row| row.kind == DiffRowKind::Omission).expect("omission row");
    let text = &omission.left.as_ref().expect("cell").segments[0].text;
    assert!(text.ends_with(" rows omitted"));
    assert!(rows.last().and_then(|row| row.left.as_ref()).is_some());
}

#[test]
fn bounded_display_lines_keep_asymmetric_head_and_tail() {
    let lines =
        display_lines_from_unified_diff("@@ -1,4 +1,4 @@\n-old-head\n+new-head\n context\n-old-tail\n+new-tail\n");
    let bounded = bounded_display_lines(&lines, 4);

    assert_eq!(bounded.len(), 4);
    assert_eq!(bounded[0].text, "@@ -1,4 +1,4 @@");
    assert_eq!(bounded[1].text, "old-head");
    assert_eq!(bounded[2].kind, DiffDisplayKind::Metadata);
    assert!(bounded[2].text.contains("lines omitted"));
    assert_eq!(bounded[3].text, "new-tail");
}

#[test]
fn display_lines_preserve_hunk_range_counts() {
    // Regression: `@@ -65,19 +64,0 @@` must not collapse to
    // `@@ -65 +64 @@`, which reads as a one-line change.
    let lines = display_lines_from_unified_diff("@@ -65,19 +64,0 @@\n-old\n");
    assert_eq!(lines[0].kind, DiffDisplayKind::HunkHeader);
    assert_eq!(lines[0].text, "@@ -65,19 +64,0 @@");
}

#[test]
fn hunk_header_formatter_elides_single_counts_and_keeps_ranges() {
    assert_eq!(format_hunk_header(1, 1, 1, 1), "@@ -1 +1 @@");
    assert_eq!(format_hunk_header(65, 19, 65, 0), "@@ -65,19 +64,0 @@");
    assert_eq!(format_hunk_header(0, 0, 1, 5), "@@ -0,0 +1,5 @@");
}

#[test]
fn display_lines_from_hunks_keep_range_counts() {
    let document = DiffDocument::between("a\nb\nc\n", "a\nx\ny\nc\n", DiffOptions::default());
    let lines = display_lines_from_hunks(&document.hunks);
    let hunk = &document.hunks[0];
    assert_eq!(lines[0].text, format_hunk_header(hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines));
    assert!(lines[0].text.contains(','), "hunk header must keep range counts: {:?}", lines[0].text);
}

#[test]
fn plain_unified_formatter_preserves_labels_and_newline_hints() {
    let output = format_unified_diff(
        "old\n",
        "new",
        DiffOptions {
            old_label: Some("a/file.txt"),
            new_label: Some("b/file.txt"),
            ..DiffOptions::default()
        },
    );

    assert!(output.starts_with("--- a/file.txt\n+++ b/file.txt\n@@"));
    assert!(output.contains("@@ -1 +1 @@"));
    assert!(output.contains("-old\n"));
    assert!(output.contains("+new\n\\ No newline at end of file\n"));
    assert!(!output.contains('\r'));
}

#[test]
fn character_chunks_coalesce_and_reconstruct_both_sides() {
    let chunks = compute_diff_chunks("abc", "axc");
    assert_eq!(chunks.len(), 4);
    let old = chunks
        .iter()
        .filter_map(|chunk| match chunk {
            Chunk::Equal(text) | Chunk::Delete(text) => Some(*text),
            Chunk::Insert(_) => None,
        })
        .collect::<String>();
    let new = chunks
        .iter()
        .filter_map(|chunk| match chunk {
            Chunk::Equal(text) | Chunk::Insert(text) => Some(*text),
            Chunk::Delete(_) => None,
        })
        .collect::<String>();
    assert_eq!(old, "abc");
    assert_eq!(new, "axc");
}

#[test]
fn disjoint_large_input_respects_small_timeout_and_remains_readable() {
    let old = (0..10_000).map(|index| format!("old-{index}\n")).collect::<String>();
    let new = (0..10_000).rev().map(|index| format!("new-{index}\n")).collect::<String>();
    let started = Instant::now();
    let document = DiffDocument::between(
        &old,
        &new,
        DiffOptions {
            timeout: Duration::from_millis(5),
            ..DiffOptions::default()
        },
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(document.stats.additions > 0);
    assert!(document.stats.deletions > 0);
}

#[test]
fn crlf_line_endings_are_preserved() {
    let document = DiffDocument::between("a\r\nb\r\n", "a\r\nc\r\n", DiffOptions::default());
    assert_eq!(document.hunks.len(), 1);
    let texts: Vec<&str> = document.hunks[0].lines.iter().map(|line| line.text.as_str()).collect();
    assert!(texts.contains(&"a\r\n"));
    assert!(texts.contains(&"b\r\n"));
    assert!(texts.contains(&"c\r\n"));
}

#[test]
fn missing_final_newline_emits_hint() {
    let document = DiffDocument::between("a\nb", "a\nb\n", DiffOptions::default());
    let formatted = format_unified_hunks(&document.hunks, &DiffOptions::default());
    assert!(formatted.contains("\\ No newline at end of file"));
}

#[test]
fn zero_context_insert_hunk_header_is_git_compatible() {
    let options = DiffOptions { context_lines: 0, ..DiffOptions::default() };
    let document = DiffDocument::between("", "x\ny\n", options.clone());
    let formatted = format_unified_hunks(&document.hunks, &options);
    assert!(formatted.contains("@@ -0,0 +1,2 @@"));
}

#[test]
fn binary_content_skips_intraline_annotation() {
    let binary_old = "data\u{0}one\nshared\n";
    let binary_new = "data\u{0}two\nshared\n";
    let lines = display_lines_from_hunks(&DiffDocument::between(binary_old, binary_new, DiffOptions::default()).hunks);
    let annotated: Vec<_> = lines.iter().filter(|line| !line.changed.is_empty()).collect();
    assert!(annotated.is_empty(), "binary lines must not receive intraline ranges");

    // Sanity: text content still gets intraline ranges.
    let text_lines =
        display_lines_from_hunks(&DiffDocument::between("alpha beta\n", "alpha gamma\n", DiffOptions::default()).hunks);
    assert!(text_lines.iter().any(|line| !line.changed.is_empty()));
}

#[cfg(feature = "ansi")]
#[test]
fn ansi_adapter_can_render_without_color() {
    let document = DiffDocument::between("old\n", "new\n", DiffOptions::default());
    let rows = document.layout(LayoutOptions::default());
    let rendered = render_ansi_rows(&rows, AnsiDiffPalette::default(), false);
    assert!(rendered.iter().any(|line| line.starts_with("- old")));
    assert!(rendered.iter().any(|line| line.starts_with("+ new")));
    assert!(rendered.iter().all(|line| !line.contains('\u{1b}')));
}

#[cfg(feature = "ratatui")]
#[test]
fn ratatui_adapter_preserves_semantic_markers() {
    let document = DiffDocument::between("old\n", "new\n", DiffOptions::default());
    let rows = document.layout(LayoutOptions::default());
    let lines = to_ratatui_lines(&rows);
    let rendered = lines.iter().map(ToString::to_string).collect::<Vec<_>>();
    assert!(rendered.iter().any(|line| line.starts_with("- old")));
    assert!(rendered.iter().any(|line| line.starts_with("+ new")));
}

#[test]
fn generative_small_docs_round_trip_across_algorithms_and_unified() {
    // Matklad-style oracle fuzzing in the small: a humble deterministic
    // PRNG is enough to shake out tricky interactions. Generate tiny docs
    // from a swarmed line alphabet (small, overlapping inputs beat huge
    // uniform ones) and cross-check Myers vs Patience vs Histogram plus a
    // unified format/parse round-trip.
    const LINE_ALPHABET: [&str; 6] = ["a\n", "b\n", "c\n", "alpha\n", "beta\n", "x\n"];
    let mut rng = SwarmRng::new(0x9E37_79B9_7F4A_7C15);
    // Re-use buffers across iterations (static allocation in the small).
    let mut alphabet: Vec<&str> = Vec::with_capacity(LINE_ALPHABET.len());
    let mut old_text = String::with_capacity(64);
    let mut new_text = String::with_capacity(64);

    // Hand-picked asymmetric boundaries first: order swaps and empty sides
    // catch what symmetric fixtures miss.
    let seeds: [(&str, &str); 5] = [
        ("a\n", "b\n"),
        ("a\nb\n", "b\na\n"),
        ("", "x\n"),
        ("x\n", ""),
        ("a\na\n", "a\n"),
    ];
    for (old, new) in seeds {
        assert_generative_oracles(old, new);
    }

    for _ in 0..1024 {
        alphabet.clear();
        alphabet.extend(LINE_ALPHABET);
        rng.shuffle_str(&mut alphabet);
        let alphabet_len = rng.range(1, alphabet.len() + 1);
        alphabet.truncate(alphabet_len);

        gen_doc(&mut rng, &alphabet, &mut old_text);
        gen_doc(&mut rng, &alphabet, &mut new_text);
        assert_generative_oracles(&old_text, &new_text);
    }
}

fn assert_generative_oracles(old: &str, new: &str) {
    let options_for = |algorithm: DiffAlgorithm| DiffOptions {
        context_lines: 100,
        algorithm,
        ..DiffOptions::default()
    };
    let myers = DiffDocument::between(old, new, options_for(DiffAlgorithm::Myers));
    let patience = DiffDocument::between(old, new, options_for(DiffAlgorithm::Patience));
    let histogram = DiffDocument::between(old, new, options_for(DiffAlgorithm::Histogram));

    if old == new {
        assert!(myers.hunks.is_empty(), "identical docs must yield no hunks: {old:?}");
        assert!(patience.hunks.is_empty(), "identical docs must yield no hunks: {old:?}");
        assert!(histogram.hunks.is_empty(), "identical docs must yield no hunks: {old:?}");
        return;
    }

    // Oracle 1: applying hunks to the old side must reconstruct both sides.
    let (myers_old, myers_new) = apply_hunks(&myers.hunks);
    assert_eq!(myers_old, old, "Myers hunks do not reconstruct old side for {old:?} -> {new:?}");
    assert_eq!(myers_new, new, "Myers hunks do not reconstruct new side for {old:?} -> {new:?}");

    // Oracle 2 (`regex` vs `regex_lite`): all algorithms must agree on the
    // applied result even when hunk splitting differs.
    let (patience_old, patience_new) = apply_hunks(&patience.hunks);
    let (histogram_old, histogram_new) = apply_hunks(&histogram.hunks);
    assert_eq!(
        (patience_old.as_str(), patience_new.as_str()),
        (old, new),
        "Patience mis-reconstructs {old:?} -> {new:?}"
    );
    assert_eq!(
        (histogram_old.as_str(), histogram_new.as_str()),
        (old, new),
        "Histogram mis-reconstructs {old:?} -> {new:?}"
    );

    // Oracle 3: unified format/parse round-trip preserves the applied result.
    let formatted = format_unified_hunks(&myers.hunks, &options_for(DiffAlgorithm::Myers));
    let reparsed = DiffDocument::from_unified(&formatted).expect("formatted hunks must parse");
    let (re_old, re_new) = apply_hunks(&reparsed.hunks);
    assert_eq!((re_old.as_str(), re_new.as_str()), (old, new), "unified round-trip diverges for {old:?} -> {new:?}");
    assert_eq!(
        (reparsed.stats.additions, reparsed.stats.deletions),
        (myers.stats.additions, myers.stats.deletions),
        "unified round-trip changed change counts for {old:?} -> {new:?}"
    );
}

fn apply_hunks(hunks: &[DiffHunk]) -> (String, String) {
    let mut old = String::new();
    let mut new = String::new();
    for line in hunks.iter().flat_map(|hunk| &hunk.lines) {
        match line.kind {
            DiffLineKind::Context => {
                old.push_str(&line.text);
                new.push_str(&line.text);
            }
            DiffLineKind::Deletion => old.push_str(&line.text),
            DiffLineKind::Addition => new.push_str(&line.text),
        }
    }
    (old, new)
}

fn gen_doc(rng: &mut SwarmRng, alphabet: &[&str], result: &mut String) {
    result.clear();
    let count = rng.range(0, 8);
    for _ in 0..count {
        let line = alphabet[rng.range(0, alphabet.len())];
        result.push_str(line);
    }
}

/// Preserves the full-width reference sequence for ranges shared by 32- and 64-bit targets.
#[test]
fn swarm_rng_range_uses_full_width_samples() {
    let mut rng = SwarmRng::new(0);
    // Keep the 64-bit sequence on narrower targets too: truncating the
    // first sample to 32 bits before taking the remainder would yield 3.
    for expected in [5, 4, 5, 7] {
        assert_eq!(rng.range(3, 10), expected);
    }
}

/// Keeps samples inside half-open ranges, including one-element and maximum-width bounds.
#[test]
fn swarm_rng_range_handles_usize_boundaries() {
    let mut rng = SwarmRng::new(0);
    for (low, high) in [
        (0, 1),
        (usize::MAX - 1, usize::MAX),
        (0, usize::MAX),
        (usize::MAX - 7, usize::MAX),
    ] {
        for _ in 0..128 {
            assert!((low..high).contains(&rng.range(low, high)));
        }
    }
}

/// Minimal deterministic PRNG (splitmix64): no new dependencies, stable CI.
struct SwarmRng {
    state: u64,
}

impl SwarmRng {
    /// Initializes the deterministic generator from an explicit test seed.
    const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Advances SplitMix64 and returns the next full-width sample.
    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut value = self.state;
        value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        value ^ (value >> 31)
    }

    /// Samples `[low, high)` using the full 64-bit sample before narrowing,
    /// keeping the sequence identical for ranges shared by 32- and 64-bit targets.
    ///
    /// # Panics
    /// Panics if `low >= high`.
    fn range(&mut self, low: usize, high: usize) -> usize {
        assert!(low < high, "empty range");
        let span = u64::try_from(high - low).expect("usize range width fits in u64");
        // Reduce before narrowing so the remainder fits in usize on every target.
        let offset = usize::try_from(self.next_u64() % span).expect("remainder is smaller than the usize range");
        low + offset
    }

    /// Shuffles the slice in place using deterministic Fisher-Yates swaps.
    fn shuffle_str(&mut self, items: &mut [&str]) {
        for index in (1..items.len()).rev() {
            let other = self.range(0, index + 1);
            items.swap(index, other);
        }
    }
}
