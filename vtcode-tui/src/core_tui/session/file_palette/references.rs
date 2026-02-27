pub fn extract_file_reference(input: &str, cursor: usize) -> Option<(usize, usize, String)> {
    if cursor == 0 || cursor > input.len() {
        return None;
    }

    let bytes = input.as_bytes();
    let mut start = cursor;

    while start > 0 && bytes[start - 1] != b'@' && !bytes[start - 1].is_ascii_whitespace() {
        start -= 1;
    }

    if start == 0 || bytes[start - 1] != b'@' {
        return None;
    }

    start -= 1;

    // Check context: if @ is preceded by command-like text, skip it
    let is_npm_context = is_npm_command_context(input, start);

    let mut end = cursor;
    while end < bytes.len() && !bytes[end].is_ascii_whitespace() {
        end += 1;
    }

    let reference = &input[start + 1..end];

    // Ensure the extracted reference looks like a file path, not a package specifier
    if !looks_like_file_path(reference, is_npm_context) {
        return None;
    }

    Some((start, end, reference.to_owned()))
}

/// Check if @ is used in npm command context (e.g., @scope/package)
fn is_npm_command_context(input: &str, at_pos: usize) -> bool {
    // Check if preceded by package manager commands: npm, npx, yarn, pnpm, bun
    let before_at = input[..at_pos].trim_end();
    let cmd_names = ["npm", "npx", "yarn", "pnpm", "bun"];

    // Check if any command appears in the command line
    for cmd in &cmd_names {
        // Look for the command at word boundaries
        // e.g., "npm install @scope/pkg" or "npm i @scope/pkg"
        let bytes = before_at.as_bytes();
        let cmd_bytes = cmd.as_bytes();

        // Check if it starts with the command
        if bytes.len() >= cmd_bytes.len() {
            // Check beginning
            if &bytes[..cmd_bytes.len()] == cmd_bytes {
                // Verify it's a word boundary (followed by space or end of string)
                if cmd_bytes.len() == bytes.len() || bytes[cmd_bytes.len()].is_ascii_whitespace() {
                    return true;
                }
            }
        }

        // Also check after spaces (in case of leading whitespace)
        if let Some(pos) = before_at.find(cmd) {
            // Check if preceded by space or start
            let is_word_start = pos == 0 || before_at.as_bytes()[pos - 1].is_ascii_whitespace();
            // Check if followed by space or end
            let is_word_end = pos + cmd.len() == before_at.len()
                || before_at.as_bytes()[pos + cmd.len()].is_ascii_whitespace();

            if is_word_start && is_word_end {
                return true;
            }
        }
    }

    false
}

/// Check if the reference looks like a file path vs package specifier
/// `is_npm_context`: whether @ appears in npm command context (affects bare identifier handling)
fn looks_like_file_path(reference: &str, is_npm_context: bool) -> bool {
    // Allow empty (bare @) to show file picker with all files
    if reference.is_empty() {
        return true;
    }

    // Reject anything that contains @ (scoped packages or version specs like @scope/pkg@1.0.0)
    if reference.contains('@') {
        return false;
    }

    let has_separator = reference.contains('/') || reference.contains('\\');
    let has_extension = reference.contains('.');

    // Relative paths with dot prefix: ./path, ../path
    if reference.starts_with("./") || reference.starts_with("../") {
        return true;
    }

    // Absolute paths: /path, ~/path
    if reference.starts_with('/') || reference.starts_with("~/") {
        return true;
    }

    // Windows absolute paths: C:\path, C:/path
    if reference.len() > 2 && reference.as_bytes()[1] == b':' {
        let sep = reference.as_bytes()[2];
        if sep == b'\\' || sep == b'/' {
            return true;
        }
    }

    // Paths with separators AND extensions: src/main.rs, foo/bar/file.ts
    // This distinguishes from packages like @scope/package (no extension)
    if has_separator && has_extension {
        return true;
    }

    // Simple filename with extension: main.rs, index.ts, image.png
    if !has_separator && has_extension {
        return true;
    }

    // In npm command context, reject bare identifiers (likely package names)
    // e.g., "npm i @types" where "types" is a package scope
    if is_npm_context {
        return false;
    }

    // In normal conversation context, allow bare identifiers for file picker
    // e.g., "choose @files" or "edit @config"
    if !has_separator && !has_extension {
        return true;
    }

    false
}
