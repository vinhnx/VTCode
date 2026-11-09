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
    open_history_with_draft(window, view, prompts, "", cx);
}

fn open_history_with_draft(
    window: &AnyWindowHandle,
    view: &Entity<DesktopView>,
    prompts: &[&str],
    draft: &str,
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
                state.set_value(draft, window, cx);
                state.focus(window, cx);
            });
            view.move_composer_caret_to_end(window, cx);
        });
        window.render_frame(cx);
        window.press("ctrl-r", cx);
        window.render_frame(cx);
        assert!(matches!(view.read(cx).palette_kind, PaletteKind::PromptHistory));
    })
    .unwrap();
}

/// Type filter text with honest keystrokes, then drive the refresh the
/// Change subscription runs in production (subscription effects flush too
/// late to assert in-test, like the mentions tests).
fn type_history_filter(window: &AnyWindowHandle, view: &Entity<DesktopView>, text: &str, cx: &mut TestAppContext) {
    cx.update_window(*window, |_, window, cx| {
        window.input(text, cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.update_composer_suggestions(window, cx));
        window.render_frame(cx);
    })
    .unwrap();
}

fn set_history_query(window: &AnyWindowHandle, view: &Entity<DesktopView>, query: &str, cx: &mut TestAppContext) {
    cx.update_window(*window, |_, window, cx| {
        // Inline design: the composer text is the filter, so write the draft
        // and drive the refresh the Change subscription runs in production.
        view.update(cx, |view, cx| {
            view.composer.update(cx, |input, cx| input.set_value(query, window, cx));
            view.update_composer_suggestions(window, cx);
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
        // Rows-only panel: Kit's match counter is vestigial, so the host
        // groups are the oracle.
        let groups = section_contents(&view.read(cx).palette_groups);
        assert_eq!(
            groups[0].1,
            vec![target.to_string()],
            "full prompt text must be searchable, not just the title row"
        );
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
        // Rows-only panel: Kit's match counter is vestigial, so the host
        // groups are the oracle.
        let groups = section_contents(&view.read(cx).palette_groups);
        assert_eq!(groups[0].1, vec![target.clone()], "search must cover the full prompt, not the 72-char title");
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

fn selected_row(view: &Entity<DesktopView>, cx: &gpui_kit::App) -> Option<gpui_kit::component::IndexPath> {
    view.read(cx).palette.as_ref().and_then(|state| state.read(cx).selected_index())
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
        // Rows-only panel: Kit's match counter is vestigial, so the host
        // groups are the oracle.
        let groups = section_contents(&view.read(cx).palette_groups);
        assert_eq!(groups[0].1, vec![target.to_string()], "fuzzy multi-token search must span the phrase");
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
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        let first = selected_row(&view, cx);
        window.press("ctrl-r", cx);
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_some(), "ctrl-r cycles instead of closing");
        let second = selected_row(&view, cx);
        assert_ne!(first, second, "ctrl-r advances the highlight");
        window.press("ctrl-r", cx);
        window.render_frame(cx);
        let third = selected_row(&view, cx);
        assert_ne!(second, third, "ctrl-r keeps advancing");
    })
    .unwrap();
}

#[gpui_kit::test]
fn history_palette_filters_from_composer_text(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    open_history_with_prompts(&window, &view, &["alpha prompt", "beta prompt", "gamma prompt"], cx);
    // Reverse search is inline: the composer keeps focus and its text is the
    // filter.
    type_history_filter(&window, &view, "beta", cx);
    cx.update_window(window, |_, _, cx| {
        assert_eq!(
            view.read(cx).composer.read(cx).value().to_string(),
            "beta",
            "the composer keeps focus and holds the typed filter"
        );
        assert_eq!(view.read(cx).palette_query.to_string(), "beta", "the palette query mirrors the draft");
        let groups = section_contents(&view.read(cx).palette_groups);
        assert_eq!(groups[0].1, vec!["beta prompt".to_string()], "the fuzzy ranker narrows the rows to the match");
    })
    .unwrap();
}

#[gpui_kit::test]
fn history_cancel_after_typing_restores_presearch_draft(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    open_history_with_draft(&window, &view, &["alpha prompt", "beta prompt"], "original request", cx);
    // The filter is typed into the composer; abandoning the search must
    // bring back exactly what was there before.
    type_history_filter(&window, &view, "zzz", cx);
    cx.update_window(window, |_, window, cx| {
        assert_eq!(view.read(cx).composer.read(cx).value().to_string(), "original requestzzz");
        window.press("ctrl-g", cx);
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_none(), "ctrl-g dismisses the history palette");
        assert_eq!(
            view.read(cx).composer.read(cx).value().to_string(),
            "original request",
            "cancel restores the pre-search draft verbatim"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn history_arrow_navigation_survives_unchanged_draft_refresh(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    open_history_with_prompts(&window, &view, &["alpha prompt", "beta prompt", "gamma prompt"], cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        let first = selected_row(&view, cx);
        assert!(first.is_some(), "opening selects the first row");
        // Arrow down (as the MoveDown capture does), then the deferred
        // navigation refresh with identical text must not reset the row.
        view.update(cx, |view, cx| {
            assert!(view.move_composer_suggestion(true, window, cx));
        });
        let second = selected_row(&view, cx);
        assert_ne!(first, second, "arrow down must move the highlight");
        view.update(cx, |view, cx| view.update_composer_suggestions(window, cx));
        window.render_frame(cx);
        assert_eq!(selected_row(&view, cx), second, "unchanged draft must not reset the highlight");
    })
    .unwrap();
}

#[gpui_kit::test]
fn history_escape_after_typing_restores_presearch_draft(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    open_history_with_draft(&window, &view, &["Review stale diffs"], "original request", cx);
    type_history_filter(&window, &view, "zzz", cx);
    cx.update_window(window, |_, window, cx| {
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_none(), "escape dismisses the history palette");
        assert_eq!(
            view.read(cx).composer.read(cx).value().to_string(),
            "original request",
            "every dismissal restores the pre-search draft, not just ctrl-g"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn history_empty_then_nonempty_restores_highlight_and_accepts(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    open_history_with_prompts(&window, &view, &["alpha prompt", "beta prompt"], cx);
    // Filter to zero rows, then back to a match — without rendering between,
    // so Kit's installed model is stale-empty (the deferred-highlight input).
    set_history_query(&window, &view, "zzzznomatch", cx);
    cx.update_window(window, |_, window, cx| {
        assert!(section_contents(&view.read(cx).palette_groups)[0].1.is_empty());
        view.update(cx, |view, cx| {
            view.composer.update(cx, |input, cx| input.set_value("beta", window, cx));
            view.update_composer_suggestions(window, cx);
        });
        let palette = view.read(cx).palette.clone().expect("history palette must be open");
        assert_eq!(palette.read(cx).matched_count(), 0, "installed model is stale-empty");
        assert!(!section_contents(&view.read(cx).palette_groups)[0].1.is_empty(), "host rows are back");
        // Arrow against the stale model (the same-frame device race), then
        // render and drive the deferred post-install highlight (`on_next_frame`
        // never runs headless): Enter must accept the row.
        view.update(cx, |view, cx| {
            assert!(view.move_composer_suggestion(true, window, cx));
        });
        window.render_frame(cx);
        view.update(cx, |view, cx| view.select_first_history_match(window, cx));
        window.render_frame(cx);
        window.press("enter", cx);
        window.render_frame(cx);
        assert_eq!(
            view.read(cx).composer.read(cx).value().to_string(),
            "beta prompt",
            "enter after empty-then-match accepts the row"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn history_cmd_enter_submits_presearch_draft_not_filter(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    open_history_with_draft(&window, &view, &["alpha prompt"], "original request", cx);
    type_history_filter(&window, &view, "zzz", cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".to_string());
            view.running = true;
        });
        window.render_frame(cx);
        window.press("cmd-enter", cx);
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_none(), "submit abandons the search");
        let view = view.read(cx);
        assert_eq!(view.queued_submissions.len(), 1);
        assert_eq!(
            view.queued_submissions[0].prompt, "original request",
            "submit reads the restored draft, not the filter text"
        );
    })
    .unwrap();
    let BackendCommand::Request { params, .. } = requests.try_recv().unwrap() else {
        panic!("expected queued turn request");
    };
    assert_eq!(params["batchable"], true);
}

#[gpui_kit::test]
fn history_enter_on_empty_match_submits_presearch_draft(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    open_history_with_draft(&window, &view, &["Review stale diffs"], "original request", cx);
    type_history_filter(&window, &view, "zzz", cx);
    cx.update_window(window, |_, window, cx| {
        // No row matches: Enter closes and submits the restored original,
        // never the filter-polluted draft.
        window.press("enter", cx);
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_none());
        assert_eq!(
            view.read(cx).composer.read(cx).value().to_string(),
            "original request",
            "enter on empty match must not submit filter text"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn history_padded_seed_keeps_arrow_navigation(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    open_history_with_draft(&window, &view, &["alpha prompt", "beta prompt", "gamma prompt"], "  ", cx);
    cx.update_window(window, |_, window, cx| {
        // The mirror holds the untrimmed draft, so a navigation refresh with
        // identical text is a no-op and keeps the moved highlight.
        view.update(cx, |view, cx| {
            assert!(view.move_composer_suggestion(true, window, cx));
        });
        let second = selected_row(&view, cx);
        view.update(cx, |view, cx| view.update_composer_suggestions(window, cx));
        window.render_frame(cx);
        assert_eq!(selected_row(&view, cx), second, "identical padded text must not reset the highlight");
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
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        let first = selected_row(&view, cx);
        window.press("tab", cx);
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_some(), "tab must not accept the row");
        assert_ne!(first, selected_row(&view, cx), "tab cycles to the next match");
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

#[gpui_kit::test]
fn history_rows_stay_within_one_merged_cap(cx: &mut TestAppContext) {
    // Both sources exceed the budget on their own; the merged result must not.
    let live: Vec<String> = (0..250).map(|index| format!("live prompt {index:03}")).collect();
    let refs: Vec<&str> = live.iter().map(String::as_str).collect();
    let (window, view, mut requests) = open_history_with_backend(&refs, cx);
    expect_recent_prompts_request(&mut requests, super::super::HistoryScope::Everywhere);
    let recents: Vec<(String, String)> = (0..250)
        .map(|index| (format!("recent prompt {index:03}"), "other".to_string()))
        .collect();
    let borrowed: Vec<(&str, &str)> = recents
        .iter()
        .map(|(content, session)| (content.as_str(), session.as_str()))
        .collect();
    deliver_recent_prompts(&window, &view, "everywhere", &borrowed, cx);
    cx.update_window(window, |_, _, cx| {
        let sections = section_contents(&view.read(cx).palette_groups);
        let rows: usize = sections.iter().map(|(_, entries)| entries.len()).sum();
        assert_eq!(rows, 200, "the merged result caps at the CLI budget, not per section");
        assert_eq!(sections[0].1.len(), 200, "live rows fill the budget first");
        assert!(sections.iter().all(|(label, entries)| !label.is_empty() && !entries.is_empty()));
    })
    .unwrap();
}

#[gpui_kit::test]
fn unmatched_query_is_not_reported_as_empty_history(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    open_history_with_prompts(&window, &view, &["alpha prompt"], cx);
    set_history_query(&window, &view, "zzzznomatch", cx);
    cx.update_window(window, |_, _, cx| {
        let state = view.read(cx);
        let rows: usize = state.palette_groups.iter().map(|(_, entries)| entries.len()).sum();
        assert_eq!(rows, 0, "the query matches nothing");
        assert!(!state.prompt_history_is_empty(), "history exists; only the filter missed");
    })
    .unwrap();
}

#[gpui_kit::test]
fn empty_catalog_is_reported_as_empty_history(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    // No backend recents and no live prompts: the palette genuinely has none.
    open_history_with_prompts(&window, &view, &[], cx);
    cx.update_window(window, |_, _, cx| {
        assert!(view.read(cx).prompt_history_is_empty());
    })
    .unwrap();
}
