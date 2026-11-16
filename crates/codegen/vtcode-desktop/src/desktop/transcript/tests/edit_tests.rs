use super::*;

#[test]
fn edit_link_changes_remeasure_only_the_affected_row() {
    let mut transcript = Transcript::default();
    transcript.push(item(0, "tool", "completed"));
    let _ = transcript.take_layout_changes();
    transcript.set_edit_path("0", Some("src/a.rs"));
    assert!(
        matches!(transcript.take_layout_changes().as_slice(), [super::super::TranscriptLayoutChange::RemeasureItems(range)] if *range == (0..1))
    );
    transcript.set_edit_path("0", Some("src/a.rs"));
    assert!(transcript.take_layout_changes().is_empty());
    transcript.set_edit_path("0", None);
    assert!(
        matches!(transcript.take_layout_changes().as_slice(), [super::super::TranscriptLayoutChange::RemeasureItems(range)] if *range == (0..1))
    );
}
