pub fn extract_file_reference(input: &str, cursor: usize) -> Option<(usize, usize, String)> {
    if cursor > input.len() {
        return None;
    }

    let bytes = input.as_bytes();

    // Find the start of the whitespace-delimited token containing the cursor
    let mut token_start = cursor;
    while token_start > 0 && !bytes[token_start - 1].is_ascii_whitespace() {
        token_start -= 1;
    }

    // Find the end of the token
    let mut token_end = cursor;
    while token_end < bytes.len() && !bytes[token_end].is_ascii_whitespace() {
        token_end += 1;
    }

    let token = &input[token_start..token_end];

    // Check if token starts with @
    if !token.starts_with('@') {
        // Token doesn't start with @, but cursor might be on a nested @
        // e.g., @scope/pkg@version - cursor on the second @
        // In this case, we should NOT trigger file picker for nested @
        return None;
    }

    // @ must be at a whitespace boundary (token_start) or at start of input
    // This prevents mid-word @ like foo@bar from triggering
    let prefix_starts_token = token_start == 0
        || input[..token_start]
            .chars()
            .next_back()
            .is_none_or(char::is_whitespace);

    if !prefix_starts_token {
        return None;
    }

    // Check context: if @ is preceded by package manager commands, skip it
    let is_npm_context = is_npm_command_context(input, token_start);

    // Extract reference (without the leading @)
    let reference = &token[1..];

    // Ensure the extracted reference looks like a file path, not a package specifier
    if !looks_like_file_path(reference, is_npm_context) {
        return None;
    }

    Some((token_start, token_end, reference.to_owned()))
}

/// Check if @ is used in npm command context (e.g., @scope/package)
fn is_npm_command_context(input: &str, at_pos: usize) -> bool {
    let before_at = &input[..at_pos];
    let cmd_names = ["npm", "npx", "yarn", "pnpm", "bun"];

    // Check if any package manager command appears as a whole word before @
    cmd_names
        .iter()
        .any(|&cmd| before_at.split_whitespace().any(|word| word == cmd))
}

/// Check if the reference looks like a file path vs package specifier
/// `is_npm_context`: whether @ appears in npm command context (affects bare identifier handling)
fn looks_like_file_path(reference: &str, is_npm_context: bool) -> bool {
    // Allow empty (bare @) to show file picker with all files
    if reference.is_empty() {
        return true;
    }

    let has_extension = reference.contains('.');

    // If reference contains @, check if it looks like a file with @ in the name
    // vs a npm package specifier (e.g., icon@2x.png vs pkg@1.0.0)
    if reference.contains('@') {
        if is_npm_context {
            return false;
        }
        // Check if part after last @ looks like a filename (has letters) vs version (numbers only)
        return reference.rsplit_once('@').is_some_and(|(_, after)| {
            after.contains(|c: char| c.is_ascii_alphabetic()) && after.contains('.')
        });
    }

    // Path patterns: ./path, ../path, /path, ~/path, C:\path
    if reference.starts_with("./")
        || reference.starts_with("../")
        || reference.starts_with('/')
        || reference.starts_with("~/")
        || (reference.len() > 2
            && reference.as_bytes()[1] == b':'
            && matches!(reference.as_bytes()[2], b'\\' | b'/'))
    {
        return true;
    }

    // File with extension (with or without separator)
    if has_extension {
        return true;
    }

    // npm context rejects bare identifiers; normal context allows them
    if is_npm_context {
        return false;
    }

    true
}
