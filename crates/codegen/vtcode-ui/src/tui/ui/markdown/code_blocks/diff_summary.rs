use super::*;

pub(super) fn diff_path_for_language(line: &str) -> Option<String> {
    parse_diff_git_path(line).or_else(|| parse_diff_marker_path(line)).or_else(|| {
        ["*** Update File:", "*** Add File:", "*** Delete File:"]
            .iter()
            .find_map(|prefix| line.strip_prefix(prefix).map(str::trim).filter(|path| !path.is_empty()))
            .map(ToOwned::to_owned)
    })
}

pub(super) fn parse_diff_summary_line(line: &str) -> Option<(&str, usize, usize)> {
    let summary = line.strip_prefix(DIFF_SUMMARY_PREFIX)?;
    let (path, counts) = summary.rsplit_once(" (")?;
    let counts = counts.strip_suffix(')')?;
    let mut parts = counts.split_whitespace();
    let additions = parts.next()?.strip_prefix('+')?.parse().ok()?;
    let deletions = parts.next()?.strip_prefix('-')?.parse().ok()?;
    Some((path, additions, deletions))
}

pub(super) fn format_diff_summary(path: &str, additions: usize, deletions: usize) -> String {
    format!("{DIFF_SUMMARY_PREFIX}{path} (+{additions} -{deletions})")
}
