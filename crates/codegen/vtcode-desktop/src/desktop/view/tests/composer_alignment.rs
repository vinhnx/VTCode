use super::*;
use gpui_kit::component::theme::Theme;

/// Zen without a workspace (the reported screenshot): the context row above
/// the input and the button row below it must share one leading spine with
/// the input text. Button frames sit on the spine; same-size ghost buttons
/// carry identical internal padding, so their labels follow.
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
    // UI base sizes, where rem-based group padding and the fixed-px input
    // token diverge most.
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

            // The row consumes the frame's own content inset: the zen group
            // pad (pl_3) plus the size input token below, so its buttons land
            // on the input spine instead of the panel edge.
            let group_pad = px(base_size * 0.75);
            assert!(
                ((context.origin.x - frame.origin.x) - group_pad).abs() <= px(1.),
                "context row sits one group pad inside the frame at {base_size}px: context={context:?} frame={frame:?}"
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
