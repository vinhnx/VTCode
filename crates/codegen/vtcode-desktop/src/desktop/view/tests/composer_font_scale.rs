use super::*;

/// Composer labels must track the UI font-size setting: every text-bearing
/// element in the chat input grows from the smallest to the largest base
/// size. Asymmetric extremes (14 vs 24) pin both ends of the supported
/// range, not just the default.
#[gpui_kit::test]
fn composer_labels_scale_with_the_ui_font_size(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    view.update(cx, |view, _| {
        view.backend = Some(BackendHandle { commands });
        view.configuration = Some(runtime_options_config());
        view.attachments
            .push(vtcode_desktop_protocol::TurnAttachment { path: "notes.txt".into(), text: "hello".into() });
        view.first_submission = Some(super::super::SubmissionDraft {
            prompt: "waiting draft".into(),
            attachments: Vec::new(),
            echo_id: "echo-font-scale".into(),
            delivery_uncertain: false,
        });
    });

    // Always-mounted composer ids: label rows plus their owning buttons, so
    // both the text and its control scale. The attachment remove button has
    // no text of its own; its padding still tracks the base size.
    let base_ids = [
        "pick-model",
        "composer-model-effort",
        "composer-model-speed",
        "composer-policy-label",
        "composer-mode-label",
        "startup-send-status",
        "send-prompt",
    ];
    let remove_attachment = opaque_path_element_id("remove-attachment", "notes.txt");
    let mut small = Vec::new();
    let mut large = Vec::new();
    for (size, store) in [(14u8, &mut small), (24u8, &mut large)] {
        view.update(cx, |view, cx| {
            view.preferences.ui_font_size = Some(size);
            view.apply_typography(cx);
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(cx.theme().font_size, px(f32::from(size)));
            store.extend(base_ids.iter().map(|id| window.find(*id).bounds().size.height));
            store.push(window.find(remove_attachment.clone()).bounds().size.height);
            let button_bounds = window.find("pick-model").bounds();
            for id in ["composer-model-effort", "composer-model-speed"] {
                let bounds = window.find(id).bounds();
                assert!(
                    bounds.left() >= button_bounds.left() && bounds.right() <= button_bounds.right(),
                    "{id} must stay inside the model button at {size}px"
                );
            }
        })
        .unwrap();
    }
    for (index, id) in base_ids.iter().enumerate() {
        assert!(
            large[index] > small[index],
            "{id} must grow with the UI font size ({} at 14px vs {} at 24px)",
            small[index].as_f32(),
            large[index].as_f32()
        );
    }
    assert!(large[base_ids.len()] > small[base_ids.len()], "remove-attachment must grow with the UI font size");

    // Composer-anchored popup labels scale too: open the model picker once,
    // then resize under it. The waiting-send draft (already measured above)
    // disables the picker, so clear it first.
    view.update(cx, |view, _| {
        view.first_submission = None;
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("pick-model", cx);
        window.render_frame(cx);
        assert!(window.try_find("model-options-summary").is_some());
    })
    .unwrap();
    let popup_ids = ["model-options-summary", "model-selected-name", "model-effort-value"];
    let mut popup_small = Vec::new();
    let mut popup_large = Vec::new();
    for (size, store) in [(14u8, &mut popup_small), (24u8, &mut popup_large)] {
        view.update(cx, |view, cx| {
            view.preferences.ui_font_size = Some(size);
            view.apply_typography(cx);
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            store.extend(popup_ids.iter().map(|id| window.find(*id).bounds().size.height));
        })
        .unwrap();
    }
    for (index, id) in popup_ids.iter().enumerate() {
        assert!(
            popup_large[index] > popup_small[index],
            "{id} must grow with the UI font size ({} at 14px vs {} at 24px)",
            popup_small[index].as_f32(),
            popup_large[index].as_f32()
        );
    }
}

/// The input text itself must track the UI font size. Three lines exceed the
/// 32px input minimum at both extremes, so the frame height is content-driven.
#[gpui_kit::test]
fn composer_input_text_scales_with_the_ui_font_size(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let mut small = Vec::new();
    let mut large = Vec::new();
    for (size, store) in [(14u8, &mut small), (24u8, &mut large)] {
        view.update(cx, |view, cx| {
            view.preferences.ui_font_size = Some(size);
            view.apply_typography(cx);
        });
        cx.update_window(window, |_, window, cx| {
            view.update(cx, |view, cx| {
                view.composer
                    .update(cx, |input, cx| input.set_value("line one\nline two\nline three", window, cx));
            });
            window.render_frame(cx);
            store.push(window.find("message-composer-frame").bounds().size.height);
        })
        .unwrap();
    }
    assert!(
        large[0] > small[0],
        "message-composer-frame must grow with the UI font size ({} at 14px vs {} at 24px)",
        small[0].as_f32(),
        large[0].as_f32()
    );
}

/// Composer-anchored popup rows (the model list here; slash commands and
/// file mentions share the same Kit `Command` list) must track the UI font
/// size even when the popup is already open: resize under the open popup to
/// catch snapshotted row measurements as well as fixed text.
#[gpui_kit::test]
fn composer_popup_rows_scale_with_the_ui_font_size(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    view.update(cx, |view, _| {
        view.backend = Some(BackendHandle { commands });
        view.configuration = Some(runtime_options_config());
        view.models = vec![vtcode_desktop_protocol::ModelInfo {
            provider: "anthropic".into(),
            id: "claude-sonnet-5".into(),
            display_name: "Claude Sonnet 5".into(),
            context_window: Some(200_000),
            current: false,
            recommended_rank: Some(0),
            capabilities: None,
        }];
    });
    let row = palette_action_id(&super::super::PaletteAction::SelectModel {
        provider: "anthropic".into(),
        model: "claude-sonnet-5".into(),
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("pick-model", cx);
        window.render_frame(cx);
        window.click("model-search-all", cx);
        window.render_frame(cx);
        assert!(window.try_find(row.clone()).is_some(), "full model search lists the seeded model");
    })
    .unwrap();
    let mut small = None;
    let mut large = None;
    for (size, store) in [(14u8, &mut small), (24u8, &mut large)] {
        view.update(cx, |view, cx| {
            view.preferences.ui_font_size = Some(size);
            view.apply_typography(cx);
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            *store = Some(window.find(row.clone()).bounds().size.height);
        })
        .unwrap();
    }
    let (small, large) = (small.unwrap(), large.unwrap());
    assert!(
        large > small,
        "composer popup rows must grow with the UI font size ({} at 14px vs {} at 24px)",
        small.as_f32(),
        large.as_f32()
    );
}

/// Transcript prose tracks the UI size while fenced code tracks the code
/// size, independently: growing one setting must not move surfaces owned by
/// the other. The user bubble is pure prose; the assistant answer mixes one
/// prose line with a three-line code fence.
#[gpui_kit::test]
fn transcript_prose_and_code_track_their_own_font_sizes(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.transcript.push(TranscriptItem::new(
            "probe-user".into(),
            "user".into(),
            "You".into(),
            "first line\nsecond line\nthird line".into(),
        ));
        view.transcript.push(TranscriptItem::new(
            "probe-assistant".into(),
            "assistant".into(),
            "Assistant".into(),
            "Here is the patch:\n```rust\nlet a = 1;\nlet b = 2;\nlet c = 3;\n```".into(),
        ));
    });

    // UI size moves prose (both bubbles contain UI-sized lines).
    let mut prose_small = None;
    let mut prose_large = None;
    for (size, store) in [(14u8, &mut prose_small), (24u8, &mut prose_large)] {
        view.update(cx, |view, cx| {
            view.preferences.ui_font_size = Some(size);
            view.preferences.code_font_size = Some(14);
            view.apply_typography(cx);
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            *store = Some(window.find("message-body-probe-user").bounds().size.height);
        })
        .unwrap();
    }
    let (prose_small, prose_large) = (prose_small.unwrap(), prose_large.unwrap());
    assert!(
        prose_large > prose_small,
        "transcript prose must grow with the UI font size ({} at 14px vs {} at 24px)",
        prose_small.as_f32(),
        prose_large.as_f32()
    );

    // Code size moves the fenced block but leaves pure prose untouched.
    let mut code_small = None;
    let mut code_large = None;
    let mut control_small = None;
    let mut control_large = None;
    for (size, code_store, control_store) in [
        (11u8, &mut code_small, &mut control_small),
        (24u8, &mut code_large, &mut control_large),
    ] {
        view.update(cx, |view, cx| {
            view.preferences.ui_font_size = Some(16);
            view.preferences.code_font_size = Some(size);
            view.apply_typography(cx);
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            *code_store = Some(window.find("message-body-probe-assistant").bounds().size.height);
            *control_store = Some(window.find("message-body-probe-user").bounds().size.height);
        })
        .unwrap();
    }
    let (code_small, code_large) = (code_small.unwrap(), code_large.unwrap());
    assert!(
        code_large > code_small,
        "fenced code must grow with the code font size ({} at 11px vs {} at 24px)",
        code_small.as_f32(),
        code_large.as_f32()
    );
    assert_eq!(control_small.unwrap(), control_large.unwrap(), "pure prose must ignore the code font size");
}

/// End-to-end through the real settings UI: a stepper click updates the
/// theme and the composer geometry follows once settings closes.
#[gpui_kit::test]
fn composer_follows_settings_stepper_clicks(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.configuration = Some(runtime_options_config());
    });
    let before = cx
        .update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(cx.theme().font_size, px(16.));
            window.find("composer-policy-label").bounds().size.height
        })
        .unwrap();
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        // The appearance page scrolls: center the stepper before clicking.
        scroll_settings_into_view(window, "font-ui-1", cx);
        window.within("settings-panel").click("font-ui-1", cx);
        window.render_frame(cx);
        assert_eq!(cx.theme().font_size, px(17.));
        window.within("settings-panel").click("settings-dismiss", cx);
        window.render_frame(cx);
    })
    .unwrap();
    let after = cx
        .update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.find("composer-policy-label").bounds().size.height
        })
        .unwrap();
    assert!(
        after > before,
        "composer labels must follow settings stepper clicks ({before:?} at 16px vs {after:?} at 17px)"
    );
}

/// Production frames draw WITHOUT `refresh()`, so element reuse stays active
/// (unlike `render_frame`, which forces a full re-render). A typography
/// change must still sync `rem_size` and move composer geometry on such a
/// frame — otherwise the setting only works in tests.
#[gpui_kit::test]
fn typography_survives_production_frame_reuse(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.configuration = Some(runtime_options_config());
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.rem_size(), px(16.));
        let before = window.find("composer-policy-label").bounds().size.height;
        view.update(cx, |view, cx| {
            view.preferences.ui_font_size = Some(24);
            view.apply_typography(cx);
        });
        window.draw(cx).clear(cx);
        assert_eq!(cx.theme().font_size, px(24.));
        assert_eq!(window.rem_size(), px(24.), "rem_size must sync on a reuse-active frame");
        let after = window.find("composer-policy-label").bounds().size.height;
        assert!(
            after > before,
            "composer labels must follow the font size on a reuse-active frame ({before:?} vs {after:?})"
        );
    })
    .unwrap();
}
