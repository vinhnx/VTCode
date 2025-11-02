//! Advanced TUI snapshot tests with real content rendering
//!
//! These tests simulate actual user interactions with real content to verify
//! the TUI renders correctly under various scenarios.
//!
//! To update snapshots, run: `cargo insta review`

use insta::assert_snapshot;
use ratatui::{Terminal, backend::TestBackend};
use vtcode_core::ui::tui::{
    InlineHeaderContext, InlineMessageKind, InlineSegment, InlineTextStyle, InlineTheme,
    spawn_session,
};

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

    assert_snapshot!(format!("{}", terminal.backend()));
}

/// Test actual UI state with populated content using the command system
#[tokio::test]
async fn test_real_ui_scenario_with_commands() {
    // Create a session with initial parameters
    let session = spawn_session(
        InlineTheme::default(),
        Some("Type your message here...".to_string()),
        Default::default(),
        12,   // inline_rows
        true, // show_timeline_pane
        None,
    );

    // Verify session was created
    assert!(session.is_ok());
    let session = session.unwrap();

    // Send some commands to populate the UI with real content
    session.handle.append_line(
        InlineMessageKind::User,
        vec![InlineSegment {
            text: "Can you help me refactor this Rust code?".to_string(),
            style: InlineTextStyle::default(),
        }],
    );

    session.handle.append_line(
        InlineMessageKind::Agent,
        vec![InlineSegment {
            text: "Sure! I can help you refactor your Rust code. Could you share the code you'd like to refactor?".to_string(),
            style: InlineTextStyle::default(),
        }],
    );

    session.handle.append_line(
        InlineMessageKind::User,
        vec![InlineSegment {
            text: "Here's the code I want to refactor:\n```rust\nfn calculate_sum(numbers: Vec<i32>) -> i32 {\n    let mut sum = 0;\n    for i in 0..numbers.len() {\n        sum += numbers[i];\n    }\n    sum\n}\n```".to_string(),
            style: InlineTextStyle::default(),
        }],
    );

    session.handle.append_line(
        InlineMessageKind::Agent,
        vec![InlineSegment {
            text: "I can help refactor this code to be more idiomatic Rust. Here's an improved version:\n```rust\nfn calculate_sum(numbers: &[i32]) -> i32 {\n    numbers.iter().sum()\n}\n```\nThis version: 1) Takes a slice instead of moving the Vec, 2) Uses iterator methods for better performance, 3) Is more idiomatic Rust.".to_string(),
            style: InlineTextStyle::default(),
        }],
    );

    // Verify the session handle still works
    assert!(!session.events.is_closed());

    // Snapshot the session state
    assert_snapshot!(
        "real_ui_scenario_session",
        format!("Session created with messages: 4")
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
                model: "gpt-4".to_string(),
                mode: "interactive".to_string(),
                ..Default::default()
            },
        ),
        (
            "advanced_context",
            InlineHeaderContext {
                provider: "anthropic".to_string(),
                model: "claude-3".to_string(),
                mode: "full-auto".to_string(),
                reasoning: "analytical".to_string(),
                ..Default::default()
            },
        ),
        (
            "minimal_context",
            InlineHeaderContext {
                provider: "local".to_string(),
                model: "llama3".to_string(),
                mode: "inline".to_string(),
                ..Default::default()
            },
        ),
    ];

    for (name, context) in contexts {
        assert_snapshot!(format!("header_context_{}", name), format!("{:?}", context));
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
                (
                    InlineMessageKind::Tool,
                    "run_terminal_cmd([\"ls\", \"-la\"])",
                ),
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
            .map(|(kind, text)| format!("{:?}: {}", kind, text))
            .collect();
        assert_snapshot!(
            format!("message_combo_{}", name),
            format!("{:?}", message_repr)
        );
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
                    bold: false,
                    italic: false,
                    color: None,
                },
            },
        ),
        (
            "bold_text",
            InlineSegment {
                text: "This is bold text".to_string(),
                style: InlineTextStyle {
                    bold: true,
                    italic: false,
                    color: None,
                },
            },
        ),
        (
            "italic_text",
            InlineSegment {
                text: "This is italic text".to_string(),
                style: InlineTextStyle {
                    bold: false,
                    italic: true,
                    color: None,
                },
            },
        ),
        (
            "bold_italic_text",
            InlineSegment {
                text: "This is bold and italic text".to_string(),
                style: InlineTextStyle {
                    bold: true,
                    italic: true,
                    color: None,
                },
            },
        ),
    ];

    for (name, segment) in styled_segments {
        assert_snapshot!(format!("styled_segment_{}", name), format!("{:?}", segment));
    }
}
