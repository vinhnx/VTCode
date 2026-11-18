use super::{DiscardIntent, Route, RouteHistory, discard_prompt_text};

#[test]
fn discard_prompt_text_names_each_surface() {
    crate::desktop::localization::initialize(crate::desktop::localization::UiLanguage::English);
    let (title, detail) = discard_prompt_text(&DiscardIntent::CloseRoute(Route::Settings));
    assert_eq!(title, "Discard unsaved changes?");
    assert_eq!(detail, "Unsaved shortcut and theme edits will be lost.");
    // Asymmetric path proves the interpolation lands in the detail.
    let (title, detail) = discard_prompt_text(&DiscardIntent::CloseConfigEditor("agent.default_model".into()));
    assert_eq!(title, "Discard unsaved value?");
    assert_eq!(detail, "Changes to agent.default_model will be lost.");
    let (title, detail) = discard_prompt_text(&DiscardIntent::CancelRename("README.md".into()));
    assert_eq!(title, "Discard rename?");
    assert_eq!(detail, "The name change will be lost.");
}

#[test]
fn starts_on_the_workspace_base() {
    let history = RouteHistory::new();
    assert_eq!(history.current(), Route::Workspace);
    assert_eq!(RouteHistory::default().current(), Route::Workspace);
}

#[test]
fn opening_a_panel_replaces_an_open_panel() {
    let mut history = RouteHistory::new();
    history.open(Route::Settings);
    history.open(Route::Browser);
    assert_eq!(history.current(), Route::Browser);
    assert_eq!(history.entries, vec![Route::Workspace, Route::Browser]);
}

#[test]
fn opening_the_workspace_resets_to_the_base() {
    let mut history = RouteHistory::new();
    history.open(Route::Review);
    history.open(Route::Workspace);
    assert_eq!(history.current(), Route::Workspace);
    assert_eq!(history.entries, vec![Route::Workspace]);
}

#[test]
fn dismissing_clears_panels_back_to_the_base() {
    let mut history = RouteHistory::new();
    history.open(Route::Settings);
    history.dismiss_to_workspace();
    assert_eq!(history.current(), Route::Workspace);
    assert_eq!(history.entries, vec![Route::Workspace]);
    history.dismiss_to_workspace();
    assert_eq!(history.current(), Route::Workspace);
}

#[test]
fn current_falls_back_to_workspace_off_the_entries() {
    let history = RouteHistory { entries: Vec::new(), cursor: 5 };
    assert_eq!(history.current(), Route::Workspace);
}

#[test]
fn back_and_forward_move_the_cursor_without_dropping_entries() {
    let mut history = RouteHistory::new();
    history.open(Route::Settings);
    assert!(history.can_back());
    assert!(!history.can_forward());
    assert_eq!(history.back(), Route::Workspace);
    assert!(!history.can_back());
    assert!(history.can_forward());
    assert_eq!(history.forward(), Route::Settings);
    assert!(history.can_back());
    assert!(!history.can_forward());
}

#[test]
fn opening_truncates_forward_entries() {
    let mut history = RouteHistory::new();
    history.open(Route::Settings);
    assert_eq!(history.back(), Route::Workspace);
    history.open(Route::Browser);
    assert_eq!(history.current(), Route::Browser);
    assert!(!history.can_forward());
    assert_eq!(history.back(), Route::Workspace);
    assert_eq!(history.forward(), Route::Browser);
}

#[test]
fn back_and_forward_clamp_at_the_ends() {
    let mut history = RouteHistory::new();
    assert_eq!(history.back(), Route::Workspace);
    assert_eq!(history.forward(), Route::Workspace);
}
