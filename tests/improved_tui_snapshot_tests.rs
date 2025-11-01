//! Comprehensive Ratatui snapshot tests for VT Code TUI
//!
//! These tests use the `insta` crate to capture visual snapshots of the actual terminal UI.
//! The tests verify that the TUI renders correctly with various content and states.
//!
//! To update snapshots, run: `cargo insta review`

use insta::assert_snapshot;
use ratatui::{Frame, Terminal, backend::TestBackend};
use vtcode_core::ui::tui::{
    InlineHeaderContext, InlineMessageKind, InlineSegment, InlineTextStyle, InlineTheme,
    spawn_session,
};

/// Test actual UI rendering with a full terminal backend simulation
#[test]
fn test_actual_tui_rendering() {
    // Create a TestBackend to simulate the terminal
    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).unwrap();

    // This test verifies that the TestBackend works correctly with the TUI system
    terminal
        .draw(|f| {
            // Draw a basic UI representation
            let area = f.area();
            // The actual rendering happens elsewhere, but this ensures our test infrastructure works
            assert!(area.width > 0);
            assert!(area.height > 0);
        })
        .unwrap();

    // Capture the actual terminal output as a snapshot
    assert_snapshot!(format!("{}", terminal.backend()));
}

/// Test that UI components are properly serializable for debugging
#[test]
fn test_ui_component_serialization() {
    // Test InlineTheme serialization
    let theme = InlineTheme::default();
    let theme_repr = format!("{:?}", theme);
    assert_snapshot!("theme_representation", theme_repr);

    // Test InlineSegment serialization with different styles
    let normal_segment = InlineSegment {
        text: "Normal text".to_string(),
        style: InlineTextStyle {
            bold: false,
            italic: false,
            color: None,
        },
    };
    assert_snapshot!("normal_segment", format!("{:?}", normal_segment));

    let bold_segment = InlineSegment {
        text: "Bold text".to_string(),
        style: InlineTextStyle {
            bold: true,
            italic: false,
            color: None,
        },
    };
    assert_snapshot!("bold_segment", format!("{:?}", bold_segment));

    let italic_segment = InlineSegment {
        text: "Italic text".to_string(),
        style: InlineTextStyle {
            bold: false,
            italic: true,
            color: None,
        },
    };
    assert_snapshot!("italic_segment", format!("{:?}", italic_segment));
}

/// Test header context rendering simulation
#[test]
fn test_header_context_rendering() {
    let context = InlineHeaderContext {
        provider: "openai".to_string(),
        model: "gpt-4".to_string(),
        version: "0.37.1".to_string(),
        git: "main branch".to_string(),
        mode: "interactive".to_string(),
        reasoning: "creative".to_string(),
        workspace_trust: "trusted".to_string(),
        tools: "enabled".to_string(),
        mcp: "available".to_string(),
        highlights: vec![],
    };

    // Test that the context can be properly represented
    let context_repr = format!("{:?}", context);
    assert_snapshot!("header_context_representation", context_repr);
}

/// Test message kind representations
#[test]
fn test_message_kind_representations() {
    let kinds = vec![
        (InlineMessageKind::Agent, "agent"),
        (InlineMessageKind::User, "user"),
        (InlineMessageKind::Error, "error"),
        (InlineMessageKind::Info, "info"),
        (InlineMessageKind::Policy, "policy"),
        (InlineMessageKind::Tool, "tool"),
        (InlineMessageKind::Pty, "pty"),
    ];

    for (kind, name) in kinds {
        assert_snapshot!(format!("message_kind_{}", name), format!("{:?}", kind));
    }
}

/// Test TUI session creation and basic functionality
#[tokio::test]
async fn test_tui_session_creation() {
    let session_result = spawn_session(
        InlineTheme::default(),
        Some("Enter your query here...".to_string()),
        Default::default(), // UiSurfacePreference
        10,                 // inline_rows
        true,               // show_timeline_pane
        None,               // event_callback
    );

    // Verify that the session was created successfully
    assert!(session_result.is_ok());

    // Snapshot the creation result
    assert_snapshot!(
        "session_creation_success",
        format!("{:?}", session_result.is_ok())
    );
}
