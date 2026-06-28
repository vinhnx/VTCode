//! Snapshot tests for the Ratatui TUI components
//!
//! These tests use the `insta` crate to capture snapshots of the UI components and logic.
//! Since direct TUI rendering requires internal access, we test the UI logic and components
//! that feed into the TUI rendering pipeline.
//!
//! To update snapshots, run: `cargo insta review`

use insta::assert_snapshot;
use vtcode_core::ui::{InlineHeaderContext, InlineMessageKind, InlineSegment, InlineTextStyle};

/// Test message kind string representation
#[test]
fn test_message_kind_snapshot() {
    let agent_kind = format!("{:?}", InlineMessageKind::Agent);
    let user_kind = format!("{:?}", InlineMessageKind::User);
    let error_kind = format!("{:?}", InlineMessageKind::Error);

    assert_snapshot!(&agent_kind, @"Agent");
    assert_snapshot!(&user_kind, @"User");
    assert_snapshot!(&error_kind, @"Error");
}

/// Test message segment representation
#[test]
fn test_inline_segment_snapshot() {
    let segment = InlineSegment {
        text: "Hello, world!".to_string(),
        style: InlineTextStyle::default().bold().into(),
    };
    assert_snapshot!(
        &format!("{segment:?}"),
        @"InlineSegment { text: \"Hello, world!\", style: InlineTextStyle { color: None, bg_color: None, effects: Effects(BOLD) } }"
    );
}

/// Test header context representation
#[test]
fn test_header_context_snapshot() {
    let context = InlineHeaderContext {
        app_name: "VT Code".to_string(),
        provider: "openai".to_string(),
        model: "gpt-oss-20b".to_string(),
        context_window_size: None,
        reasoning: "creative".to_string(),
        reasoning_stage: None,
        workspace_trust: "trusted".to_string(),
        tools: "enabled".to_string(),
        git: "clean".to_string(),
        mcp: "disabled".to_string(),
        primary_agent: None,
        primary_agent_color: None,
        highlights: vec![],
        subagent_badges: vec![],
        version: "0.37.1".to_string(),
        search_tools: None,
        persistent_memory: None,
        pr_review: None,
        editor_context: None,
    };
    assert_snapshot!(
        &format!("{context:?}"),
        @"InlineHeaderContext { app_name: \"VT Code\", provider: \"openai\", model: \"gpt-oss-20b\", context_window_size: None, version: \"0.37.1\", search_tools: None, persistent_memory: None, pr_review: None, editor_context: None, git: \"clean\", reasoning: \"creative\", reasoning_stage: None, workspace_trust: \"trusted\", tools: \"enabled\", mcp: \"disabled\", primary_agent: None, primary_agent_color: None, highlights: [], subagent_badges: [] }"
    );
}

/// Test inline command debugging output
#[test]
fn test_inline_command_debug() {
    let segment = InlineSegment {
        text: "Hello! I'm your AI assistant.".to_string(),
        style: InlineTextStyle::default().bold().into(),
    };

    // Test string representation
    let debug_output = format!("{:?}", InlineMessageKind::Agent);
    assert_snapshot!(&debug_output, @"Agent");

    let segment_debug = format!("{segment:?}");
    assert_snapshot!(
        &segment_debug,
        @"InlineSegment { text: \"Hello! I'm your AI assistant.\", style: InlineTextStyle { color: None, bg_color: None, effects: Effects(BOLD) } }"
    );
}

/// Test UI component combinations
#[test]
fn test_ui_component_combinations() {
    let message_segment = InlineSegment {
        text: "This is a test message with styling".to_string(),
        style: InlineTextStyle::default().bold().italic().into(),
    };

    let context = InlineHeaderContext {
        provider: "anthropic".to_string(),
        model: "claude-3".to_string(),
        reasoning_stage: None,
        version: "test-version".to_string(),
        ..Default::default()
    };

    let combined_repr = format!("Message: {message_segment:?}\nContext: {context:?}");

    assert_snapshot!(
        &combined_repr,
        @r###"
    Message: InlineSegment { text: "This is a test message with styling", style: InlineTextStyle { color: None, bg_color: None, effects: Effects(BOLD | ITALIC) } }
    Context: InlineHeaderContext { app_name: "App", provider: "anthropic", model: "claude-3", context_window_size: None, version: "test-version", search_tools: None, persistent_memory: None, pr_review: None, editor_context: None, git: "git: unavailable", reasoning: "unavailable", reasoning_stage: None, workspace_trust: "Trust: unavailable", tools: "Tools: unavailable", mcp: "MCP: unavailable", primary_agent: None, primary_agent_color: None, highlights: [], subagent_badges: [] }
    "###
    );
}
