use vtcode_tui::{InlineHeaderContext, InlineTheme, SessionOptions, spawn_session_with_options};

fn main() {
    let _header = InlineHeaderContext::default();
    let _theme = InlineTheme::default();

    // Keep this example non-interactive in CI.
    if std::env::var("VTCODE_TUI_RUN_EXAMPLES").is_ok() {
        let options = SessionOptions {
            placeholder: Some("Prompt...".to_string()),
            ..SessionOptions::default()
        };
        let _ = spawn_session_with_options(InlineTheme::default(), options);
    }
}
