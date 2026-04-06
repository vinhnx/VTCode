use std::env;
use std::path::{Path, PathBuf};

use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorPoint {
    pub line: usize,
    pub column: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorTarget {
    path: PathBuf,
    location_suffix: Option<String>,
}

impl EditorTarget {
    #[must_use]
    pub fn new(path: PathBuf, location_suffix: Option<String>) -> Self {
        Self {
            path,
            location_suffix,
        }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn location_suffix(&self) -> Option<&str> {
        self.location_suffix.as_deref()
    }

    #[must_use]
    pub fn with_resolved_path(mut self, base: &Path) -> Self {
        self.path = resolve_editor_path(&self.path, base);
        self
    }

    #[must_use]
    pub fn canonical_string(&self) -> String {
        let mut target = self.path.display().to_string();
        if let Some(location) = self.location_suffix() {
            target.push_str(location);
        }
        target
    }

    #[must_use]
    pub fn point(&self) -> Option<EditorPoint> {
        let suffix = self.location_suffix()?.strip_prefix(':')?;
        if suffix.contains('-') {
            return None;
        }

        let mut parts = suffix.split(':');
        let line = parts.next()?.parse().ok()?;
        let column = parts.next().map(str::parse).transpose().ok().flatten();
        if parts.next().is_some() {
            return None;
        }

        Some(EditorPoint { line, column })
    }
}

#[must_use]
pub fn parse_editor_target(raw: &str) -> Option<EditorTarget> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }

    if raw.starts_with("http://") || raw.starts_with("https://") {
        return None;
    }
    if raw.contains("://") && !raw.starts_with("file://") {
        return None;
    }

    if raw.starts_with("file://") {
        let url = Url::parse(raw).ok()?;
        let location_suffix = url
            .fragment()
            .and_then(normalize_editor_hash_fragment)
            .or_else(|| extract_trailing_location(url.path()));
        let path = url.to_file_path().ok()?;
        return Some(EditorTarget::new(path, location_suffix));
    }

    if let Some((path_str, fragment)) = raw.split_once('#')
        && let Some(location_suffix) = normalize_editor_hash_fragment(fragment)
    {
        if path_str.is_empty() {
            return None;
        }
        return Some(EditorTarget::new(
            expand_home_relative_path(path_str).unwrap_or_else(|| PathBuf::from(path_str)),
            Some(location_suffix),
        ));
    }

    if let Some(paren_start) = location_paren_suffix_start(raw) {
        let location_suffix = parse_paren_location_suffix(&raw[paren_start..])?;
        let path_str = &raw[..paren_start];
        if path_str.is_empty() {
            return None;
        }

        return Some(EditorTarget::new(
            expand_home_relative_path(path_str).unwrap_or_else(|| PathBuf::from(path_str)),
            Some(location_suffix),
        ));
    }

    let location_suffix = extract_trailing_location(raw);
    let path_str = match location_suffix.as_deref() {
        Some(suffix) => &raw[..raw.len().saturating_sub(suffix.len())],
        None => raw,
    };
    if path_str.is_empty() {
        return None;
    }

    Some(EditorTarget::new(
        expand_home_relative_path(path_str).unwrap_or_else(|| PathBuf::from(path_str)),
        location_suffix,
    ))
}

#[must_use]
pub fn resolve_editor_target(raw: &str, base: &Path) -> Option<EditorTarget> {
    parse_editor_target(raw).map(|target| target.with_resolved_path(base))
}

#[must_use]
pub fn resolve_editor_path(path: &Path, base: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }

    let mut joined = PathBuf::from(base);
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                joined.pop();
            }
            other => joined.push(other.as_os_str()),
        }
    }
    joined
}

fn expand_home_relative_path(path: &str) -> Option<PathBuf> {
    let remainder = path
        .strip_prefix("~/")
        .or_else(|| path.strip_prefix("~\\"))?;
    let home = env::var_os("HOME").or_else(|| env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(remainder))
}

fn extract_trailing_location(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut idx = bytes.len();
    while idx > 0 && (bytes[idx - 1].is_ascii_digit() || matches!(bytes[idx - 1], b':' | b'-')) {
        idx -= 1;
    }
    if idx >= bytes.len() || bytes.get(idx).copied() != Some(b':') {
        return None;
    }

    let suffix = &raw[idx..];
    let digits = suffix.chars().filter(|ch| ch.is_ascii_digit()).count();
    (digits > 0).then(|| suffix.to_string())
}

fn location_paren_suffix_start(token: &str) -> Option<usize> {
    let paren_start = token.rfind('(')?;
    let inner = token[paren_start + 1..].strip_suffix(')')?;
    let valid = !inner.is_empty()
        && !inner.starts_with(',')
        && !inner.ends_with(',')
        && !inner.contains(",,")
        && inner.chars().all(|c| c.is_ascii_digit() || c == ',');
    valid.then_some(paren_start)
}

fn parse_paren_location_suffix(suffix: &str) -> Option<String> {
    let inner = suffix.strip_prefix('(')?.strip_suffix(')')?;
    if inner.is_empty() {
        return None;
    }

    let mut parts = inner.split(',');
    let line = parts.next()?;
    let column = parts.next();
    if parts.next().is_some() {
        return None;
    }

    if line.is_empty() || !line.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }

    let mut normalized = format!(":{line}");
    if let Some(column) = column {
        if column.is_empty() || !column.chars().all(|ch| ch.is_ascii_digit()) {
            return None;
        }
        normalized.push(':');
        normalized.push_str(column);
    }

    Some(normalized)
}

#[must_use]
pub fn normalize_editor_hash_fragment(fragment: &str) -> Option<String> {
    let (start, end) = match fragment.split_once('-') {
        Some((start, end)) => (start, Some(end)),
        None => (fragment, None),
    };

    let (start_line, start_col) = parse_hash_point(start)?;
    let mut normalized = format!(":{start_line}");
    if let Some(col) = start_col {
        normalized.push(':');
        normalized.push_str(col);
    }

    if let Some(end) = end {
        let (end_line, end_col) = parse_hash_point(end)?;
        normalized.push('-');
        normalized.push_str(end_line);
        if let Some(col) = end_col {
            normalized.push(':');
            normalized.push_str(col);
        }
    }

    Some(normalized)
}

fn parse_hash_point(point: &str) -> Option<(&str, Option<&str>)> {
    let point = point.strip_prefix('L')?;
    let (line, column) = match point.split_once('C') {
        Some((line, column)) => (line, Some(column)),
        None => (point, None),
    };
    if line.is_empty() || !line.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    if let Some(column) = column
        && (column.is_empty() || !column.chars().all(|ch| ch.is_ascii_digit()))
    {
        return None;
    }
    Some((line, column))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_colon_location_suffix() {
        let target = parse_editor_target("/tmp/demo.rs:12:4").expect("target");
        assert_eq!(target.path(), Path::new("/tmp/demo.rs"));
        assert_eq!(target.location_suffix(), Some(":12:4"));
        assert_eq!(
            target.point(),
            Some(EditorPoint {
                line: 12,
                column: Some(4)
            })
        );
    }

    #[test]
    fn parses_paren_location_suffix() {
        let target = parse_editor_target("/tmp/demo.rs(12,4)").expect("target");
        assert_eq!(target.path(), Path::new("/tmp/demo.rs"));
        assert_eq!(target.location_suffix(), Some(":12:4"));
    }

    #[test]
    fn parses_hash_location_suffix() {
        let target = parse_editor_target("/tmp/demo.rs#L12C4").expect("target");
        assert_eq!(target.path(), Path::new("/tmp/demo.rs"));
        assert_eq!(target.location_suffix(), Some(":12:4"));
    }

    #[test]
    fn normalizes_hash_location_ranges() {
        assert_eq!(
            normalize_editor_hash_fragment("L74C3-L76C9"),
            Some(":74:3-76:9".to_string())
        );
        assert_eq!(
            normalize_editor_hash_fragment("L74-L76"),
            Some(":74-76".to_string())
        );
        assert_eq!(normalize_editor_hash_fragment("L"), None);
        assert_eq!(normalize_editor_hash_fragment("L74-"), None);
        assert_eq!(normalize_editor_hash_fragment("L74C"), None);
    }

    #[test]
    fn hash_ranges_preserve_suffix_but_not_point() {
        let target = parse_editor_target("/tmp/demo.rs#L12-L18").expect("target");
        assert_eq!(target.location_suffix(), Some(":12-18"));
        assert_eq!(target.point(), None);
    }

    #[test]
    fn file_urls_are_supported() {
        let target = parse_editor_target("file:///tmp/demo.rs#L12").expect("target");
        assert_eq!(target.path(), Path::new("/tmp/demo.rs"));
        assert_eq!(target.location_suffix(), Some(":12"));
    }

    #[test]
    fn non_file_urls_are_rejected() {
        assert!(parse_editor_target("https://example.com/file.rs").is_none());
    }

    #[test]
    fn resolves_relative_paths_against_base() {
        let target =
            resolve_editor_target("src/lib.rs:12", Path::new("/workspace")).expect("target");
        assert_eq!(target.path(), Path::new("/workspace/src/lib.rs"));
        assert_eq!(target.location_suffix(), Some(":12"));
        assert_eq!(target.canonical_string(), "/workspace/src/lib.rs:12");
    }
}
