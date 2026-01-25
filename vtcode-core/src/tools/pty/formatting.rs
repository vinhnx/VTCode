use crate::tools::types::VTCodePtySession;

/// Sanitize session ID for use in filename
pub(super) fn sanitize_session_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect()
}

/// Format terminal session as a file with metadata header
pub(super) fn format_terminal_file(session: &VTCodePtySession, output: &str) -> String {
    let mut content = String::new();

    // Metadata header
    content.push_str("---\n");
    content.push_str(&format!("session_id: {}\n", session.id));
    content.push_str(&format!("command: {}\n", session.command));
    if !session.args.is_empty() {
        content.push_str(&format!("args: {}\n", session.args.join(" ")));
    }
    if let Some(cwd) = &session.working_dir {
        content.push_str(&format!("cwd: {}\n", cwd));
    }
    content.push_str(&format!("size: {}x{}\n", session.cols, session.rows));
    content.push_str("---\n\n");

    // Terminal output
    content.push_str(output);

    content
}
