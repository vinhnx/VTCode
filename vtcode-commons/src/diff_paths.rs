use std::path::Path;

/// Parse `diff --git a/... b/...` line and return normalized new path.
pub fn parse_diff_git_path(line: &str) -> Option<String> {
    let mut parts = line.split_whitespace();
    if parts.next()? != "diff" || parts.next()? != "--git" {
        return None;
    }
    let _old = parts.next()?;
    let new_path = parts.next()?;
    Some(new_path.trim_start_matches("b/").to_string())
}

/// Parse unified diff marker line (`---`/`+++`) and return normalized path.
pub fn parse_diff_marker_path(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if !(is_diff_old_file_marker_line(trimmed) || is_diff_new_file_marker_line(trimmed)) {
        return None;
    }
    let path = trimmed.split_whitespace().nth(1)?;
    if path == "/dev/null" {
        return None;
    }
    Some(
        path.trim_start_matches("a/")
            .trim_start_matches("b/")
            .to_string(),
    )
}

/// Convert file path to language hint based on extension.
pub fn language_hint_from_path(path: &str) -> Option<String> {
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .filter(|ext| !ext.is_empty())
        .map(|ext| ext.to_ascii_lowercase())
}

/// Whether a line is a unified diff addition content line (`+...`, excluding `+++` marker).
pub fn is_diff_addition_line(line: &str) -> bool {
    line.starts_with('+') && !line.starts_with("+++")
}

/// Whether a line is a unified diff removal content line (`-...`, excluding `---` marker).
pub fn is_diff_deletion_line(line: &str) -> bool {
    line.starts_with('-') && !line.starts_with("---")
}

/// Whether a line is a unified diff old-file marker (`--- ...`).
pub fn is_diff_old_file_marker_line(line: &str) -> bool {
    line.starts_with("--- ")
}

/// Whether a line is a unified diff new-file marker (`+++ ...`).
pub fn is_diff_new_file_marker_line(line: &str) -> bool {
    line.starts_with("+++ ")
}

/// Whether a line is an apply_patch operation header.
pub fn is_apply_patch_header_line(line: &str) -> bool {
    line.starts_with("*** Begin Patch")
        || line.starts_with("*** Update File:")
        || line.starts_with("*** Add File:")
        || line.starts_with("*** Delete File:")
}

/// Whether a line is a recognized diff metadata/header line.
pub fn is_diff_header_line(line: &str) -> bool {
    line.starts_with("diff --git ")
        || line.starts_with("@@")
        || line.starts_with("index ")
        || line.starts_with("new file mode ")
        || line.starts_with("deleted file mode ")
        || line.starts_with("rename from ")
        || line.starts_with("rename to ")
        || line.starts_with("copy from ")
        || line.starts_with("copy to ")
        || line.starts_with("similarity index ")
        || line.starts_with("dissimilarity index ")
        || line.starts_with("old mode ")
        || line.starts_with("new mode ")
        || line.starts_with("Binary files ")
        || line.starts_with("\\ No newline at end of file")
        || is_diff_new_file_marker_line(line)
        || is_diff_old_file_marker_line(line)
        || is_apply_patch_header_line(line)
}

/// Heuristic classifier for unified/git diff content.
///
/// This intentionally avoids classifying plain source code containing `+`/`-`
/// lines as a diff unless there are structural diff markers.
pub fn looks_like_diff_content(content: &str) -> bool {
    let mut has_git_header = false;
    let mut has_hunk = false;
    let mut has_old_marker = false;
    let mut has_new_marker = false;
    let mut has_add = false;
    let mut has_del = false;
    let mut has_binary_or_mode_header = false;
    let mut has_apply_patch = false;

    for raw in content.lines() {
        let line = raw.trim_start();
        if line.is_empty() {
            continue;
        }

        if line.starts_with("diff --git ") {
            has_git_header = true;
            continue;
        }
        if line.starts_with("@@") {
            has_hunk = true;
            continue;
        }
        if is_diff_old_file_marker_line(line) {
            has_old_marker = true;
            continue;
        }
        if is_diff_new_file_marker_line(line) {
            has_new_marker = true;
            continue;
        }
        if is_apply_patch_header_line(line) {
            has_apply_patch = true;
            continue;
        }
        if line.starts_with("new file mode ")
            || line.starts_with("deleted file mode ")
            || line.starts_with("rename from ")
            || line.starts_with("rename to ")
            || line.starts_with("copy from ")
            || line.starts_with("copy to ")
            || line.starts_with("similarity index ")
            || line.starts_with("dissimilarity index ")
            || line.starts_with("old mode ")
            || line.starts_with("new mode ")
            || line.starts_with("Binary files ")
            || line.starts_with("index ")
            || line.starts_with("\\ No newline at end of file")
        {
            has_binary_or_mode_header = true;
            continue;
        }

        if is_diff_addition_line(line) {
            has_add = true;
            continue;
        }
        if is_diff_deletion_line(line) {
            has_del = true;
        }
    }

    if has_apply_patch {
        return true;
    }
    if has_git_header && (has_hunk || has_old_marker || has_new_marker || has_binary_or_mode_header)
    {
        return true;
    }
    if has_hunk && (has_old_marker || has_new_marker || has_add || has_del) {
        return true;
    }
    if has_old_marker && has_new_marker && (has_add || has_del) {
        return true;
    }

    false
}

/// Parse unified diff hunk header starts from `@@ -old,+new @@`.
pub fn parse_hunk_starts(line: &str) -> Option<(usize, usize)> {
    let trimmed = line.trim_end();
    let rest = trimmed.strip_prefix("@@ -")?;
    let mut parts = rest.split_whitespace();
    let old_part = parts.next()?;
    let new_part = parts.next()?;
    if !new_part.starts_with('+') {
        return None;
    }

    let old_start = old_part.split(',').next()?.parse::<usize>().ok()?;
    let new_start = new_part
        .trim_start_matches('+')
        .split(',')
        .next()?
        .parse::<usize>()
        .ok()?;
    Some((old_start, new_start))
}

/// Normalize hunk header to start-only form: `@@ -old +new @@`.
pub fn format_start_only_hunk_header(line: &str) -> Option<String> {
    let (old_start, new_start) = parse_hunk_starts(line)?;
    Some(format!("@@ -{} +{} @@", old_start, new_start))
}

#[cfg(test)]
mod tests {
    use super::{
        format_start_only_hunk_header, is_apply_patch_header_line, is_diff_addition_line,
        is_diff_deletion_line, is_diff_header_line, is_diff_new_file_marker_line,
        is_diff_old_file_marker_line, language_hint_from_path, looks_like_diff_content,
        parse_diff_git_path, parse_diff_marker_path, parse_hunk_starts,
    };

    #[test]
    fn parses_git_diff_path() {
        let line = "diff --git a/src/lib.rs b/src/lib.rs";
        assert_eq!(parse_diff_git_path(line).as_deref(), Some("src/lib.rs"));
    }

    #[test]
    fn parses_marker_path() {
        assert_eq!(
            parse_diff_marker_path("+++ b/src/main.rs").as_deref(),
            Some("src/main.rs")
        );
        assert_eq!(parse_diff_marker_path("--- /dev/null"), None);
    }

    #[test]
    fn infers_language_hint_from_extension() {
        assert_eq!(
            language_hint_from_path("src/main.RS").as_deref(),
            Some("rs")
        );
        assert_eq!(language_hint_from_path("Makefile"), None);
    }

    #[test]
    fn parses_hunk_starts() {
        assert_eq!(parse_hunk_starts("@@ -536,4 +540,5 @@"), Some((536, 540)));
        assert_eq!(parse_hunk_starts("not a hunk"), None);
    }

    #[test]
    fn formats_start_only_hunk_header() {
        assert_eq!(
            format_start_only_hunk_header("@@ -536,4 +540,5 @@"),
            Some("@@ -536 +540 @@".to_string())
        );
    }

    #[test]
    fn detects_diff_add_remove_lines() {
        assert!(is_diff_addition_line("+added"));
        assert!(!is_diff_addition_line("+++ b/file.rs"));
        assert!(is_diff_deletion_line("-removed"));
        assert!(!is_diff_deletion_line("--- a/file.rs"));
    }

    #[test]
    fn detects_diff_header_lines() {
        assert!(is_diff_header_line("diff --git a/a b/a"));
        assert!(is_diff_header_line("@@ -1 +1 @@"));
        assert!(is_diff_header_line("+++ b/src/main.rs"));
        assert!(!is_diff_header_line("println!(\"diff --git\");"));
    }

    #[test]
    fn detects_marker_and_apply_patch_header_lines() {
        assert!(is_diff_old_file_marker_line("--- a/src/lib.rs"));
        assert!(is_diff_new_file_marker_line("+++ b/src/lib.rs"));
        assert!(is_apply_patch_header_line("*** Update File: src/lib.rs"));
        assert!(!is_apply_patch_header_line("*** End Patch"));
    }

    #[test]
    fn classifies_git_diff_content() {
        let diff = "diff --git a/a.rs b/a.rs\n@@ -1 +1 @@\n-old\n+new\n";
        assert!(looks_like_diff_content(diff));
    }

    #[test]
    fn classifies_apply_patch_content() {
        let patch = "*** Begin Patch\n*** Update File: a.rs\n@@\n-old\n+new\n*** End Patch\n";
        assert!(looks_like_diff_content(patch));
    }

    #[test]
    fn avoids_false_positive_for_regular_code() {
        let code =
            "fn delta(x: i32) -> i32 {\n    let y = x + 1;\n    let z = x - 1;\n    y + z\n}\n";
        assert!(!looks_like_diff_content(code));
    }

    #[test]
    fn avoids_false_positive_for_plus_minus_logs() {
        let log = "+ started service\n- previous pid cleaned\n";
        assert!(!looks_like_diff_content(log));
    }
}
