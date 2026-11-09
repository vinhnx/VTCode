use super::*;

#[test]
fn completion_time_requires_matching_assistant_identity() {
    let mut transcript = Transcript::default();
    transcript.push(item(1, "user", "question"));
    transcript.push(item(2, "assistant", "answer"));
    transcript.set_item_turn("1", "turn-a");
    transcript.set_item_turn("2", "turn-b");
    transcript.set_completion_time("turn-a", "1", "10:15".into());
    transcript.set_completion_time("turn-a", "missing", "10:15".into());
    transcript.set_completion_time("turn-a", "2", "10:15".into());
    assert!(transcript.get(0).unwrap().completed_at.is_none());
    assert!(transcript.get(1).unwrap().completed_at.is_none());
    transcript.set_completion_time("turn-b", "2", "10:15".into());
    assert_eq!(transcript.get(1).unwrap().completed_at.as_deref(), Some("10:15"));
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
