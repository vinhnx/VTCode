use super::*;

fn state_with(tags: &[(&str, &[&str])], active: &[&str]) -> ProjectTagsState {
    let mut state = ProjectTagsState::default();
    for (path, tags) in tags {
        let _ = state.set_workspace_tags(path, tags.iter().map(|tag| (*tag).to_owned()).collect());
    }
    state.active = active.iter().map(|tag| (*tag).to_owned()).collect();
    state
}

#[test]
fn normalize_tag_accepts_hash_and_collapses_case_and_spaces() {
    assert_eq!(normalize_tag("#Work  INFRA"), Some("work infra".to_owned()));
    assert_eq!(normalize_tag("  #ratatui\n"), Some("ratatui".to_owned()));
    assert_eq!(normalize_tag("One  Two Three"), Some("one two three".to_owned()));
}

#[test]
fn normalize_tag_rejects_invalid_names() {
    assert_eq!(normalize_tag(""), None);
    assert_eq!(normalize_tag("#"), None);
    assert_eq!(normalize_tag("   "), None);
    assert_eq!(normalize_tag("work,infra"), None);
    assert_eq!(normalize_tag("work\ninfra"), None);
    assert_eq!(normalize_tag("work\u{0000}"), None);
    assert_eq!(normalize_tag(&"a".repeat(MAX_TAG_CHARS + 1)), None);
}

#[test]
fn normalize_tag_checks_length_after_collapsing_spaces() {
    let spaced = format!("a{}b", " ".repeat(MAX_TAG_CHARS));
    assert_eq!(normalize_tag(&spaced), Some("a b".to_owned()));
}

#[test]
fn normalize_tag_accepts_the_length_boundary() {
    let tag = "a".repeat(MAX_TAG_CHARS);
    assert_eq!(normalize_tag(&tag), Some(tag));
}

#[test]
fn all_tags_counts_each_workspace_once() {
    let state = state_with(&[("a", &["work", "work", "infra"]), ("b", &["work"]), ("c", &["home"])], &[]);
    let counts = state.all_tags();
    assert_eq!(counts.get("work"), Some(&2));
    assert_eq!(counts.get("infra"), Some(&1));
    assert_eq!(counts.get("home"), Some(&1));
    assert_eq!(counts.len(), 3);
}

#[test]
fn matches_uses_or_semantics_and_allows_unfiltered_workspaces() {
    let state = state_with(&[("a", &["work"]), ("b", &["infra"]), ("c", &["home"])], &["work", "infra"]);
    assert!(state.matches("a"), "first active tag matches");
    assert!(state.matches("b"), "second active tag matches");
    assert!(!state.matches("c"), "unrelated workspace is filtered out");
    assert!(!state.matches("unknown"), "unknown workspace is filtered out");

    let unfiltered = state_with(&[("a", &["work"])], &[]);
    assert!(unfiltered.matches("anything"), "an empty filter shows every workspace");
}

#[test]
fn matches_ignores_stale_active_tags() {
    let state = state_with(&[("a", &["work"]), ("b", &["infra"])], &["gone"]);
    assert!(state.matches("a"), "a stale active tag must not hide everything");
    assert!(state.matches("b"));
    assert_eq!(state.active_label(), "", "a stale tag must not label the trigger");
}

#[test]
fn visible_workspaces_preserves_caller_order() {
    let state = state_with(&[("b", &["work"])], &["work"]);
    let recent = vec!["c".to_owned(), "b".to_owned(), "a".to_owned()];
    assert_eq!(state.visible_workspaces(&recent), vec!["b".to_owned()]);

    let unfiltered = ProjectTagsState::default();
    assert_eq!(unfiltered.visible_workspaces(&recent), recent);
}

#[test]
fn active_label_formats_one_and_many_tags() {
    let mut state = state_with(&[("a", &["work", "infra"])], &[]);
    assert_eq!(state.active_label(), "");
    state.active = vec!["work".to_owned()];
    assert_eq!(state.active_label(), "work");
    state.active = vec!["work".to_owned(), "infra".to_owned()];
    assert_eq!(state.active_label(), "work +1");
    let _ = state.set_workspace_tags("b", vec!["home".to_owned()]);
    state.active = vec!["work".to_owned(), "infra".to_owned(), "home".to_owned()];
    assert_eq!(state.active_label(), "work +2");
}

#[test]
fn set_workspace_tags_normalizes_dedupes_and_caps() {
    let mut state = ProjectTagsState::default();
    let tags = (0..=MAX_TAGS_PER_WORKSPACE)
        .map(|index| format!("Tag {index}"))
        .collect::<Vec<_>>();
    assert!(state.set_workspace_tags("a", tags.clone()));
    let stored = &state.workspaces["a"];
    assert_eq!(stored.len(), MAX_TAGS_PER_WORKSPACE, "cap wins over the last tag");
    assert_eq!(stored[0], "tag 0");
    assert!(!state.set_workspace_tags("a", tags), "an identical write is a no-op");

    assert!(state.set_workspace_tags("a", vec!["WORK".to_owned(), "#work".to_owned()]));
    assert_eq!(state.workspaces["a"], vec!["work".to_owned()]);
    assert!(!state.set_workspace_tags("a", vec!["bad,tag".to_owned()]));
    assert_eq!(state.workspaces["a"], vec!["work".to_owned()], "invalid replacements are ignored");
    assert!(state.set_workspace_tags("a", vec![]), "clearing tags changes state");
    assert!(!state.workspaces.contains_key("a"), "an empty tag list removes the entry");
}

#[test]
fn add_tag_accepts_hash_and_refuses_duplicates_and_cap() {
    let mut state = state_with(&[("a", &["work"])], &[]);
    assert!(!state.add_tag("a", "#WORK"), "duplicate is a no-op");
    assert!(state.add_tag("a", "#infra"));
    assert_eq!(state.workspaces["a"], vec!["work".to_owned(), "infra".to_owned()]);
    assert!(!state.add_tag("b", "bad,tag"));
    assert!(!state.workspaces.contains_key("b"));

    let full = (0..MAX_TAGS_PER_WORKSPACE).map(|i| format!("t{i}")).collect::<Vec<_>>();
    let _ = state.set_workspace_tags("full", full);
    assert!(!state.add_tag("full", "extra"), "the per-project cap holds");
}

#[test]
fn remove_tag_drops_the_workspace_entry_when_empty() {
    let mut state = state_with(&[("a", &["work"])], &[]);
    assert!(state.remove_tag("a", "work"));
    assert!(!state.workspaces.contains_key("a"));
    assert!(!state.remove_tag("a", "work"));
}

#[test]
fn rename_tag_merges_into_existing_and_prunes_active() {
    let mut state = state_with(&[("a", &["work", "infra"]), ("b", &["work"])], &["work", "infra"]);
    assert!(state.rename_tag("work", "#INFRA"));
    assert_eq!(state.workspaces["a"], vec!["infra".to_owned()], "merge dedupes on one project");
    assert_eq!(state.workspaces["b"], vec!["infra".to_owned()]);
    assert_eq!(state.active, vec!["infra".to_owned()], "renaming onto an active tag merges the filter too");
    assert!(!state.rename_tag("work", "anything"), "an unknown source is a no-op");
    assert!(!state.rename_tag("infra", "bad,tag"), "invalid targets are rejected");
}

#[test]
fn delete_tag_removes_everywhere_and_prunes_active() {
    let mut state = state_with(&[("a", &["work"]), ("b", &["work", "infra"])], &["work", "infra"]);
    assert!(state.delete_tag("work"));
    assert!(!state.workspaces.contains_key("a"));
    assert_eq!(state.workspaces["b"], vec!["infra".to_owned()]);
    assert_eq!(state.active, vec!["infra".to_owned()]);
    assert!(!state.delete_tag("work"), "a second delete is a no-op");
}

#[test]
fn retain_recent_drops_evicted_projects_and_their_tags() {
    let mut state = state_with(&[("a", &["work"]), ("b", &["infra"])], &["work", "infra"]);
    assert!(state.retain_recent(&["b".to_owned()]));
    assert!(!state.workspaces.contains_key("a"), "the evicted project's tags are dropped");
    assert!(!state.all_tags().contains_key("work"), "no phantom tag remains");
    assert_eq!(state.active, ["infra".to_owned()], "a tag that lost its last project is pruned");
    assert_eq!(state.active_label(), "infra");
    assert!(!state.retain_recent(&["b".to_owned()]), "an already-clean map is a no-op");
}

#[test]
fn prune_active_drops_only_stale_tags() {
    let mut state = state_with(&[("a", &["work"])], &["work", "gone", "work"]);
    assert!(state.prune_active());
    assert_eq!(state.active, vec!["work".to_owned()]);
    assert!(!state.prune_active(), "an already-clean filter is a no-op");
}

#[test]
fn tag_new_workspace_requires_exactly_one_active_tag() {
    let mut state = state_with(&[("a", &["work"])], &["work"]);
    assert!(state.tag_new_workspace("new"));
    assert_eq!(state.workspaces["new"], vec!["work".to_owned()]);
    assert!(!state.tag_new_workspace("new"), "an existing workspace is not re-tagged");

    let mut two = state_with(&[("a", &["work"])], &["work", "infra"]);
    assert!(!two.tag_new_workspace("new"), "ambiguous contexts never auto-tag");
    assert!(!two.workspaces.contains_key("new"));
}

#[test]
fn serde_round_trip_keeps_tags_and_filter() {
    let state = state_with(&[("a", &["work"])], &["work"]);
    let json = serde_json::to_string(&state).expect("state serializes");
    let decoded: ProjectTagsState = serde_json::from_str(&json).expect("state deserializes");
    assert_eq!(decoded, state);

    let legacy: ProjectTagsState = serde_json::from_str("{}").expect("legacy state defaults");
    assert_eq!(legacy, ProjectTagsState::default());
}
