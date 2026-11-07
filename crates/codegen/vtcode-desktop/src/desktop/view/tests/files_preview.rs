use super::*;

#[gpui_kit::test]
fn markdown_files_preview_through_the_shared_viewer(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    std::fs::write(workspace.path().join("notes.md"), "# Notes\n\nShip the **viewer**.\n").unwrap();
    std::fs::write(workspace.path().join("notes.txt"), "plain text body").unwrap();
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
    });

    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_file_preview("notes.md".into(), window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("file-preview").is_some(), "the preview overlay opens");
        let viewer = window.find("file-preview-markdown");
        assert!(viewer.visible(), "markdown previews reuse the shared viewer");
        let spoken = viewer.label().unwrap_or_default();
        assert!(spoken.contains("Notes"), "the viewer exposes the heading: {spoken}");
        assert!(spoken.contains("Ship the viewer."), "the viewer exposes the body text: {spoken}");
    })
    .unwrap();

    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_file_preview("notes.txt".into(), window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("file-preview").is_some(), "text previews keep the overlay");
        assert!(window.try_find("file-preview-markdown").is_none(), "text previews keep the mono surface");
        assert_eq!(view.read(cx).file_preview.as_ref().expect("preview state").text, "plain text body");
    })
    .unwrap();
}

#[gpui_kit::test]
fn escape_dismisses_the_preview_and_restores_focus(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    std::fs::write(workspace.path().join("notes.md"), "# Notes\n").unwrap();
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
    });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.focus_composer(window, cx);
            view.open_file_preview("notes.md".into(), window, cx);
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("file-preview").is_some(), "the preview overlay opens");
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(view.read(cx).file_preview.is_none(), "Escape closes the preview");
        assert!(window.try_find("file-preview").is_none());
        assert!(
            view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window),
            "dismissal restores the captured focus"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn file_preview_traps_tab_across_its_header_actions(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    std::fs::write(workspace.path().join("notes.md"), "# Notes\n").unwrap();
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
    });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_file_preview("notes.md".into(), window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("file-preview").is_some(), "the preview overlay opens");
        assert_tab_trapped_within_dialog(window, "open-preview-editor", "close-preview", cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn markdown_preview_toggles_between_rendered_and_source(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    std::fs::write(workspace.path().join("notes.md"), "# Notes\n").unwrap();
    std::fs::write(workspace.path().join("notes.txt"), "plain text body").unwrap();
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
    });
    let open_preview = |window: AnyWindowHandle, view: &Entity<DesktopView>, cx: &mut TestAppContext, name: &str| {
        let name = name.to_string();
        cx.update_window(window, |_, window, cx| {
            view.update(cx, |view, cx| view.open_file_preview(name, window, cx));
        })
        .unwrap();
        cx.run_until_parked();
    };
    open_preview(window, &view, cx, "notes.md");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("preview-source-toggle").label(), Some("Source"));
        assert!(window.try_find("file-preview-markdown").is_some(), "markdown opens rendered");
        window.click("preview-source-toggle", cx);
        window.render_frame(cx);
        assert_eq!(window.find("preview-source-toggle").label(), Some("Preview"));
        assert!(view.read(cx).file_preview.as_ref().expect("preview state").show_source);
        assert!(window.try_find("file-preview-markdown").is_none(), "source hides the viewer");
        // The source arm renders the read-only editor surface. Padded
        // inside the body (p_3): no overflow below zero, no dead space
        // above the padding plus rounding slack.
        let body = window.find("file-preview-body").bounds();
        let source = window.find("file-preview-source").bounds();
        let height_gap = body.size.height - source.size.height;
        assert!(height_gap >= px(0.) && height_gap < px(32.), "source editor tracks the body height");
        let width_gap = body.size.width - source.size.width;
        assert!(width_gap >= px(0.) && width_gap < px(32.), "source editor tracks the body width");
        window.click("preview-source-toggle", cx);
        window.render_frame(cx);
        assert_eq!(window.find("preview-source-toggle").label(), Some("Source"));
        assert!(window.try_find("file-preview-markdown").is_some(), "toggling back restores the viewer");
        assert!(window.try_find("file-preview-source").is_none(), "rendered arm hides the source editor");
    })
    .unwrap();

    // Plain text has no rendered form, so the toggle stays out of its header.
    open_preview(window, &view, cx, "notes.txt");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find_all("preview-source-toggle").is_empty());
    })
    .unwrap();

    // Reopening markdown starts rendered even after viewing source.
    open_preview(window, &view, cx, "notes.md");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("preview-source-toggle").label(), Some("Source"));
        assert!(window.try_find("file-preview-markdown").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn markdown_source_editor_fills_a_tall_constrained_body(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    let text = (0..150)
        .map(|line| format!("# Heading {line}\n\nBody {line}.\n"))
        .collect::<String>();
    std::fs::write(workspace.path().join("long.md"), &text).unwrap();
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
    });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_file_preview("long.md".into(), window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        // The loader copies markdown into the persistent source editor.
        assert_eq!(view.read(cx).file_preview_source.read(cx).value().to_string(), text);
        window.render_frame(cx);
        window.click("preview-source-toggle", cx);
        window.render_frame(cx);
        // A 150-heading document overflows the panel: the body clamps to
        // the panel maximum instead of sizing to content.
        let body = window.find("file-preview-body").bounds();
        assert!(body.size.height > px(200.), "tall source constrains the body; got {body:?}");
        // The editor surface fills that constrained body rather than
        // overflowing it (negative gap) or collapsing (large gap).
        let source = window.find("file-preview-source").bounds();
        let height_gap = body.size.height - source.size.height;
        assert!(height_gap >= px(0.) && height_gap < px(32.), "source editor fills the tall body");
    })
    .unwrap();
}

#[gpui_kit::test]
fn file_preview_source_arm_keeps_tab_trapped(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    std::fs::write(workspace.path().join("notes.md"), "# Notes\n").unwrap();
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
    });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_file_preview("notes.md".into(), window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("preview-source-toggle", cx);
        window.render_frame(cx);
        assert!(window.try_find("file-preview-source").is_some(), "the source arm opens");
        // Tab order runs the header actions, then the editor body: from the
        // last header action focus enters the read-only editor …
        focus_element_by_tab(window, "close-preview", cx);
        window.press("tab", cx);
        window.render_frame(cx);
        assert!(
            view.read(cx).file_preview_source.read(cx).focus_handle(cx).is_focused(window),
            "tab from the header reaches the source editor"
        );
        // … and Tab leaves it again: the read-only editor propagates Tab
        // instead of indenting, so the trap wraps back to the header.
        window.press("tab", cx);
        window.render_frame(cx);
        assert_eq!(
            window.find("open-preview-editor").focused(),
            Some(true),
            "tab from the source editor wraps to the first header action"
        );
        window.press("shift-tab", cx);
        window.render_frame(cx);
        assert!(
            view.read(cx).file_preview_source.read(cx).focus_handle(cx).is_focused(window),
            "shift-tab from the first action returns to the source editor"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn markdown_source_arm_stays_contained_at_the_minimum_window(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    let text = (0..150)
        .map(|line| format!("# Heading {line}\n\nBody {line}.\n"))
        .collect::<String>();
    std::fs::write(workspace.path().join("long.md"), &text).unwrap();
    let (window, view) = open_desktop_sized(cx, 600., 560.);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
    });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_file_preview("long.md".into(), window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("preview-source-toggle", cx);
        window.render_frame(cx);
        // The 24-row source viewport must not push the panel past the
        // window: the clamped panel contains it and the editor scrolls.
        let viewport = window.viewport_size();
        let panel = window.find("file-preview").bounds();
        assert!(panel.origin.x >= px(0.) && panel.origin.y >= px(0.), "panel stays on-screen: {panel:?}");
        assert!(
            panel.origin.x + panel.size.width <= viewport.width
                && panel.origin.y + panel.size.height <= viewport.height,
            "panel fits the minimum window: {panel:?} in {viewport:?}"
        );
        assert!(window.try_find("file-preview-source").is_some(), "the source arm renders");
        // The full seven-button header (Edit + Save armed) wraps instead of
        // overflowing the same minimum window.
        window.click("edit-preview-source", cx);
        window.render_frame(cx);
        let panel = window.find("file-preview").bounds();
        assert!(
            panel.origin.x + panel.size.width <= viewport.width
                && panel.origin.y + panel.size.height <= viewport.height,
            "panel fits the minimum window while editing: {panel:?} in {viewport:?}"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn crlf_markdown_opens_clean_in_edit_mode(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    std::fs::write(workspace.path().join("notes.md"), "# Notes\r\n\r\nBody.\r\n").unwrap();
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
    });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_file_preview("notes.md".into(), window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        // Line-ending normalization (if any) must not read as a user edit:
        // a CRLF file opens clean, and saving an untouched draft is refused.
        window.click("edit-preview-source", cx);
        window.render_frame(cx);
        assert_eq!(window.find("save-preview-source").disabled(), Some(true), "CRLF file opens clean");
    })
    .unwrap();
}

#[gpui_kit::test]
fn preview_source_save_failure_keeps_the_draft(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    let path = workspace.path().join("notes.md");
    std::fs::write(&path, "# Notes\n").unwrap();
    let mut permissions = std::fs::metadata(&path).expect("file metadata").permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&path, permissions).unwrap();
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
    });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_file_preview("notes.md".into(), window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("edit-preview-source", cx);
        window.render_frame(cx);
        view.read(cx).file_preview_source.read(cx).focus_handle(cx).focus(window, cx);
        window.input("!", cx);
        window.render_frame(cx);
        window.click("save-preview-source", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let on_disk = std::fs::read_to_string(&path).expect("read-only file");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        // The write fails, the error surfaces in the preview, and the draft
        // stays put: editing is still armed and the preview keeps old text.
        assert_eq!(on_disk, "# Notes\n", "failed save leaves the file alone");
        let preview = view.read(cx).file_preview.as_ref().expect("preview state");
        assert_eq!(preview.text, "# Notes\n", "failed save leaves the preview alone");
        assert!(preview.editing, "failed save keeps edit mode armed");
        let editor_error = view.read(cx).preview_editor_error.clone();
        assert!(editor_error.is_some_and(|error| error.contains("notes.md")));
        assert!(window.try_find("preview-editor-error").is_some(), "the error alert renders");
        assert_ne!(window.find("save-preview-source").disabled(), Some(true), "draft stays dirty");
    })
    .unwrap();
}

#[gpui_kit::test]
fn preview_source_cmd_s_saves_the_draft(cx: &mut TestAppContext) {
    use super::super::SavePreviewSource;
    use crate::desktop::strings::key_context;

    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    std::fs::write(workspace.path().join("notes.md"), "# Notes\n").unwrap();
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
    });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_file_preview("notes.md".into(), window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        assert_chord_resolves_to(cx, "cmd-s", key_context::DESKTOP, &SavePreviewSource);
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("edit-preview-source", cx);
        window.render_frame(cx);
        view.read(cx).file_preview_source.read(cx).focus_handle(cx).focus(window, cx);
        window.input("!", cx);
        window.render_frame(cx);
        window.press("cmd-s", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let saved = std::fs::read_to_string(workspace.path().join("notes.md")).expect("saved file");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(saved.contains('!'), "Cmd+S persists the keystroke to disk");
        assert_eq!(saved, view.read(cx).file_preview.as_ref().expect("preview state").text);
        assert_eq!(window.find("save-preview-source").disabled(), Some(true), "saved draft is clean");
    })
    .unwrap();
}

#[gpui_kit::test]
fn preview_source_cmd_s_ignores_unarmed_states(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    std::fs::write(workspace.path().join("notes.md"), "# Notes\n").unwrap();
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
    });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_file_preview("notes.md".into(), window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        // Rendered arm: nothing armed, nothing saved.
        window.press("cmd-s", cx);
        window.render_frame(cx);
        // A dirty draft parked with Done stays parked: Cmd+S mirrors the
        // Save button, which only exists while editing.
        window.click("edit-preview-source", cx);
        window.render_frame(cx);
        view.read(cx).file_preview_source.read(cx).focus_handle(cx).focus(window, cx);
        window.input("!", cx);
        window.render_frame(cx);
        window.click("edit-preview-source", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).file_preview.as_ref().expect("preview state").editing);
        window.press("cmd-s", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    let on_disk = std::fs::read_to_string(workspace.path().join("notes.md")).expect("unsaved file");
    assert_eq!(on_disk, "# Notes\n", "unarmed Cmd+S writes nothing");
    // The same draft saves once re-armed, proving the shortcut only gates
    // on the edit session, not the content.
    cx.update_window(window, |_, window, cx| {
        window.click("edit-preview-source", cx);
        window.render_frame(cx);
        window.press("cmd-s", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let saved = std::fs::read_to_string(workspace.path().join("notes.md")).expect("saved file");
    assert!(saved.contains('!'), "armed Cmd+S saves the parked draft");
}

#[gpui_kit::test]
fn file_preview_toolbar_moves_focus_with_arrow_keys(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    std::fs::write(workspace.path().join("notes.md"), "# Notes\n").unwrap();
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
    });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_file_preview("notes.md".into(), window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        // The bar id scopes paths but registers nothing observable (same
        // as tab bars), so pin the roving behavior, not the structure.
        // Arrow keys rove across the header actions in visual order.
        focus_element_by_tab(window, "open-preview-editor", cx);
        window.press("right", cx);
        window.render_frame(cx);
        assert_eq!(window.find("open-preview-vscode").focused(), Some(true));
        window.press("right", cx);
        window.render_frame(cx);
        assert_eq!(window.find("preview-source-toggle").focused(), Some(true));
        window.press("left", cx);
        window.render_frame(cx);
        assert_eq!(window.find("open-preview-vscode").focused(), Some(true));
        // Left from the first action wraps to the last one.
        focus_element_by_tab(window, "open-preview-editor", cx);
        window.press("left", cx);
        window.render_frame(cx);
        assert_eq!(window.find("close-preview").focused(), Some(true));
    })
    .unwrap();
}

#[gpui_kit::test]
fn preview_source_edits_save_back_to_the_file(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    std::fs::write(workspace.path().join("notes.md"), "# Notes\n").unwrap();
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
    });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_file_preview("notes.md".into(), window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        // The Edit toggle jumps straight from the rendered arm into the
        // editable source surface.
        assert_eq!(window.find("edit-preview-source").label(), Some("Edit"));
        window.click("edit-preview-source", cx);
        window.render_frame(cx);
        assert!(view.read(cx).file_preview.as_ref().expect("preview state").editing);
        assert!(view.read(cx).file_preview.as_ref().expect("preview state").show_source);
        assert_eq!(window.find("edit-preview-source").label(), Some("Done"));
        assert_eq!(window.find("save-preview-source").disabled(), Some(true), "clean draft disables Save");
        // Honest keystrokes (not set_value, which suppresses Change):
        // focus the editor through its handle, then type.
        view.read(cx).file_preview_source.read(cx).focus_handle(cx).focus(window, cx);
        window.input("!", cx);
        window.render_frame(cx);
        assert_ne!(window.find("save-preview-source").disabled(), Some(true), "typing enables Save");
        // The draft survives a Preview round-trip.
        window.click("preview-source-toggle", cx);
        window.render_frame(cx);
        window.click("preview-source-toggle", cx);
        window.render_frame(cx);
        let draft = view.read(cx).file_preview_source.read(cx).value().to_string();
        assert!(draft.contains('!') && draft.len() == "# Notes\n".len() + 1, "draft keeps the keystroke");
        window.click("save-preview-source", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let saved = std::fs::read_to_string(workspace.path().join("notes.md")).expect("saved file");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let preview = view.read(cx).file_preview.as_ref().expect("preview state");
        assert_eq!(saved, preview.text, "save refreshes the preview text");
        assert!(saved.contains('!'), "save persists the keystroke to disk");
        assert_eq!(window.find("save-preview-source").disabled(), Some(true), "saved draft is clean");
    })
    .unwrap();
}

#[gpui_kit::test]
fn escape_exits_preview_edit_mode_before_dismissing(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    std::fs::write(workspace.path().join("notes.md"), "# Notes\n").unwrap();
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
    });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_file_preview("notes.md".into(), window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("preview-source-toggle", cx);
        window.click("edit-preview-source", cx);
        window.render_frame(cx);
        assert!(view.read(cx).file_preview.as_ref().expect("preview state").editing);
        // First Escape leaves edit mode (restoring Tab navigation) without
        // dismissing; the second Escape closes the preview.
        window.press("escape", cx);
        window.render_frame(cx);
        let preview = view.read(cx).file_preview.as_ref().expect("preview stays open");
        assert!(!preview.editing, "first escape disarms editing");
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(view.read(cx).file_preview.is_none(), "second escape dismisses");
    })
    .unwrap();
}

#[gpui_kit::test]
fn truncated_markdown_hides_the_edit_toggle(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    std::fs::write(workspace.path().join("big.md"), "x".repeat(70_000)).unwrap();
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
    });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_file_preview("big.md".into(), window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).file_preview.as_ref().expect("preview state").truncated);
        window.click("preview-source-toggle", cx);
        window.render_frame(cx);
        // The editor holds only the file head, so editing stays
        // unavailable: saving would destroy the unseen tail.
        assert!(window.find_all("edit-preview-source").is_empty(), "truncated hides Edit");
        assert!(window.find_all("save-preview-source").is_empty(), "truncated hides Save");
    })
    .unwrap();
}
