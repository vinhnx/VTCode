//! Unified-diff header and metadata parsing helpers.

pub(crate) fn parse_hunk_starts(line: &str) -> Option<(u32, u32)> {
    let (old, _, new, _) = parse_hunk_range(line)?;
    Some((old, new))
}

pub(crate) fn parse_hunk_range(line: &str) -> Option<(u32, usize, u32, usize)> {
    let body = line.strip_prefix("@@ ")?.split(" @@").next()?;
    let mut parts = body.split_whitespace();
    let (old_start, old_count) = parse_range(parts.next()?, '-')?;
    let (new_start, new_count) = parse_range(parts.next()?, '+')?;
    Some((old_start, old_count, new_start, new_count))
}

fn parse_range(value: &str, marker: char) -> Option<(u32, usize)> {
    let range = value.strip_prefix(marker)?;
    let mut parts = range.splitn(2, ',');
    let start = parts.next()?.parse().ok()?;
    let count = parts.next().map_or(Some(1), |count| count.parse().ok())?;
    Some((start, count))
}

pub(crate) fn parse_omitted_line_count(line: &str) -> Option<usize> {
    let line = line.trim();
    line.strip_prefix("... ")?.strip_suffix(" lines omitted ...")?.parse().ok()
}

pub(crate) fn is_unified_metadata_line(line: &str) -> bool {
    line.starts_with("--- ")
        || line.starts_with("+++ ")
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
        || line == "GIT binary patch"
        || line.starts_with("literal ")
        || line.starts_with("delta ")
}

pub(crate) fn trim_line_ending(text: &str) -> &str {
    text.strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
        .or_else(|| text.strip_suffix('\r'))
        .unwrap_or(text)
}
