use super::super::DesktopView;
use super::super::PaletteKind;
use super::*;
use gpui_kit::Entity;

fn open_history_with_prompts(
    window: &AnyWindowHandle,
    view: &Entity<DesktopView>,
    prompts: &[&str],
    cx: &mut TestAppContext,
) {
    cx.update_window(*window, |_, window, cx| {
        view.update(cx, |view, cx| {
            for (index, prompt) in prompts.iter().enumerate() {
                view.transcript.push(TranscriptItem::new(
                    format!("history-user-{index}"),
                    "user".into(),
                    "You".into(),
                    (*prompt).into(),
                ));
            }
            view.composer.update(cx, |state, cx| {
                state.set_value("", window, cx);
                state.focus(window, cx);
            });
        });
        window.render_frame(cx);
        window.press("ctrl-r", cx);
        window.render_frame(cx);
        assert!(matches!(view.read(cx).palette_kind, PaletteKind::PromptHistory));
    })
    .unwrap();
}

fn set_history_query(window: &AnyWindowHandle, view: &Entity<DesktopView>, query: &str, cx: &mut TestAppContext) {
    cx.update_window(*window, |_, window, cx| {
        view.update(cx, |view, cx| {
            let palette = view.palette.clone().expect("history palette must be open");
            palette.update(cx, |state, cx| state.set_query(query, window, cx));
            view.focus_palette_state(window, cx);
        });
        window.render_frame(cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn history_search_matches_second_line_of_multiline_prompt(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let target = "Review stale diffs\nsecond line mentions zebra for recall";
    open_history_with_prompts(&window, &view, &["Implement current changes", target], cx);
    // "zebra" appears only in the second line: label-only search misses it.
    set_history_query(&window, &view, "zebra", cx);
    cx.update_window(window, |_, window, cx| {
        let matched = view.read(cx).palette.as_ref().expect("palette open").read(cx).matched_count();
        assert_eq!(matched, 1, "full prompt text must be searchable, not just the title row");
        window.press("enter", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, _, cx| {
        assert_eq!(view.read(cx).composer.read(cx).value(), target);
        assert!(view.read(cx).palette.is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn history_search_matches_text_beyond_truncated_title(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    // Titles truncate at 72 chars; the needle sits past that cut.
    let target = format!("{} needle-past-title", "a".repeat(80));
    open_history_with_prompts(&window, &view, &["unrelated short prompt", target.as_str()], cx);
    set_history_query(&window, &view, "needle-past-title", cx);
    cx.update_window(window, |_, window, cx| {
        let matched = view.read(cx).palette.as_ref().expect("palette open").read(cx).matched_count();
        assert_eq!(matched, 1, "search must cover the full prompt, not the 72-char title");
        window.press("enter", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, _, cx| {
        assert_eq!(view.read(cx).composer.read(cx).value(), target.as_str());
    })
    .unwrap();
}

fn open_history_with_backend(
    prompts: &[&str],
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<DesktopView>, tokio::sync::mpsc::Receiver<BackendCommand>) {
    let (window, view) = open_desktop(cx);
    let (commands, requests) = tokio::sync::mpsc::channel(8);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("history-session".into());
            for (index, prompt) in prompts.iter().enumerate() {
                view.transcript.push(TranscriptItem::new(
                    format!("history-user-{index}"),
                    "user".into(),
                    "You".into(),
                    (*prompt).into(),
                ));
            }
            view.composer.update(cx, |state, cx| {
                state.set_value("", window, cx);
                state.focus(window, cx);
            });
        });
        window.render_frame(cx);
        window.press("ctrl-r", cx);
        window.render_frame(cx);
    })
    .unwrap();
    (window, view, requests)
}

fn deliver_recent_prompts(
    window: &AnyWindowHandle,
    view: &Entity<DesktopView>,
    scope: &str,
    prompts: &[(&str, &str)],
    cx: &mut TestAppContext,
) {
    let result = serde_json::json!({
        "scope": scope,
        "prompts": prompts
            .iter()
            .map(|(content, session)| serde_json::json!({ "content": content, "sessionId": session }))
            .collect::<Vec<_>>(),
    });
    cx.update_window(*window, |_, _, cx| {
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::SESSION_RECENT_PROMPTS.into(),
                    result: Some(result),
                    error: None,
                },
                cx,
            );
        });
    })
    .unwrap();
}

fn section_contents(groups: &[(String, Vec<std::rc::Rc<super::super::PaletteCommand>>)]) -> Vec<(String, Vec<String>)> {
    groups
        .iter()
        .map(|(label, entries)| {
            let prompts = entries
                .iter()
                .filter_map(|entry| match &entry.action {
                    super::super::PaletteAction::InsertHistoryPrompt(prompt) => Some(prompt.clone()),
                    _ => None,
                })
                .collect();
            (label.clone(), prompts)
        })
        .collect()
}

#[gpui_kit::test]
fn history_palette_caps_rows_at_cli_parity_limit(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let prompts: Vec<String> = (0..250).map(|index| format!("prompt {index:03}")).collect();
    let refs: Vec<&str> = prompts.iter().map(String::as_str).collect();
    open_history_with_prompts(&window, &view, &refs, cx);
    cx.update_window(window, |_, _, cx| {
        let rows: usize = view.read(cx).palette_groups.iter().map(|(_, entries)| entries.len()).sum();
        assert_eq!(rows, 200, "history rows must cap at 200 like the CLI picker");
    })
    .unwrap();
}

#[gpui_kit::test]
fn empty_thread_shows_recents_after_fetch_and_accepts_full_text(cx: &mut TestAppContext) {
    let (window, view, mut requests) = open_history_with_backend(&[], cx);
    expect_recent_prompts_request(&mut requests, super::super::HistoryScope::Everywhere);
    cx.update_window(window, |_, _, cx| {
        assert!(view.read(cx).palette_groups.iter().all(|(_, entries)| entries.is_empty()));
    })
    .unwrap();

    let target = "Other thread prompt\nwith a second line";
    deliver_recent_prompts(&window, &view, "everywhere", &[(target, "other-session")], cx);
    cx.update_window(window, |_, window, cx| {
        let sections = section_contents(&view.read(cx).palette_groups);
        assert_eq!(sections.len(), 1, "empty thread shows the recents section only");
        assert_eq!(sections[0].1, vec![target.to_string()]);
        // Kit installs the rebuilt model at render; focus and accept after it.
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.press("enter", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, _, cx| {
        assert_eq!(view.read(cx).composer.read(cx).value(), target);
        assert!(view.read(cx).palette.is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn recents_dedup_against_live_thread_with_thread_winning(cx: &mut TestAppContext) {
    let (window, view, mut requests) = open_history_with_backend(&["shared prompt"], cx);
    expect_recent_prompts_request(&mut requests, super::super::HistoryScope::Everywhere);
    deliver_recent_prompts(
        &window,
        &view,
        "everywhere",
        &[("shared prompt", "other-session"), ("foreign prompt", "other-session")],
        cx,
    );
    cx.update_window(window, |_, _, cx| {
        let sections = section_contents(&view.read(cx).palette_groups);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].1, vec!["shared prompt".to_string()]);
        assert_eq!(sections[1].1, vec!["foreign prompt".to_string()]);
    })
    .unwrap();
}

#[gpui_kit::test]
fn session_scope_hides_recents_and_skips_fetch(cx: &mut TestAppContext) {
    let (window, view, mut requests) = open_history_with_backend(&["thread prompt"], cx);
    expect_recent_prompts_request(&mut requests, super::super::HistoryScope::Everywhere);
    deliver_recent_prompts(&window, &view, "everywhere", &[("foreign prompt", "other-session")], cx);
    cx.update_window(window, |_, _, cx| {
        assert_eq!(view.read(cx).palette_groups.len(), 2);
        view.update(cx, |view, cx| {
            view.set_history_scope(super::super::HistoryScope::Session, cx);
        });
    })
    .unwrap();
    cx.update_window(window, |_, _, cx| {
        let sections = section_contents(&view.read(cx).palette_groups);
        assert_eq!(sections.len(), 1, "session scope shows the live thread only");
        assert_eq!(sections[0].1, vec!["thread prompt".to_string()]);
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "session scope needs no fetch");
}

#[gpui_kit::test]
fn ctrl_s_cycles_scope_and_refetches_cross_thread_scopes(cx: &mut TestAppContext) {
    use super::super::HistoryScope;
    let (window, view, mut requests) = open_history_with_backend(&["thread prompt"], cx);
    expect_recent_prompts_request(&mut requests, HistoryScope::Everywhere);
    for (scope, fetch) in [
        (HistoryScope::Project, true),
        (HistoryScope::Session, false),
        (HistoryScope::Everywhere, true),
    ] {
        cx.update_window(window, |_, window, cx| {
            window.press("ctrl-s", cx);
            window.render_frame(cx);
            assert_eq!(view.read(cx).history_scope, scope);
            assert!(view.read(cx).palette.is_some(), "cycling keeps the palette open");
        })
        .unwrap();
        if fetch {
            expect_recent_prompts_request(&mut requests, scope);
        } else {
            assert!(requests.try_recv().is_err(), "session scope sends no request");
        }
    }
}

#[gpui_kit::test]
fn stale_scope_response_tags_cache_without_rebuilding(cx: &mut TestAppContext) {
    use super::super::HistoryScope;
    let (window, view, mut requests) = open_history_with_backend(&[], cx);
    expect_recent_prompts_request(&mut requests, HistoryScope::Everywhere);
    // Switch scope while the everywhere fetch is in flight; the switch
    // refreshes for the new scope.
    cx.update_window(window, |_, window, cx| {
        window.press("ctrl-s", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).history_scope, HistoryScope::Project);
    })
    .unwrap();
    expect_recent_prompts_request(&mut requests, HistoryScope::Project);
    // The late everywhere response caches under its own scope without
    // rebuilding the project palette.
    deliver_recent_prompts(&window, &view, "everywhere", &[("everywhere prompt", "other-session")], cx);
    cx.update_window(window, |_, _, cx| {
        let state = view.read(cx);
        assert_eq!(state.recent_prompts_scope, Some(HistoryScope::Everywhere));
        assert!(state.palette_groups.iter().all(|(_, entries)| entries.is_empty()));
    })
    .unwrap();
    deliver_recent_prompts(&window, &view, "project", &[("project prompt", "project-session")], cx);
    cx.update_window(window, |_, _, cx| {
        let sections = section_contents(&view.read(cx).palette_groups);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].1, vec!["project prompt".to_string()]);
    })
    .unwrap();
}

#[gpui_kit::test]
fn recents_error_stays_silent_and_keeps_thread_rows(cx: &mut TestAppContext) {
    let (window, view, mut requests) = open_history_with_backend(&["thread prompt"], cx);
    expect_recent_prompts_request(&mut requests, super::super::HistoryScope::Everywhere);
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::SESSION_RECENT_PROMPTS.into(),
                    result: None,
                    error: Some("unknown desktop method".into()),
                },
                cx,
            );
        });
    })
    .unwrap();
    cx.update_window(window, |_, _, cx| {
        let state = view.read(cx);
        assert!(state.error.is_none(), "missing server method stays silent");
        assert!(!state.recent_prompts_loading);
        let sections = section_contents(&state.palette_groups);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].1, vec!["thread prompt".to_string()]);
    })
    .unwrap();
}

#[gpui_kit::test]
fn scope_switcher_click_changes_scope_and_fetches(cx: &mut TestAppContext) {
    use super::super::HistoryScope;
    let (window, view, mut requests) = open_history_with_backend(&["thread prompt"], cx);
    expect_recent_prompts_request(&mut requests, HistoryScope::Everywhere);
    cx.update_window(window, |_, window, cx| {
        assert!(window.try_find("history-scope-project").is_some(), "scope switcher offers the project scope");
        window.click("history-scope-project", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).history_scope, HistoryScope::Project);
    })
    .unwrap();
    expect_recent_prompts_request(&mut requests, HistoryScope::Project);
}

#[gpui_kit::test]
fn history_search_matches_non_contiguous_multi_word_query(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let target = "Implement current changes";
    open_history_with_prompts(&window, &view, &["Review stale diffs", target, "cargo test"], cx);
    // Neither token is contiguous in the target: Kit's substring matcher
    // returned zero rows here, which is the divergence from the CLI fixed by
    // the shared nucleo ranker.
    set_history_query(&window, &view, "impl chg", cx);
    cx.update_window(window, |_, _, cx| {
        let matched = view.read(cx).palette.as_ref().expect("palette open").read(cx).matched_count();
        assert_eq!(matched, 1, "fuzzy multi-token search must span the phrase");
        let groups = section_contents(&view.read(cx).palette_groups);
        assert_eq!(groups[0].1, vec![target.to_string()]);
    })
    .unwrap();
}

#[gpui_kit::test]
fn history_empty_query_lists_prompts_newest_first(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    open_history_with_prompts(&window, &view, &["oldest prompt", "middle prompt", "newest prompt"], cx);
    cx.update_window(window, |_, _, cx| {
        let groups = section_contents(&view.read(cx).palette_groups);
        assert_eq!(
            groups[0].1,
            vec![
                "newest prompt".to_string(),
                "middle prompt".to_string(),
                "oldest prompt".to_string()
            ],
            "an empty query keeps the newest-first source order"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn ctrl_r_cycles_through_matches_without_closing(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    open_history_with_prompts(&window, &view, &["alpha prompt", "beta prompt", "gamma prompt"], cx);
    let selected = |view: &Entity<DesktopView>, cx: &gpui_kit::App| {
        view.read(cx).palette.as_ref().and_then(|state| state.read(cx).selected_index())
    };
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        let first = selected(&view, cx);
        window.press("ctrl-r", cx);
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_some(), "ctrl-r cycles instead of closing");
        let second = selected(&view, cx);
        assert_ne!(first, second, "ctrl-r advances the highlight");
        window.press("ctrl-r", cx);
        window.render_frame(cx);
        let third = selected(&view, cx);
        assert_ne!(second, third, "ctrl-r keeps advancing");
    })
    .unwrap();
}

#[gpui_kit::test]
fn ctrl_g_cancels_prompt_history_and_keeps_the_draft(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    open_history_with_prompts(&window, &view, &["alpha prompt", "beta prompt"], cx);
    cx.update_window(window, |_, window, cx| {
        window.press("ctrl-g", cx);
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_none(), "ctrl-g dismisses the history palette");
    })
    .unwrap();
}

#[gpui_kit::test]
fn tab_cycles_history_matches_instead_of_accepting(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    open_history_with_prompts(&window, &view, &["alpha prompt", "beta prompt", "gamma prompt"], cx);
    let selected = |view: &Entity<DesktopView>, cx: &gpui_kit::App| {
        view.read(cx).palette.as_ref().and_then(|state| state.read(cx).selected_index())
    };
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        let first = selected(&view, cx);
        window.press("tab", cx);
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_some(), "tab must not accept the row");
        assert_ne!(first, selected(&view, cx), "tab cycles to the next match");
    })
    .unwrap();
}

#[gpui_kit::test]
fn history_rows_report_hidden_lines(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    open_history_with_prompts(&window, &view, &["single line", "first line\nsecond line\nthird line"], cx);
    cx.update_window(window, |_, _, cx| {
        let labels: Vec<String> = view
            .read(cx)
            .palette_groups
            .iter()
            .flat_map(|(_, entries)| entries.iter().map(|entry| entry.label.clone()))
            .collect();
        assert!(
            labels.iter().any(|label| label.contains("+2 lines")),
            "multi-line prompts advertise hidden lines: {labels:?}"
        );
        assert!(labels.iter().any(|label| label == "single line"), "single-line prompts stay bare");
    })
    .unwrap();
}
