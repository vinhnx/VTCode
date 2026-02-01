//! Unified formatting utilities for UI and logging

/// Format file size in human-readable form (KB, MB, GB, etc.)
pub fn format_size(size: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if size >= GB {
        format!("{:.1}GB", size as f64 / GB as f64)
    } else if size >= MB {
        format!("{:.1}MB", size as f64 / MB as f64)
    } else if size >= KB {
        format!("{:.1}KB", size as f64 / KB as f64)
    } else {
        format!("{}B", size)
    }
}

/// Indent a block of text with the given prefix
pub fn indent_block(text: &str, indent: &str) -> String {
    if indent.is_empty() || text.is_empty() {
        return text.to_string();
    }
    let mut indented = String::with_capacity(text.len() + indent.len() * text.lines().count());
    for (idx, line) in text.split('\n').enumerate() {
        if idx > 0 {
            indented.push('\n');
        }
        if !line.is_empty() {
            indented.push_str(indent);
        }
        indented.push_str(line);
    }
    indented
}

/// Truncate text to a maximum length with an optional ellipsis
pub fn truncate_text(text: &str, max_len: usize, ellipsis: &str) -> String {
    if text.len() <= max_len {
        return text.to_string();
    }

    let mut truncated = text.chars().take(max_len).collect::<String>();
    truncated.push_str(ellipsis);
    truncated
}
