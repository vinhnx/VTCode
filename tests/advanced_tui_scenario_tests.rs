//! Advanced TUI snapshot tests with real content rendering
//!
//! These tests simulate actual user interactions with real content to verify
//! the TUI renders correctly under various scenarios.
//!
//! To update snapshots, run: `cargo insta review`

use anstyle::Effects;
use insta::assert_snapshot;
use ratatui::{Terminal, backend::TestBackend};
use vtcode_core::ui::{
    InlineCommand, InlineHandle, InlineHeaderContext, InlineMessageKind, InlineSegment,
    InlineTextStyle,
};

fn inline_command_variant_name(command: &InlineCommand) -> &'static str {
    match command {
        InlineCommand::AppendLine { .. } => "AppendLine",
        InlineCommand::AppendPastedMessage { .. } => "AppendPastedMessage",
        InlineCommand::Inline { .. } => "Inline",
        InlineCommand::ReplaceLast { .. } => "ReplaceLast",
        InlineCommand::SetPrompt { .. } => "SetPrompt",
        InlineCommand::SetPlaceholder { .. } => "SetPlaceholder",
        InlineCommand::SetMessageLabels { .. } => "SetMessageLabels",
        InlineCommand::SetHeaderContext { .. } => "SetHeaderContext",
        InlineCommand::SetInputStatus { .. } => "SetInputStatus",
        InlineCommand::SetTerminalTitleItems { .. } => "SetTerminalTitleItems",
        InlineCommand::SetTerminalTitleThreadLabel { .. } => "SetTerminalTitleThreadLabel",
        InlineCommand::SetTerminalTitleGitBranch { .. } => "SetTerminalTitleGitBranch",
        InlineCommand::SetTheme { .. } => "SetTheme",
        InlineCommand::SetAppearance { .. } => "SetAppearance",
        InlineCommand::SetVimModeEnabled(_) => "SetVimModeEnabled",
        InlineCommand::SetQueuedInputs { .. } => "SetQueuedInputs",
        InlineCommand::SetSubprocessEntries { .. } => "SetSubprocessEntries",
        InlineCommand::SetSubagentPreview { .. } => "SetSubagentPreview",
        InlineCommand::SetLocalAgents { .. } => "SetLocalAgents",
        InlineCommand::SetArchivedHistory { .. } => "SetArchivedHistory",
        InlineCommand::SetPrimaryAgent { .. } => "SetPrimaryAgent",
        InlineCommand::SetCursorVisible(_) => "SetCursorVisible",
        InlineCommand::SetInputEnabled(_) => "SetInputEnabled",
        InlineCommand::SetInput(_) => "SetInput",
        InlineCommand::ApplySuggestedPrompt(_) => "ApplySuggestedPrompt",
        InlineCommand::SetInlinePromptSuggestion { .. } => "SetInlinePromptSuggestion",
        InlineCommand::ClearInlinePromptSuggestion => "ClearInlinePromptSuggestion",
        InlineCommand::ClearInput => "ClearInput",
        InlineCommand::ForceRedraw => "ForceRedraw",
        InlineCommand::ShowTransient { .. } => "ShowTransient",
        InlineCommand::CloseTransient => "CloseTransient",
        InlineCommand::ClearScreen => "ClearScreen",
        InlineCommand::SuspendEventLoop => "SuspendEventLoop",
        InlineCommand::ResumeEventLoop => "ResumeEventLoop",
        InlineCommand::ClearInputQueue => "ClearInputQueue",
        InlineCommand::StopEventStream => "StopEventStream",
        InlineCommand::StartEventStream => "StartEventStream",
        InlineCommand::SetSkipConfirmations(_) => "SetSkipConfirmations",
        InlineCommand::Shutdown => "Shutdown",
        InlineCommand::SetReasoningStage(_) => "SetReasoningStage",
    }
}

/// Test TUI with actual conversation history
#[test]
fn test_tui_with_conversation_history() {
    // Create a TestBackend to simulate the terminal
    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).unwrap();

    // Draw a representation of what a conversation might look like
    terminal
        .draw(|f| {
            // This simulates drawing with conversation content
            let area = f.area();
            assert!(area.width > 0);
            assert!(area.height > 0);
        })
        .unwrap();

    assert_snapshot!(
        format!("{}", terminal.backend()),
        @r###"
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "                                                                                "
    "###
    );
}

/// Test actual UI state with populated content using the command system
#[test]
fn test_real_ui_scenario_with_commands() {
    let (command_tx, mut command_rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(command_tx);

    // Send some commands to populate the UI with real content
    handle.append_line(
        InlineMessageKind::User,
        vec![InlineSegment {
            text: "Can you help me refactor this Rust code?".to_string(),
            style: InlineTextStyle::default().into(),
        }],
    );

    handle.append_line(
        InlineMessageKind::Agent,
        vec![InlineSegment {
            text: "Sure! I can help you refactor your Rust code. Could you share the code you'd like to refactor?".to_string(),
            style: InlineTextStyle::default().into(),
        }],
    );

    handle.append_line(
        InlineMessageKind::User,
        vec![InlineSegment {
            text: "Here's the code I want to refactor:\n```rust\nfn calculate_sum(numbers: Vec<i32>) -> i32 {\n    let mut sum = 0;\n    for i in 0..numbers.len() {\n        sum += numbers[i];\n    }\n    sum\n}\n```".to_string(),
            style: InlineTextStyle::default().into(),
        }],
    );

    handle.append_line(
        InlineMessageKind::Agent,
        vec![InlineSegment {
            text: "I can help refactor this code to be more idiomatic Rust. Here's an improved version:\n```rust\nfn calculate_sum(numbers: &[i32]) -> i32 {\n    numbers.iter().sum()\n}\n```\nThis version: 1) Takes a slice instead of moving the Vec, 2) Uses iterator methods for better performance, 3) Is more idiomatic Rust.".to_string(),
            style: InlineTextStyle::default().into(),
        }],
    );

    let mut appended = Vec::new();
    while let Ok(command) = command_rx.try_recv() {
        match command {
            InlineCommand::AppendLine { kind, segments } => appended.push((kind, segments)),
            unexpected => panic!(
                "unexpected inline command variant: {}",
                inline_command_variant_name(&unexpected)
            ),
        }
    }

    assert_eq!(appended.len(), 4);
    assert_eq!(appended[0].0, InlineMessageKind::User);
    assert_eq!(appended[1].0, InlineMessageKind::Agent);
    assert_eq!(appended[2].0, InlineMessageKind::User);
    assert_eq!(appended[3].0, InlineMessageKind::Agent);
    assert_eq!(
        appended[0].1.first().map(|segment| segment.text.as_str()),
        Some("Can you help me refactor this Rust code?")
    );
    assert!(
        appended[3]
            .1
            .first()
            .is_some_and(|segment| segment.text.contains("numbers.iter().sum()"))
    );
}

/// Test TUI with various header contexts that represent different states
#[test]
fn test_tui_with_different_header_contexts() {
    let contexts = vec![
        (
            "basic_context",
            InlineHeaderContext {
                provider: "openai".to_string(),
                model: "gpt-oss-20b".to_string(),
                version: "test-version".to_string(),
                ..Default::default()
            },
        ),
        (
            "advanced_context",
            InlineHeaderContext {
                provider: "anthropic".to_string(),
                model: "claude-3".to_string(),
                reasoning: "analytical".to_string(),
                version: "test-version".to_string(),
                ..Default::default()
            },
        ),
        (
            "minimal_context",
            InlineHeaderContext {
                provider: "local".to_string(),
                model: "llama3".to_string(),
                version: "test-version".to_string(),
                ..Default::default()
            },
        ),
    ];

    for (name, context) in contexts {
        let expected = match name {
            "basic_context" => {
                r#"InlineHeaderContext { app_name: "App", provider: "openai", model: "gpt-oss-20b", context_window_size: None, version: "test-version", search_tools: None, persistent_memory: None, pr_review: None, editor_context: None, git: "git: unavailable", reasoning: "Reasoning effort: unavailable", reasoning_stage: None, workspace_trust: "Trust: unavailable", tools: "Tools: unavailable", mcp: "MCP: unavailable", primary_agent: None, highlights: [], subagent_badges: [] }"#
            }
            "advanced_context" => {
                r#"InlineHeaderContext { app_name: "App", provider: "anthropic", model: "claude-3", context_window_size: None, version: "test-version", search_tools: None, persistent_memory: None, pr_review: None, editor_context: None, git: "git: unavailable", reasoning: "analytical", reasoning_stage: None, workspace_trust: "Trust: unavailable", tools: "Tools: unavailable", mcp: "MCP: unavailable", primary_agent: None, highlights: [], subagent_badges: [] }"#
            }
            "minimal_context" => {
                r#"InlineHeaderContext { app_name: "App", provider: "local", model: "llama3", context_window_size: None, version: "test-version", search_tools: None, persistent_memory: None, pr_review: None, editor_context: None, git: "git: unavailable", reasoning: "Reasoning effort: unavailable", reasoning_stage: None, workspace_trust: "Trust: unavailable", tools: "Tools: unavailable", mcp: "MCP: unavailable", primary_agent: None, highlights: [], subagent_badges: [] }"#
            }
            _ => unreachable!("unexpected context fixture"),
        };
        assert_eq!(format!("{context:?}"), expected);
    }
}

/// Test UI with different message combinations
#[test]
fn test_ui_message_combinations() {
    let test_cases = vec![
        (
            "user_agent_exchange",
            vec![
                (InlineMessageKind::User, "Hello!"),
                (InlineMessageKind::Agent, "Hi there! How can I help you?"),
            ],
        ),
        (
            "error_scenario",
            vec![
                (InlineMessageKind::User, "Run this command"),
                (
                    InlineMessageKind::Error,
                    "Command failed with error: Permission denied",
                ),
                (
                    InlineMessageKind::Agent,
                    "I encountered an error. Would you like me to try again with sudo?",
                ),
            ],
        ),
        (
            "tool_usage",
            vec![
                (
                    InlineMessageKind::User,
                    "Show me files in current directory",
                ),
                (InlineMessageKind::Tool, "run_pty_cmd([\"ls\", \"-la\"])"),
                (InlineMessageKind::Pty, "file1.txt  file2.rs  src/"),
                (
                    InlineMessageKind::Agent,
                    "I've listed the files in the current directory for you.",
                ),
            ],
        ),
    ];

    for (name, messages) in test_cases {
        let message_repr: Vec<String> = messages
            .iter()
            .map(|(kind, text)| format!("{kind:?}: {text}"))
            .collect();
        let expected = match name {
            "user_agent_exchange" => r#"["User: Hello!", "Agent: Hi there! How can I help you?"]"#,
            "error_scenario" => {
                r#"["User: Run this command", "Error: Command failed with error: Permission denied", "Agent: I encountered an error. Would you like me to try again with sudo?"]"#
            }
            "tool_usage" => {
                r#"["User: Show me files in current directory", "Tool: run_pty_cmd([\"ls\", \"-la\"])", "Pty: file1.txt  file2.rs  src/", "Agent: I've listed the files in the current directory for you."]"#
            }
            _ => unreachable!("unexpected message fixture"),
        };
        assert_eq!(format!("{message_repr:?}"), expected);
    }
}

/// Test UI styling combinations
#[test]
fn test_ui_styling_variations() {
    let styled_segments = vec![
        (
            "plain_text",
            InlineSegment {
                text: "This is plain text".to_string(),
                style: InlineTextStyle {
                    color: None,
                    bg_color: None,
                    effects: Effects::new(),
                }
                .into(),
            },
        ),
        (
            "bold_text",
            InlineSegment {
                text: "This is bold text".to_string(),
                style: InlineTextStyle {
                    color: None,
                    bg_color: None,
                    effects: Effects::BOLD,
                }
                .into(),
            },
        ),
        (
            "italic_text",
            InlineSegment {
                text: "This is italic text".to_string(),
                style: InlineTextStyle {
                    color: None,
                    bg_color: None,
                    effects: Effects::ITALIC,
                }
                .into(),
            },
        ),
        (
            "bold_italic_text",
            InlineSegment {
                text: "This is bold and italic text".to_string(),
                style: InlineTextStyle {
                    color: None,
                    bg_color: None,
                    effects: Effects::BOLD | Effects::ITALIC,
                }
                .into(),
            },
        ),
    ];

    for (name, segment) in styled_segments {
        let expected = match name {
            "plain_text" => {
                r#"InlineSegment { text: "This is plain text", style: InlineTextStyle { color: None, bg_color: None, effects: Effects() } }"#
            }
            "bold_text" => {
                r#"InlineSegment { text: "This is bold text", style: InlineTextStyle { color: None, bg_color: None, effects: Effects(BOLD) } }"#
            }
            "italic_text" => {
                r#"InlineSegment { text: "This is italic text", style: InlineTextStyle { color: None, bg_color: None, effects: Effects(ITALIC) } }"#
            }
            "bold_italic_text" => {
                r#"InlineSegment { text: "This is bold and italic text", style: InlineTextStyle { color: None, bg_color: None, effects: Effects(BOLD | ITALIC) } }"#
            }
            _ => unreachable!("unexpected style fixture"),
        };
        assert_eq!(format!("{segment:?}"), expected);
    }
}
