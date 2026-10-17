use tokio::sync::mpsc;
use vtcode_commons::ui_protocol::ActivityState;

macro_rules! protocol_control_tests {
    ($module:ident, $protocol:path) => {
        mod $module {
            use super::*;
            use $protocol as protocol;

            #[test]
            fn lifecycle_commands_preserve_order_and_identity() {
                let (sender, mut receiver) = mpsc::unbounded_channel();
                let handle = protocol::InlineHandle::new_for_tests(sender);
                handle.suspend_event_loop();
                handle.resume_event_loop();
                handle.clear_input_queue();
                handle.stop_event_stream();
                handle.start_event_stream();
                handle.clear_inline_prompt_suggestion();
                handle.clear_input();
                handle.force_redraw();
                handle.clear_screen();
                handle.shutdown();
                let mut names = Vec::new();
                while let Ok(command) = receiver.try_recv() {
                    names.push(match command {
                        protocol::InlineCommand::SuspendEventLoop => "suspend",
                        protocol::InlineCommand::ResumeEventLoop => "resume",
                        protocol::InlineCommand::ClearInputQueue => "queue",
                        protocol::InlineCommand::StopEventStream => "stop",
                        protocol::InlineCommand::StartEventStream => "start",
                        protocol::InlineCommand::ClearInlinePromptSuggestion => "suggestion",
                        protocol::InlineCommand::ClearInput => "input",
                        protocol::InlineCommand::ForceRedraw => "redraw",
                        protocol::InlineCommand::ClearScreen => "screen",
                        protocol::InlineCommand::Shutdown => "shutdown",
                        _ => panic!("unexpected lifecycle command"),
                    });
                }
                assert_eq!(names, ["suspend", "resume", "queue", "stop", "start", "suggestion", "input", "redraw", "screen", "shutdown"]);
            }

            #[test]
            fn toggles_preserve_both_boolean_values() {
                let (sender, mut receiver) = mpsc::unbounded_channel();
                let handle = protocol::InlineHandle::new_for_tests(sender);
                for enabled in [false, true] {
                    handle.set_cursor_visible(enabled);
                    handle.set_input_enabled(enabled);
                    handle.set_image_input_enabled(enabled);
                    handle.set_vim_mode_enabled(enabled);
                    handle.set_skip_confirmations(enabled);
                    handle.set_color_scheme_auto(enabled);
                    assert!(matches!(receiver.try_recv().unwrap(), protocol::InlineCommand::SetCursorVisible(value) if value == enabled));
                    assert!(matches!(receiver.try_recv().unwrap(), protocol::InlineCommand::SetInputEnabled(value) if value == enabled));
                    assert!(matches!(receiver.try_recv().unwrap(), protocol::InlineCommand::SetImageInputEnabled(value) if value == enabled));
                    assert!(matches!(receiver.try_recv().unwrap(), protocol::InlineCommand::SetVimModeEnabled(value) if value == enabled));
                    assert!(matches!(receiver.try_recv().unwrap(), protocol::InlineCommand::SetSkipConfirmations(value) if value == enabled));
                    assert!(matches!(receiver.try_recv().unwrap(), protocol::InlineCommand::SetColorSchemeAuto { enabled: value } if value == enabled));
                }
                assert!(receiver.try_recv().is_err());
            }

            #[test]
            fn configured_status_preserves_custom_text_and_explicit_clear() {
                let (sender, mut receiver) = mpsc::unbounded_channel();
                let handle = protocol::InlineHandle::new_for_tests(sender);
                handle.set_configured_input_status(Some("Running custom dashboard".into()), Some("configured right".into()));
                handle.set_configured_input_status(None, None);
                assert!(matches!(receiver.try_recv().unwrap(), protocol::InlineCommand::SetConfiguredInputStatus { left: Some(left), right: Some(right) } if left == "Running custom dashboard" && right == "configured right"));
                assert!(matches!(receiver.try_recv().unwrap(), protocol::InlineCommand::SetConfiguredInputStatus { left: None, right: None }));
                assert!(receiver.try_recv().is_err());
            }

            #[test]
            fn optional_status_and_queued_entries_remain_distinct() {
                let (sender, mut receiver) = mpsc::unbounded_channel();
                let handle = protocol::InlineHandle::new_for_tests(sender);
                handle.set_input_status(Some("left".into()), None);
                handle.set_input_status(None, Some("right".into()));
                handle.set_primary_agent(Some("agent".into()), Some("blue".into()));
                handle.set_subagent_preview(None);
                handle.set_queued_inputs(vec!["queued-first".into(), "queued-second".into()]);
                handle.set_subprocess_entries(vec!["process".into()]);
                handle.set_activity_state(ActivityState::Blocked);
                handle.set_reasoning_stage(Some("checking".into()));
                handle.set_placeholder(Some("hint".into()));
                assert!(matches!(receiver.try_recv().unwrap(), protocol::InlineCommand::SetInputStatus { left: Some(left), right: None } if left == "left"));
                assert!(matches!(receiver.try_recv().unwrap(), protocol::InlineCommand::SetInputStatus { left: None, right: Some(right) } if right == "right"));
                assert!(matches!(receiver.try_recv().unwrap(), protocol::InlineCommand::SetPrimaryAgent { name: Some(name), color: Some(color) } if name == "agent" && color == "blue"));
                assert!(matches!(receiver.try_recv().unwrap(), protocol::InlineCommand::SetSubagentPreview { text: None }));
                assert!(matches!(receiver.try_recv().unwrap(), protocol::InlineCommand::SetQueuedInputs { entries } if entries == ["queued-first", "queued-second"]));
                assert!(matches!(receiver.try_recv().unwrap(), protocol::InlineCommand::SetSubprocessEntries { entries } if entries == ["process"]));
                assert!(matches!(receiver.try_recv().unwrap(), protocol::InlineCommand::SetActivityState(ActivityState::Blocked)));
                assert!(matches!(receiver.try_recv().unwrap(), protocol::InlineCommand::SetReasoningStage(Some(stage)) if stage == "checking"));
                assert!(matches!(receiver.try_recv().unwrap(), protocol::InlineCommand::SetPlaceholder { hint: Some(hint), style: None } if hint == "hint"));
                assert!(receiver.try_recv().is_err());
            }

            #[test]
            fn restoring_a_draft_preserves_attachments_and_batching_metadata() {
                let (sender, mut receiver) = mpsc::unbounded_channel();
                let handle = protocol::InlineHandle::new_for_tests(sender);
                let input = protocol::SubmittedInput::new(" original ", vec![protocol::ContentPart::image("image-data", "image/png")]).batchable();
                handle.restore_input_draft(input);
                match receiver.try_recv().unwrap() {
                    protocol::InlineCommand::RestoreInputDraft(input) => {
                        assert_eq!(input.text, " original ");
                        assert!(input.batchable);
                        assert_eq!(input.attachments.len(), 1);
                        assert!(matches!(&input.attachments[0], protocol::ContentPart::Image { data, media_type } if data.as_ref() == "image-data" && media_type == "image/png"));
                    }
                    _ => panic!("expected draft restoration"),
                }
                assert!(receiver.try_recv().is_err());
            }
        }
    };
}

protocol_control_tests!(core, crate::tui::core_tui::types);
protocol_control_tests!(app, crate::tui::core_tui::app::types);

#[test]
fn app_message_labels_keep_display_width_side_effects_local() {
    use crate::tui::core_tui::app::types::{InlineCommand, InlineHandle};
    let (sender, mut receiver) = mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    handle.set_message_labels(Some("界".into()), Some("user".into()));
    assert_eq!(handle.agent_label_frame_width(), 3);
    assert!(
        matches!(receiver.try_recv().unwrap(), InlineCommand::SetMessageLabels { agent: Some(agent), user: Some(user) } if agent == "界" && user == "user")
    );
    handle.set_message_labels(Some(String::new()), None);
    assert_eq!(handle.agent_label_frame_width(), 0);
    assert!(
        matches!(receiver.try_recv().unwrap(), InlineCommand::SetMessageLabels { agent: Some(agent), user: None } if agent.is_empty())
    );
}
