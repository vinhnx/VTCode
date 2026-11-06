use super::*;
use gpui_kit::component::theme::Theme;

/// Zen without a workspace (the reported screenshot): the context row above
/// the input and the button row below it must share one leading spine with
/// the input text, which itself sits on the transcript spine. Button frames
/// sit on the spine; same-size ghost buttons carry identical internal
/// padding, so their labels follow.
#[gpui_kit::test]
fn composer_rows_share_one_leading_spine_in_zen_without_workspace(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = None;
    });
    cx.update_window(window, |_, window, cx| {
        window.press("ctrl-cmd-z", cx);
        window.render_frame(cx);
        assert!(
            window.try_find("composer-context").is_some(),
            "zen without a workspace shows the context row above the input"
        );
    })
    .unwrap();

    // Asymmetric extremes: the spine must hold at the smallest and largest
    // UI base sizes, where rem-based wrapper insets and fixed-px Kit text
    // metrics diverge most.
    for base_size in [14., 24.] {
        cx.update(|cx| {
            Theme::update(cx, |theme| {
                theme.font_size = px(base_size);
            });
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let context = window.find("composer-context").bounds();
            let frame = window.find("message-composer-frame").bounds();
            let cluster = window.find("zen-composer-context").bounds();
            let workspace_button = window.find("composer-workspace").bounds();
            let model_button = window.find("pick-model").bounds();
            let mode_button = window.find("composer-mode").bounds();
            let policy_button = window.find("composer-policy").bounds();
            let policy_label = window.find("composer-policy-label").bounds();
            let send = window.find("send-prompt").bounds();

            // The frame carries no horizontal inset, so the context row
            // starts on the frame edge and its buttons land on the input
            // spine directly.
            assert!(
                (context.origin.x - frame.origin.x).abs() <= px(1.),
                "context row starts on the frame edge at {base_size}px: context={context:?} frame={frame:?}"
            );
            assert!(
                (workspace_button.origin.x - model_button.origin.x).abs() <= px(1.),
                "context-row buttons start on the button-row spine at {base_size}px: workspace={workspace_button:?} model={model_button:?}"
            );
            let leading = cluster.origin.x - frame.origin.x;
            let trailing = (frame.origin.x + frame.size.width) - (send.origin.x + send.size.width);
            assert!(
                (trailing - leading).abs() <= px(1.),
                "send keeps a symmetric trailing lane at {base_size}px: leading={leading:?} trailing={trailing:?} frame={frame:?} send={send:?}"
            );
            for (name, button) in
                [("mode", mode_button), ("policy", policy_button), ("send", send)]
            {
                let drift = (button.center().y - model_button.center().y).abs();
                assert!(
                    drift <= px(1.),
                    "button row shares one center line at {base_size}px: {name} drifts {drift:?} from model: model={model_button:?} other={button:?}"
                );
            }
            let ghost_pad = policy_label.origin.x - policy_button.origin.x;
            assert!(
                ghost_pad >= px(0.),
                "policy label sits inside its button frame at {base_size}px: pad={ghost_pad:?} label={policy_label:?} button={policy_button:?}"
            );
        })
        .unwrap();
    }
}

/// The reported screenshot: the pending `Running` label and the composer
/// input sat right of the transcript body. Borderless text across the
/// transcript body, pending row, thread header, and composer frame must
/// share one leading spine in both modes. The assistant body is Kit-owned
/// geometry this change does not touch, so it is the independent
/// reference; ghost button labels intentionally stay inset by button
/// padding (matching transcript tool rows), only frames join the spine.
#[gpui_kit::test]
fn transcript_text_and_chrome_hold_their_spines(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    running_turn_without_content(cx, &view);
    view.update(cx, |view, _| {
        view.transcript.push(TranscriptItem::new(
            "spine-answer".into(),
            "assistant".into(),
            "VT Code".into(),
            "Sharing the spine.".into(),
        ));
        view.transcript.push(TranscriptItem::new(
            "spine-tool".into(),
            "tool".into(),
            "Read Cargo.toml".into(),
            "completed\nmanifest".into(),
        ));
    });

    // Asymmetric extremes: rem-based Kit insets scale with the base size
    // while the column caps stay fixed, so drift shows most at the ends.
    for base_size in [14., 24.] {
        cx.update(|cx| {
            Theme::update(cx, |theme| {
                theme.font_size = px(base_size);
            });
        });
        // Each zoom level re-arms the running turn the idle assertions
        // below complete, so the pending row is up in both iterations.
        // Clearing the finished id keeps the repeat completion below
        // from tripping the stale-turn guard.
        view.update(cx, |view, _| {
            view.running = true;
            view.turn_content_seen = false;
            view.finished_turn_id = None;
        });
        // Normal mode: header, pending row, and full button row visible.
        // While the turn runs, the header row starts with the running
        // indicator on the spine and the title follows after it; once
        // idle, the title itself starts on the spine.
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let body = window.find("message-body-spine-answer").bounds();
            let tool_title = window.find("tool-title-spine-tool").bounds();
            let pending = window.find("turn-pending-label").bounds();
            let running_mark = window.find("thread-header-running").bounds();
            let frame = window.find("message-composer-frame").bounds();
            let attach = window.find("attach-files").bounds();
            let policy = window.find("composer-policy").bounds();
            let policy_label = window.find("composer-policy-label").bounds();
            // Chrome shares one leading edge; transcript text insets to the
            // header title spine. Only the row-leading button joins the
            // chrome spine; siblings follow after it with row gaps (center
            // line pinned below by the button-row test).
            for (name, other) in [("pending", pending), ("running", running_mark), ("attach", attach)] {
                assert!(
                    (other.origin.x - frame.origin.x).abs() <= px(1.),
                    "{name} starts on the chrome spine at {base_size}px: frame={frame:?} other={other:?}"
                );
            }
            assert!(
                (body.origin.x - tool_title.origin.x).abs() <= px(1.),
                "assistant text starts on the header title spine at {base_size}px: body={body:?} title={tool_title:?}"
            );
            assert!(
                (frame.center().x - body.center().x).abs() <= px(1.),
                "composer stays centered on the transcript column at {base_size}px: frame={frame:?} body={body:?}"
            );
            assert!(
                policy_label.origin.x > policy.origin.x,
                "ghost labels stay inset by button padding at {base_size}px: label={policy_label:?} button={policy:?}"
            );

            view.update(cx, |view, cx| {
                view.handle_backend_event(turn_completed_event("session", "turn-1"), cx);
                assert!(!view.running);
            });
            window.render_frame(cx);
            assert!(window.try_find("turn-pending").is_none());
            assert!(window.try_find("thread-header-running").is_none());
            let idle_frame = window.find("message-composer-frame").bounds();
            let idle_title = window.find("thread-title").bounds();
            assert!(
                (idle_title.origin.x - idle_frame.origin.x).abs() <= px(1.),
                "idle title starts on the chrome spine at {base_size}px: frame={idle_frame:?} title={idle_title:?}"
            );
        })
        .unwrap();

        // Zen mode: narrower column, header and attach action hidden.
        cx.update_window(window, |_, window, cx| {
            view.update(cx, |view, _| {
                view.running = true;
                view.turn_content_seen = false;
                view.finished_turn_id = None;
            });
            window.press("ctrl-cmd-z", cx);
            window.render_frame(cx);
            assert!(window.try_find("thread-header").is_none());
            let body = window.find("message-body-spine-answer").bounds();
            let tool_title = window.find("tool-title-spine-tool").bounds();
            let pending = window.find("turn-pending-label").bounds();
            let frame = window.find("message-composer-frame").bounds();
            assert!(
                (pending.origin.x - frame.origin.x).abs() <= px(1.),
                "zen pending starts on the chrome spine at {base_size}px: frame={frame:?} pending={pending:?}"
            );
            assert!(
                (body.origin.x - tool_title.origin.x).abs() <= px(1.),
                "zen assistant text starts on the header title spine at {base_size}px: body={body:?} title={tool_title:?}"
            );
            assert!(
                (frame.center().x - body.center().x).abs() <= px(1.),
                "zen composer stays centered on the transcript column at {base_size}px: frame={frame:?} body={body:?}"
            );
            window.press("ctrl-cmd-z", cx);
            window.render_frame(cx);
            assert!(window.try_find("thread-header").is_some());
        })
        .unwrap();
    }
}

/// Normal mode with a workspace: the option group, model cluster, and send
/// action share one center line, and the send action keeps a symmetric
/// trailing lane against the leading option group. Sidebar and inspector
/// stay closed (their production defaults), so the column fits the row
/// without wrapping even at the largest zoom; the wrapped narrow-column
/// layout is responsive fallback, not the aligned state pinned here.
#[gpui_kit::test]
fn composer_button_row_stays_aligned_in_normal_mode(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.sidebar_open = false;
        view.inspector_open = false;
    });
    for base_size in [14., 24.] {
        cx.update(|cx| {
            Theme::update(cx, |theme| {
                theme.font_size = px(base_size);
            });
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let frame = window.find("message-composer-frame").bounds();
            let attach = window.find("attach-files").bounds();
            let model_button = window.find("pick-model").bounds();
            let mode_button = window.find("composer-mode").bounds();
            let policy_button = window.find("composer-policy").bounds();
            let send = window.find("send-prompt").bounds();
            let frame_right = frame.origin.x + frame.size.width;

            if (model_button.origin.y - attach.origin.y).abs() <= px(1.) {
                for (name, button) in
                    [("attach", attach), ("mode", mode_button), ("policy", policy_button), ("send", send)]
                {
                    let drift = (button.center().y - model_button.center().y).abs();
                    assert!(
                        drift <= px(1.),
                        "button row shares one center line at {base_size}px: {name} drifts {drift:?} from model: frame={frame:?} attach={attach:?} mode={mode_button:?} policy={policy_button:?} model={model_button:?} send={send:?}"
                    );
                }
                let leading = attach.origin.x - frame.origin.x;
                let trailing = frame_right - (send.origin.x + send.size.width);
                assert!(
                    (trailing - leading).abs() <= px(1.),
                    "send keeps a symmetric trailing lane at {base_size}px: leading={leading:?} trailing={trailing:?} frame={frame:?} send={send:?}"
                );
            } else {
                // Wrapped fallback (largest zoom): both lines restart on the
                // leading spine, send rides the model's line, and nothing
                // escapes the frame.
                assert!(
                    (model_button.origin.x - attach.origin.x).abs() <= px(1.),
                    "wrapped lines share the leading spine at {base_size}px: frame={frame:?} attach={attach:?} model={model_button:?}"
                );
                let send_drift = (send.center().y - model_button.center().y).abs();
                assert!(
                    send_drift <= px(1.),
                    "send rides the model's wrapped line at {base_size}px: drift={send_drift:?} model={model_button:?} send={send:?}"
                );
                for (name, button) in
                    [("attach", attach), ("mode", mode_button), ("policy", policy_button), ("send", send)]
                {
                    let right = button.origin.x + button.size.width;
                    assert!(
                        right <= frame_right + px(1.),
                        "{name} stays inside the frame at {base_size}px: right={right:?} frame={frame:?} button={button:?}"
                    );
                }
            }
        })
        .unwrap();
    }
}
