use super::super::SubmissionDraft;
use super::*;

fn attachment_draft(prompt: &str, echo_id: &str) -> SubmissionDraft {
    SubmissionDraft {
        prompt: prompt.to_string(),
        // Asymmetric fixtures: a text file with content bytes and an image with none.
        attachments: vec![
            vtcode_desktop_protocol::TurnAttachment {
                path: "src/main.rs".into(),
                text: "fn main() {}".into(),
            },
            vtcode_desktop_protocol::TurnAttachment { path: "shot.png".into(), text: String::new() },
        ],
        echo_id: echo_id.to_string(),
        delivery_uncertain: false,
    }
}

#[gpui_kit::test]
fn user_echo_with_attachments_renders_cards_above_the_prompt(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 1600., 900.);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.push_submission_echo(&attachment_draft("Review these files", "echo-attachments"));
            view.push_submission_echo(&SubmissionDraft {
                prompt: "Plain follow-up".into(),
                attachments: Vec::new(),
                echo_id: "echo-plain".into(),
                delivery_uncertain: false,
            });
        });
        window.render_frame(cx);

        let cards = window.find("user-attachments-echo-attachments");
        assert_eq!(cards.role(), Some(gpui_kit::Role::Group));
        let label = cards.label().expect("attachment stack exposes its label");
        assert!(
            label.contains("src/main.rs") && label.contains("shot.png"),
            "both card paths are announced, got {label:?}"
        );
        assert!(window.try_find("user-attachments-echo-plain").is_none(), "text-only echoes keep the previous shape");

        let body_handle = window.find("message-body-echo-attachments");
        let body_label = body_handle.label().expect("echo body exposes its label");
        assert!(body_label.contains("Review these files"), "prompt stays announced, got {body_label:?}");
        assert!(
            body_label.contains("src/main.rs") && body_label.contains("shot.png"),
            "paths stay on the body label, got {body_label:?}"
        );

        let stack = window.find("user-attachments-echo-attachments").bounds();
        let body = window.find("message-body-echo-attachments").bounds();
        let column = window.find("transcript-content-echo-attachments").bounds();
        assert!(stack.bottom() <= body.top(), "cards stack above the prompt text");
        assert!(stack.right() <= column.right(), "cards stay inside the conversation column");
    })
    .unwrap();
}

#[gpui_kit::test]
fn attachment_filenames_match_palette_conversation_search(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let session_id = "search-attachments".to_string();
    view.update(cx, |view, _| {
        view.selected_session_id = Some(session_id.clone());
        view.push_submission_echo(&attachment_draft("Review these files", "echo-attachments"));
        view.push_submission_echo(&SubmissionDraft {
            prompt: "Plain follow-up".into(),
            attachments: Vec::new(),
            echo_id: "echo-plain".into(),
            delivery_uncertain: false,
        });
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, cx| view.open_conversation_search(session_id.clone(), window, cx));
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        // Neither prompt, title, nor speaker contains the filename: only the
        // attachment keyword can match.
        window.within("command-palette").input("main.rs", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).palette.as_ref().unwrap().read(cx).matched_count(), 1);
        let echo_id = palette_action_id(&super::super::PaletteAction::JumpToTranscriptItem {
            session_id: session_id.clone(),
            item_id: "echo-attachments".to_string(),
            from_search: true,
        });
        assert!(window.try_find(echo_id).is_some(), "the filename match lands on the attachment echo");
    })
    .unwrap();
}

#[gpui_kit::test]
fn echo_card_view_action_opens_the_file_preview(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    std::fs::write(workspace.path().join("notes.md"), "# Notes\n").unwrap();
    std::fs::write(workspace.path().join("todo.txt"), "plain text body").unwrap();
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
        view.push_submission_echo(&SubmissionDraft {
            prompt: "Review these files".into(),
            attachments: vec![
                vtcode_desktop_protocol::TurnAttachment { path: "notes.md".into(), text: "# Notes\n".into() },
                vtcode_desktop_protocol::TurnAttachment {
                    path: "todo.txt".into(),
                    text: "plain text body".into(),
                },
            ],
            echo_id: "echo-preview".into(),
            delivery_uncertain: false,
        });
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        // Asymmetric: both cards expose distinct View actions.
        let notes_id = opaque_path_element_id("view-echo-attachment", "notes.md");
        let todo_id = opaque_path_element_id("view-echo-attachment", "todo.txt");
        assert_ne!(notes_id, todo_id);
        assert!(window.try_find(notes_id.clone()).is_some(), "first card exposes View");
        assert!(window.try_find(todo_id).is_some(), "second card exposes View");
        window.click(notes_id, cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("file-preview").is_some(), "View opens the in-app preview");
        assert_eq!(view.read(cx).file_preview.as_ref().expect("preview state").rel_path, "notes.md");
    })
    .unwrap();
}

#[gpui_kit::test]
fn attachment_paths_stay_searchable_after_deflattening(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, _| {
            view.push_submission_echo(&attachment_draft("Review these files", "echo-attachments"));
            view.push_submission_echo(&SubmissionDraft {
                prompt: "Plain follow-up".into(),
                attachments: Vec::new(),
                echo_id: "echo-plain".into(),
                delivery_uncertain: false,
            });
            // A picked id outside the transcript cannot join the set, so a
            // snapshot proves the query itself matched a row.
            view.remember_filter_matches("main.rs", "no-such-row");
            assert!(view.filter_matches.is_some(), "the text-file path still matches conversation search");
            view.remember_filter_matches("shot.png", "no-such-row");
            assert!(view.filter_matches.is_some(), "the image path still matches conversation search");
            view.remember_filter_matches("zzz-no-such-file", "no-such-row");
            assert!(view.filter_matches.is_none(), "unrelated queries still match nothing");
        });
    })
    .unwrap();
}
