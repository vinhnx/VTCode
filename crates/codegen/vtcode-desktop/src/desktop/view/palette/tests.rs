//! Tests for the palette ingestion boundary (`palette.rs` + `palette/tests.rs`).

use super::*;

/// The render path clones the whole group table on every frame to hand it
/// to `'static` row closures. A `String` field would copy these bytes each
/// time; `SharedString` must keep one backing allocation across clones.
#[test]
fn palette_group_clone_shares_display_text_without_copying_bytes() {
    let mut entry = command(
        "A retained palette label that is long enough to live on the heap".to_string(),
        PaletteAction::ChooseWorkspace,
    );
    entry.description = Some(
        "A retained palette description that is long enough to live on the heap"
            .to_string()
            .into(),
    );
    let groups = share_palette_commands(vec![(
        "A retained group label that is long enough to live on the heap".to_string(),
        vec![entry],
    )]);
    let cloned = groups.clone();

    let (label, entries) = &groups[0];
    let (cloned_label, cloned_entries) = &cloned[0];
    assert_eq!(label.as_str(), cloned_label.as_str());
    assert_eq!(
        label.as_str().as_ptr(),
        cloned_label.as_str().as_ptr(),
        "group labels must share their backing text when the table is cloned"
    );
    assert_eq!(entries[0].label.as_str(), cloned_entries[0].label.as_str());
    assert_eq!(
        entries[0].label.as_str().as_ptr(),
        cloned_entries[0].label.as_str().as_ptr(),
        "entry labels must share their backing text when the table is cloned"
    );
    let description = entries[0].description.as_ref().expect("description retained");
    let cloned_description = cloned_entries[0].description.as_ref().expect("description retained");
    assert_eq!(
        description.as_str().as_ptr(),
        cloned_description.as_str().as_ptr(),
        "entry descriptions must share their backing text when the table is cloned"
    );
    let expected = format!("{} · {}", entries[0].label, description);
    assert_eq!(
        entries[0].accessible_label.as_str(),
        expected,
        "the a11y label is derived once at ingestion, not on each render frame"
    );
    assert_eq!(
        entries[0].accessible_label.as_str().as_ptr(),
        cloned_entries[0].accessible_label.as_str().as_ptr(),
        "derived a11y labels must share their backing text when the table is cloned"
    );
}

#[test]
fn accessible_label_falls_back_to_label_without_description() {
    let groups = share_palette_commands(vec![(
        "group".to_string(),
        vec![command(
            "Create a thread with a name long enough to live on the heap".to_string(),
            PaletteAction::NewSession,
        )],
    )]);
    let entry = &groups[0].1[0];
    assert_eq!(entry.accessible_label, entry.label);
    assert_eq!(
        entry.accessible_label.as_str().as_ptr(),
        entry.label.as_str().as_ptr(),
        "no-description entries must reuse the label handle rather than build a new value"
    );
}
