use super::super::PaletteAction;
use super::*;

use crate::desktop::inspector::InspectorTab;

#[gpui_kit::test]
fn transcript_arrows_navigate_messages_reveal_actions_and_escape_returns_to_composer(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            for (id, role, label) in [
                ("nav-user-1", "user", "You"),
                ("nav-assistant-1", "assistant", "VT Code"),
                ("nav-assistant-2", "assistant", "VT Code"),
            ] {
                view.transcript
                    .push(TranscriptItem::new(id.into(), role.into(), label.into(), format!("Body {id}")));
            }
        });
        window.render_frame(cx);
        focus_element_by_tab(window, "transcript-scroll", cx);
        assert!(view.read(cx).transcript_focus.is_focused(window));
        let row = window.find("transcript-row-nav-assistant-1").bounds();
        window.press("down", cx);
        assert_eq!(view.read(cx).keyboard_focused_message.as_deref(), Some("nav-user-1"));
        window.press("down", cx);
        assert_eq!(view.read(cx).keyboard_focused_message.as_deref(), Some("nav-assistant-1"));
        window.render_frame(cx);
        assert!(
            window.try_find("copy-message-nav-assistant-1").is_some(),
            "highlight reveals row actions like hover"
        );
        assert!(
            window.try_find("transcript-focus-rail-nav-assistant-1").is_some(),
            "highlight paints a visible rail"
        );
        assert_eq!(window.find("transcript-row-nav-assistant-1").bounds(), row, "highlight must not reflow the row");
        window.press("down", cx);
        assert_eq!(view.read(cx).keyboard_focused_message.as_deref(), Some("nav-assistant-2"));
        window.press("down", cx);
        assert_eq!(
            view.read(cx).keyboard_focused_message.as_deref(),
            Some("nav-assistant-2"),
            "navigation clamps at the last message"
        );
        window.press("up", cx);
        assert_eq!(view.read(cx).keyboard_focused_message.as_deref(), Some("nav-assistant-1"));
        window.render_frame(cx);
        assert!(window.try_find("copy-message-nav-assistant-2").is_none(), "reveal follows the highlight");
        window.press("up", cx);
        window.press("up", cx);
        assert_eq!(view.read(cx).keyboard_focused_message.as_deref(), Some("nav-user-1"));
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(
            view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window),
            "escape returns to the composer"
        );
        assert_eq!(
            view.read(cx).keyboard_focused_message.as_deref(),
            Some("nav-user-1"),
            "escape keeps the highlight as position memory"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn transcript_nav_skips_collapsed_activity_members(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            for (id, hidden) in [("nav-first", false), ("nav-hidden", true), ("nav-last", false)] {
                let mut item =
                    TranscriptItem::new(id.into(), "assistant".into(), "VT Code".into(), format!("Body {id}"));
                if hidden {
                    item.activity_grouped = true;
                    item.activity_group_head = false;
                    item.activity_expanded = false;
                }
                view.transcript.push(item);
            }
        });
        window.render_frame(cx);
        view.update(cx, |view, cx| view.transcript_focus.focus(window, cx));
        window.press("down", cx);
        assert_eq!(view.read(cx).keyboard_focused_message.as_deref(), Some("nav-first"));
        window.press("down", cx);
        assert_eq!(
            view.read(cx).keyboard_focused_message.as_deref(),
            Some("nav-last"),
            "collapsed members are skipped"
        );
        view.update(cx, |view, _| view.keyboard_focused_message = None);
        window.press("up", cx);
        assert_eq!(
            view.read(cx).keyboard_focused_message.as_deref(),
            Some("nav-last"),
            "up from nothing starts at the last visible message"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn transcript_row_click_anchors_highlight_without_moving_the_viewport(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            for (id, role) in [
                ("click-user", "user"),
                ("click-first", "assistant"),
                ("click-second", "assistant"),
            ] {
                view.transcript.push(TranscriptItem::new(
                    id.into(),
                    role.into(),
                    "VT Code".into(),
                    format!("Body {id}"),
                ));
            }
        });
        window.render_frame(cx);
        let scroller = view.read(cx).message_scroller.clone();
        let before = scroller.read(cx).logical_scroll_top();
        let bounds = window.find("transcript-row-click-first").bounds();
        window.click("transcript-row-click-first", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).keyboard_focused_message.as_deref(), Some("click-first"));
        assert!(window.try_find("transcript-focus-rail-click-first").is_some(), "click paints the rail");
        assert_eq!(window.find("transcript-row-click-first").bounds(), bounds, "anchor must not reflow the row");
        let after = scroller.read(cx).logical_scroll_top();
        assert_eq!(after.item_ix, before.item_ix, "clicking a visible row must not yank the viewport");
        assert!((after.offset_in_item - before.offset_in_item).abs() <= px(1.));
        // A second click moves the anchor: different target, different rail.
        window.click("transcript-row-click-second", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).keyboard_focused_message.as_deref(), Some("click-second"));
        assert!(window.try_find("transcript-focus-rail-click-first").is_none());
        assert!(window.try_find("transcript-focus-rail-click-second").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn scroll_first_button_appears_off_top_and_returns_to_the_first_row(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let scroller = cx.update(|cx| {
        view.update(cx, |view, _| {
            for index in 0..40 {
                view.transcript.push(TranscriptItem::new(
                    format!("first-{index:02}"),
                    "assistant".into(),
                    "VT Code".into(),
                    format!("Body {index}"),
                ));
            }
            view.message_scroller.clone()
        })
    });
    cx.update_window(window, |_, window, cx| {
        // Two frames: the first settles list layout, the second paints the
        // overlay that waits for it.
        window.render_frame(cx);
        window.render_frame(cx);
        // Fresh threads follow the tail, so the top edge sits above the viewport.
        assert!(scroller.read(cx).logical_scroll_top().item_ix > 0);
        let button = window.find("transcript-scroll-first");
        assert_eq!(button.role(), Some(gpui_kit::Role::Button));
        assert_eq!(button.label(), Some("Scroll to first message"));
        window.click("transcript-scroll-first", cx);
        window.render_frame(cx);
        assert_eq!(scroller.read(cx).logical_scroll_top().item_ix, 0);
        assert!(window.try_find("transcript-scroll-first").is_none(), "button hides at the top edge");
        // Scrolling back down restores it: both sides of the boundary.
        scroller.update(cx, |state, cx| assert!(state.scroll_to_item(20, cx)));
        window.render_frame(cx);
        assert!(window.try_find("transcript-scroll-first").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn scroll_first_button_absent_without_rows(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("transcript-scroll-first").is_none());
        assert!(!view.update(cx, |view, cx| view.scroll_to_first_message(cx)));
    })
    .unwrap();
}

#[gpui_kit::test]
fn activity_rows_jump_to_live_messages_and_hide_for_restored_history(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let session_id = "jump-thread".to_string();
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.selected_session_id = Some(session_id.clone());
            // Target sits mid-list with ample rows below, so pin-to-top can
            // land exactly (a target near the tail would clamp to the last
            // full viewport instead).
            for index in 0..10 {
                view.transcript.push(TranscriptItem::new(
                    format!("pad-{index:02}"),
                    "assistant".into(),
                    "VT Code".into(),
                    format!("Filler {index}"),
                ));
            }
            view.transcript.push(TranscriptItem::new(
                "deploy-live".into(),
                "tool".into(),
                "Run tests".into(),
                "failed (exit 1)\nred output".into(),
            ));
            for index in 10..31 {
                view.transcript.push(TranscriptItem::new(
                    format!("pad-{index:02}"),
                    "assistant".into(),
                    "VT Code".into(),
                    format!("Filler {index}"),
                ));
            }
            view.transcript.push(TranscriptItem::new(
                "after".into(),
                "assistant".into(),
                "VT Code".into(),
                "Trailing message.".into(),
            ));
            // Restored-only history: older than the loaded window, so the
            // activity list shows it but no transcript jump can resolve.
            view.push_restored_activity(TranscriptItem::new(
                "deploy-old".into(),
                "tool".into(),
                "Old deploy".into(),
                "failed (exit 2)\narchived output".into(),
            ));
            view.toggle_activity_filter(ActivityState::Failed);
            view.open_inspector_tab(InspectorTab::Activity, cx);
        });
        window.render_frame(cx);
        assert!(window.try_find("activity-deploy-live").is_some(), "filtered list keeps the live row");
        let jump = window.find("activity-jump-deploy-live");
        assert_eq!(jump.role(), Some(gpui_kit::Role::Button));
        assert_eq!(jump.label(), Some("Jump to message for Run tests"));
        assert!(window.try_find("activity-deploy-old").is_some(), "restored row renders in the filtered list");
        assert!(window.try_find("activity-jump-deploy-old").is_none(), "restored-only rows offer no transcript jump");
        window.click("activity-jump-deploy-live", cx);
        window.render_frame(cx);
        assert_eq!(
            view.read(cx).keyboard_focused_message.as_deref(),
            Some("deploy-live"),
            "click anchors synchronously"
        );
        // The jump scrolls synchronously; one frame renders the result.
        let scroller = view.read(cx).message_scroller.clone();
        assert_eq!(scroller.read(cx).logical_scroll_top().item_ix, 10, "jump lands on the live row, not the tail");
        assert!(window.try_find("transcript-row-deploy-live").is_some(), "target row renders");
        assert!(window.try_find("transcript-focus-rail-deploy-live").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn filter_match_pill_cycles_search_matches_and_wraps(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let session_id = "filter-thread".to_string();
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.selected_session_id = Some(session_id.clone());
            for index in 0..5 {
                view.transcript.push(TranscriptItem::new(
                    format!("pad-{index:02}"),
                    "assistant".into(),
                    "VT Code".into(),
                    format!("Filler {index}"),
                ));
            }
            view.transcript.push(TranscriptItem::new(
                "needle-user".into(),
                "user".into(),
                "You".into(),
                "First needle here.".into(),
            ));
            for index in 5..10 {
                view.transcript.push(TranscriptItem::new(
                    format!("pad-{index:02}"),
                    "assistant".into(),
                    "VT Code".into(),
                    format!("Filler {index}"),
                ));
            }
            view.transcript.push(TranscriptItem::new(
                "needle-assistant".into(),
                "assistant".into(),
                "VT Code".into(),
                "Second needle here.".into(),
            ));
            for index in 10..15 {
                view.transcript.push(TranscriptItem::new(
                    format!("pad-{index:02}"),
                    "assistant".into(),
                    "VT Code".into(),
                    format!("Filler {index}"),
                ));
            }
            view.transcript.push(TranscriptItem::new(
                "needle-tool".into(),
                "tool".into(),
                "Run needle checks".into(),
                "failed (exit 1)\nred output".into(),
            ));
            for index in 15..25 {
                view.transcript.push(TranscriptItem::new(
                    format!("pad-{index:02}"),
                    "assistant".into(),
                    "VT Code".into(),
                    format!("Filler {index}"),
                ));
            }
        });
        window.render_frame(cx);
        window.render_frame(cx);
        // Land on the middle match: the pill anchors at 2/3, the first cycle
        // step exercises the tool reveal path, the second proves wrapping.
        view.update(cx, |view, cx| {
            view.palette_query = "needle".into();
            view.run_palette_action(
                PaletteAction::JumpToTranscriptItem {
                    session_id: session_id.clone(),
                    item_id: "needle-assistant".to_string(),
                    from_search: true,
                },
                window,
                cx,
            );
        });
        window.render_frame(cx);
        let scroller = view.read(cx).message_scroller.clone();
        assert_eq!(scroller.read(cx).logical_scroll_top().item_ix, 11);
        assert_eq!(window.find("transcript-filter-next").label(), Some("Next match (2/3)"));
        assert!(window.try_find("transcript-filter-clear").is_some(), "dismiss rides with the pill");
        window.click("transcript-filter-next", cx);
        window.render_frame(cx);
        assert_eq!(scroller.read(cx).logical_scroll_top().item_ix, 17, "cycle reveals and lands on the tool match");
        assert_eq!(view.read(cx).keyboard_focused_message.as_deref(), Some("needle-tool"));
        assert_eq!(window.find("transcript-filter-next").label(), Some("Next match (3/3)"));
        window.click("transcript-filter-next", cx);
        window.render_frame(cx);
        assert_eq!(scroller.read(cx).logical_scroll_top().item_ix, 5, "cycle wraps to the first match");
        assert_eq!(window.find("transcript-filter-next").label(), Some("Next match (1/3)"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn filter_matches_cover_fuzzy_only_picks_and_dismiss_or_reset_clears(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let session_id = "filter-fuzzy".to_string();
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.selected_session_id = Some(session_id.clone());
            view.transcript.push(TranscriptItem::new(
                "needle-one".into(),
                "assistant".into(),
                "VT Code".into(),
                "A needle in the text.".into(),
            ));
            view.transcript.push(TranscriptItem::new(
                "plain-filler".into(),
                "assistant".into(),
                "VT Code".into(),
                "Nothing relevant.".into(),
            ));
        });
        window.render_frame(cx);
        window.render_frame(cx);
        // A fuzzy-only pick (no substring) still joins the cycle so the
        // landing row is never outside the set the pill describes.
        view.update(cx, |view, cx| {
            view.palette_query = "needle".into();
            view.run_palette_action(
                PaletteAction::JumpToTranscriptItem {
                    session_id: session_id.clone(),
                    item_id: "plain-filler".to_string(),
                    from_search: true,
                },
                window,
                cx,
            );
        });
        window.render_frame(cx);
        assert_eq!(window.find("transcript-filter-next").label(), Some("Next match (2/2)"));
        window.click("transcript-filter-clear", cx);
        window.render_frame(cx);
        assert!(window.try_find("transcript-filter-next").is_none(), "dismiss clears the pill");
        assert!(window.try_find("transcript-filter-clear").is_none(), "dismiss clears itself");
        assert!(view.read(cx).filter_matches.is_none());
        // Non-search jumps never seed the cycle, even with a stale query.
        view.update(cx, |view, cx| {
            view.palette_query = "needle".into();
            view.run_palette_action(
                PaletteAction::JumpToTranscriptItem {
                    session_id: session_id.clone(),
                    item_id: "needle-one".to_string(),
                    from_search: false,
                },
                window,
                cx,
            );
        });
        window.render_frame(cx);
        assert!(window.try_find("transcript-filter-next").is_none(), "deeplinks seed no filter");
        // Session reset clears a live cycle.
        view.update(cx, |view, cx| {
            view.palette_query = "needle".into();
            view.run_palette_action(
                PaletteAction::JumpToTranscriptItem {
                    session_id: session_id.clone(),
                    item_id: "needle-one".to_string(),
                    from_search: true,
                },
                window,
                cx,
            );
        });
        window.render_frame(cx);
        assert!(window.try_find("transcript-filter-next").is_some());
        view.update(cx, |view, _| view.reset_session_state());
        assert!(view.read(cx).filter_matches.is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn filter_cycle_skips_evicted_matches_and_clears_when_empty(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let session_id = "filter-evict".to_string();
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.selected_session_id = Some(session_id.clone());
            view.transcript.push(TranscriptItem::new(
                "needle-alpha".into(),
                "user".into(),
                "You".into(),
                "Alpha needle.".into(),
            ));
            for index in 0..1998 {
                view.transcript.push(TranscriptItem::new(
                    format!("pad-{index:04}"),
                    "assistant".into(),
                    "VT Code".into(),
                    format!("Filler {index}"),
                ));
            }
            view.transcript.push(TranscriptItem::new(
                "needle-beta".into(),
                "user".into(),
                "You".into(),
                "Beta needle.".into(),
            ));
        });
        window.render_frame(cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| {
            view.palette_query = "needle".into();
            view.run_palette_action(
                PaletteAction::JumpToTranscriptItem {
                    session_id: session_id.clone(),
                    item_id: "needle-alpha".to_string(),
                    from_search: true,
                },
                window,
                cx,
            );
        });
        window.render_frame(cx);
        assert_eq!(window.find("transcript-filter-next").label(), Some("Next match (1/2)"));
        // Evict the first match; the cycle must skip it and land on beta.
        view.update(cx, |view, _| {
            for index in 0..5 {
                view.transcript.push(TranscriptItem::new(
                    format!("tail-{index}"),
                    "assistant".into(),
                    "VT Code".into(),
                    format!("Tail {index}"),
                ));
            }
        });
        window.click("transcript-filter-next", cx);
        window.render_frame(cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).keyboard_focused_message.as_deref(), Some("needle-beta"));
        assert_eq!(window.find("transcript-filter-next").label(), Some("Next match (2/2)"));
        // Evict everything; the cycle clears itself instead of stranding.
        view.update(cx, |view, _| {
            for index in 0..2000 {
                view.transcript.push(TranscriptItem::new(
                    format!("flood-{index:04}"),
                    "assistant".into(),
                    "VT Code".into(),
                    format!("Flood {index}"),
                ));
            }
        });
        window.click("transcript-filter-next", cx);
        window.render_frame(cx);
        assert!(window.try_find("transcript-filter-next").is_none(), "empty cycle clears the pill");
        assert!(view.read(cx).filter_matches.is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn jump_to_unknown_message_leaves_state_untouched(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.transcript.push(TranscriptItem::new(
                "only".into(),
                "assistant".into(),
                "VT Code".into(),
                "Body".into(),
            ));
        });
        window.render_frame(cx);
        let jumped = view.update(cx, |view, cx| view.jump_to_transcript_item("missing-id", window, cx));
        assert!(!jumped);
        assert!(view.read(cx).keyboard_focused_message.is_none(), "failed jumps set no anchor");
    })
    .unwrap();
}
