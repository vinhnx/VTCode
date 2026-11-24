use super::*;

#[test]
fn completion_time_requires_matching_assistant_identity() {
    let mut transcript = Transcript::default();
    transcript.push(item(1, "user", "question"));
    transcript.push(item(2, "assistant", "answer"));
    transcript.set_item_turn("1", "turn-a");
    transcript.set_item_turn("2", "turn-b");
    transcript.set_completion_time("turn-a", "1", "10:15".into(), None);
    transcript.set_completion_time("turn-a", "missing", "10:15".into(), None);
    transcript.set_completion_time("turn-a", "2", "10:15".into(), None);
    assert!(transcript.get(0).unwrap().completed_at.is_none());
    assert!(transcript.get(1).unwrap().completed_at.is_none());
    transcript.set_completion_time("turn-b", "2", "10:15".into(), Some("13m 24s".into()));
    assert_eq!(transcript.get(1).unwrap().completed_at.as_deref(), Some("10:15"));
    assert_eq!(transcript.get(1).unwrap().completed_elapsed.as_deref(), Some("13m 24s"));
}

#[test]
fn humanize_elapsed_uses_clock_symbol_tiers() {
    assert_eq!(humanize_elapsed(0), "0s");
    assert_eq!(humanize_elapsed(42), "42s");
    assert_eq!(humanize_elapsed(60), "1m");
    assert_eq!(humanize_elapsed(804), "13m 24s");
    assert_eq!(humanize_elapsed(3600), "1h");
    assert_eq!(humanize_elapsed(3900), "1h 5m");
    assert_eq!(humanize_elapsed(-5), "0s");
}

#[test]
fn item_turn_identity_survives_streamed_updates() {
    let mut transcript = Transcript::default();
    transcript.upsert(item(7, "assistant", "first"));
    transcript.set_item_turn("7", "turn-a");
    transcript.upsert(item(7, "assistant", "updated"));
    assert_eq!(transcript.get(0).unwrap().turn_id.as_deref(), Some("turn-a"));
    transcript.upsert(item(8, "tool", "running"));
    transcript.set_item_turn("8", "turn-b");
    assert_eq!(transcript.get(1).unwrap().turn_id.as_deref(), Some("turn-b"));
}

#[test]
fn complete_turn_group_invalidates_single_row_turn() {
    // With no other rows in the turn there is no group head, but the final
    // row's file summary is still rewritten. The revision and remeasure must
    // reflect that mutation instead of being skipped.
    let mut transcript = Transcript::default();
    transcript.push(item(1, "assistant", "answer"));
    transcript.set_item_turn("1", "turn-a");
    let before = transcript.revision();
    let _ = transcript.take_layout_changes();

    transcript.complete_turn_group("turn-a", "1");

    assert!(transcript.revision() > before, "single-row completion still mutates the final row",);
    assert!(
        transcript.take_layout_changes().iter().any(|change| matches!(
            change,
            super::super::TranscriptLayoutChange::RemeasureItems(range) if *range == (0..1)
        )),
        "the final row must be remeasured even without a group head",
    );
}

#[test]
fn turn_files_deduplicate_and_exclude_failed_or_other_turns() {
    use super::super::{DiffCard, DiffFile, DiffFileKind};
    let mut transcript = Transcript::default();
    for (id, turn, path, failed) in [
        (1, "a", "one.rs", false),
        (2, "a", "one.rs", false),
        (3, "a", "bad.rs", true),
        (4, "b", "other.rs", false),
    ] {
        let mut row = item(id, "diff", "");
        row.diff = Some(DiffCard {
            files: vec![DiffFile { path: path.into(), kind: DiffFileKind::Update }],
            additions: None,
            deletions: None,
            incomplete: false,
            failed,
            patch: String::new(),
        });
        transcript.push(row);
        transcript.set_item_turn(&id.to_string(), turn);
    }
    transcript.push(item(5, "assistant", "done"));
    transcript.set_item_turn("5", "a");
    transcript.complete_turn_group("a", "5");
    let files = &transcript.get(4).unwrap().turn_files;
    assert_eq!(files.iter().map(|file| file.file.path.as_str()).collect::<Vec<_>>(), ["one.rs"]);
}

#[test]
fn turn_files_deduplicate_late_repeats_without_losing_first_seen_order() {
    use super::super::{DiffCard, DiffFile, DiffFileKind};
    let patch_for = |path: &str| {
        format!("diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1 +1,2 @@\n-old\n+new\n+extra\n")
    };
    let mut transcript = Transcript::default();
    // Enough distinct paths that a repeated linear scan would revisit each one
    // many times, plus one late repeat of the very first path.
    for index in 1..=32 {
        let path = format!("file-{index}.rs");
        let mut row = item(index, "diff", "");
        row.diff = Some(DiffCard {
            files: vec![DiffFile { path: path.clone(), kind: DiffFileKind::Update }],
            additions: None,
            deletions: None,
            incomplete: false,
            failed: false,
            patch: patch_for(&path),
        });
        transcript.push(row);
        transcript.set_item_turn(&index.to_string(), "a");
    }
    let repeat = 33;
    let mut row = item(repeat, "diff", "");
    row.diff = Some(DiffCard {
        files: vec![DiffFile {
            path: "file-1.rs".into(),
            kind: DiffFileKind::Update,
        }],
        additions: None,
        deletions: None,
        incomplete: false,
        failed: false,
        patch: patch_for("file-1.rs"),
    });
    transcript.push(row);
    transcript.set_item_turn(&repeat.to_string(), "a");
    transcript.push(item(34, "assistant", "done"));
    transcript.set_item_turn("34", "a");
    transcript.complete_turn_group("a", "34");

    let final_item = transcript.get(33).unwrap();
    let files = &final_item.turn_files;
    assert_eq!(files.len(), 32);
    assert_eq!(files[0].file.path, "file-1.rs");
    assert_eq!(files[31].file.path, "file-32.rs");
    // Same parseable patch for both rows; only the repeated path loses counts.
    assert_eq!(files[0].counts, None);
    assert_eq!(files[1].counts, Some((2, 1)));
    assert_eq!(final_item.turn_file_totals, None);
}

#[test]
fn turn_file_preview_lookup_keeps_the_first_entry_for_a_repeated_path() {
    use super::super::{DiffCard, DiffFile, DiffFileKind};
    // One patch with two sections for the same path: the old `.find` took the
    // first preview, so counts must come from `+2/-1`, not `+3/-1`.
    let patch = concat!(
        "diff --git a/one.rs b/one.rs\n",
        "--- a/one.rs\n",
        "+++ b/one.rs\n",
        "@@ -1 +1,2 @@\n",
        "-old\n",
        "+new\n",
        "+extra\n",
        "diff --git a/one.rs b/one.rs\n",
        "--- a/one.rs\n",
        "+++ b/one.rs\n",
        "@@ -1 +1,3 @@\n",
        "-old\n",
        "+new\n",
        "+extra\n",
        "+third\n",
    );
    let mut transcript = Transcript::default();
    let mut row = item(1, "diff", "");
    row.diff = Some(DiffCard {
        files: vec![DiffFile { path: "one.rs".into(), kind: DiffFileKind::Update }],
        additions: None,
        deletions: None,
        incomplete: false,
        failed: false,
        patch: patch.into(),
    });
    transcript.push(row);
    transcript.set_item_turn("1", "a");
    transcript.push(item(2, "assistant", "done"));
    transcript.set_item_turn("2", "a");
    transcript.complete_turn_group("a", "2");
    assert_eq!(transcript.get(1).unwrap().turn_files[0].counts, Some((2, 1)));
}

#[test]
fn turn_file_counts_require_one_complete_parseable_change() {
    use super::super::{DiffCard, DiffFile, DiffFileKind};
    let patch = "diff --git a/one.rs b/one.rs\n--- a/one.rs\n+++ b/one.rs\n@@ -1 +1,2 @@\n-old\n+new\n+extra\n";
    let mut transcript = Transcript::default();
    for (id, path, incomplete) in [(1, "one.rs", false), (2, "two.rs", true)] {
        let mut row = item(id, "diff", "");
        row.diff = Some(DiffCard {
            files: vec![DiffFile { path: path.into(), kind: DiffFileKind::Update }],
            additions: None,
            deletions: None,
            incomplete,
            failed: false,
            patch: patch.into(),
        });
        transcript.push(row);
        transcript.set_item_turn(&id.to_string(), "a");
    }
    transcript.push(item(3, "assistant", "done"));
    transcript.set_item_turn("3", "a");
    transcript.complete_turn_group("a", "3");
    let files = &transcript.get(2).unwrap().turn_files;
    assert_eq!(files[0].counts, Some((2, 1)));
    assert_eq!(files[1].counts, None);
    assert_eq!(transcript.get(2).unwrap().turn_file_totals, None);
    assert!(transcript.get(2).unwrap().turn_files_incomplete);
    transcript.remove("2");
    transcript.complete_turn_group("a", "3");
    assert_eq!(transcript.get(1).unwrap().turn_file_totals, Some((2, 1)));
    assert!(!transcript.get(1).unwrap().turn_files_incomplete);
}

#[test]
fn turn_file_pages_reach_all_paths_and_return_to_preview() {
    use super::super::{DiffFile, DiffFileKind, TurnFile};
    let mut transcript = Transcript::default();
    let mut final_item = item(1, "assistant", "done");
    final_item.turn_files = (0..120)
        .map(|index| TurnFile {
            file: DiffFile {
                path: format!("{index}.rs"),
                kind: DiffFileKind::Update,
            },
            counts: None,
        })
        .collect();
    transcript.push(final_item);
    for expected in [3, 53, 103, 0] {
        transcript.toggle_turn_files("1");
        assert_eq!(transcript.get(0).unwrap().turn_files_offset, expected);
    }
}

#[test]
fn completed_turn_groups_matching_work_around_final() {
    let mut transcript = Transcript::default();
    for (id, role, turn) in [
        (1, "assistant", "a"),
        (2, "tool", "b"),
        (3, "tool", "a"),
        (4, "assistant", "a"),
        (5, "tool", "a"),
    ] {
        transcript.push(item(id, role, "content"));
        transcript.set_item_turn(&id.to_string(), turn);
    }
    transcript.complete_turn_group("a", "4");
    assert!(transcript.get(0).unwrap().turn_group_head);
    assert!(!transcript.get(1).unwrap().turn_grouped);
    assert!(transcript.get(2).unwrap().turn_grouped);
    assert!(!transcript.get(3).unwrap().turn_grouped);
    assert!(transcript.get(4).unwrap().turn_grouped);
    transcript.set_turn_group_expanded("a", true);
    assert!(transcript.get(0).unwrap().turn_group_expanded);
    assert!(transcript.get(2).unwrap().turn_group_expanded);
    transcript.complete_turn_group("a", "4");
    assert!(transcript.get(0).unwrap().turn_group_expanded);
    assert!(transcript.get(2).unwrap().turn_group_expanded);
}

#[test]
fn turn_final_prefers_reported_identity_then_newest_message() {
    let mut transcript = Transcript::default();
    for (id, role, turn) in [
        (1, "user", "a"),
        (2, "assistant", "a"),
        (3, "tool", "a"),
        (4, "assistant", "a"),
        (5, "assistant", "b"),
    ] {
        transcript.push(item(id, role, "content"));
        transcript.set_item_turn(&id.to_string(), turn);
    }
    assert_eq!(transcript.resolve_turn_final("a", Some("2")).as_deref(), Some("2"));
    assert_eq!(transcript.resolve_turn_final("a", None).as_deref(), Some("4"));
    assert_eq!(transcript.resolve_turn_final("a", Some("missing")).as_deref(), Some("4"));
    assert_eq!(transcript.resolve_turn_final("a", Some("5")).as_deref(), Some("4"));
    assert_eq!(transcript.resolve_turn_final("a", Some("1")).as_deref(), Some("4"));
    assert_eq!(transcript.resolve_turn_final("a", Some("3")).as_deref(), Some("4"));
    assert_eq!(transcript.resolve_turn_final("b", None).as_deref(), Some("5"));
    assert_eq!(transcript.resolve_turn_final("b", Some("4")).as_deref(), Some("5"));
}

#[test]
fn turn_final_without_assistant_row_resolves_to_nothing() {
    let mut transcript = Transcript::default();
    transcript.push(item(1, "user", "question"));
    transcript.set_item_turn("1", "a");
    transcript.push(item(2, "tool", "work"));
    transcript.set_item_turn("2", "a");
    assert_eq!(transcript.resolve_turn_final("a", None), None);
    assert_eq!(transcript.resolve_turn_final("a", Some("2")), None);
    assert_eq!(transcript.resolve_turn_final("missing-turn", None), None);
}

#[test]
fn missing_or_foreign_final_identity_does_not_hide_work() {
    let mut transcript = Transcript::default();
    transcript.push(item(1, "tool", "work"));
    transcript.set_item_turn("1", "turn-a");
    transcript.push(item(2, "assistant", "other answer"));
    transcript.set_item_turn("2", "turn-b");
    transcript.complete_turn_group("turn-a", "missing");
    transcript.complete_turn_group("turn-a", "2");
    assert!(!transcript.get(0).unwrap().turn_grouped);
    assert!(!transcript.get(1).unwrap().turn_grouped);
}

#[test]
fn removing_turn_group_head_promotes_remaining_work() {
    let mut transcript = Transcript::default();
    for id in 1..=3 {
        transcript.push(item(id, "assistant", "content"));
        transcript.set_item_turn(&id.to_string(), "turn");
    }
    transcript.complete_turn_group("turn", "3");
    transcript.remove("1");
    assert!(transcript.get(0).unwrap().turn_group_head);
    assert!(!transcript.get(0).unwrap().turn_group_expanded);
}
