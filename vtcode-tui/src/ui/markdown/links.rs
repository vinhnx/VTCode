use super::MarkdownSegment;
use regex::Regex;
use std::sync::LazyLock;

pub(crate) static COLON_LOCATION_SUFFIX_RE: LazyLock<Regex> =
    LazyLock::new(|| {
        Regex::new(r":\d+(?::\d+)?(?:[-–]\d+(?::\d+)?)?$")
            .expect("invalid location suffix regex")
    });

pub(crate) static HASH_LOCATION_SUFFIX_RE: LazyLock<Regex> =
    LazyLock::new(|| {
        Regex::new(r"^L\d+(?:C\d+)?(?:-L\d+(?:C\d+)?)?$").expect("invalid hash location regex")
    });

pub(crate) fn should_render_link_destination(dest_url: &str) -> bool {
    !is_local_path_like_link(dest_url)
}

pub(crate) fn label_has_location_suffix(text: &str) -> bool {
    text.rsplit_once('#')
        .is_some_and(|(_, fragment)| HASH_LOCATION_SUFFIX_RE.is_match(fragment))
        || COLON_LOCATION_SUFFIX_RE.find(text).is_some()
}

pub(crate) fn label_segments_have_location_suffix(segments: &[MarkdownSegment]) -> bool {
    let Some(last) = segments.last() else {
        return false;
    };
    if label_has_location_suffix(&last.text) {
        return true;
    }
    if segments.len() == 1 {
        return false;
    }

    let mut label = String::with_capacity(segments.iter().map(|s| s.text.len()).sum());
    for segment in segments {
        label.push_str(&segment.text);
    }
    label_has_location_suffix(&label)
}

pub(crate) fn extract_hidden_location_suffix(dest_url: &str) -> Option<String> {
    if !is_local_path_like_link(dest_url) {
        return None;
    }

    if let Some((_, fragment)) = dest_url.rsplit_once('#')
        && HASH_LOCATION_SUFFIX_RE.is_match(fragment)
    {
        return normalize_hash_location(fragment);
    }

    COLON_LOCATION_SUFFIX_RE
        .find(dest_url)
        .map(|m| m.as_str().to_string())
}

pub(crate) fn normalize_hash_location(fragment: &str) -> Option<String> {
    let (start, end) = match fragment.split_once('-') {
        Some((start, end)) => (start, Some(end)),
        None => (fragment, None),
    };

    let (start_line, start_col) = parse_hash_point(start)?;
    let mut result = format!(":{start_line}");
    if let Some(col) = start_col {
        result.push(':');
        result.push_str(col);
    }

    if let Some(end) = end {
        let (end_line, end_col) = parse_hash_point(end)?;
        result.push('-');
        result.push_str(end_line);
        if let Some(col) = end_col {
            result.push(':');
            result.push_str(col);
        }
    }

    Some(result)
}

fn is_local_path_like_link(dest_url: &str) -> bool {
    dest_url.starts_with("file://")
        || dest_url.starts_with('/')
        || dest_url.starts_with("~/")
        || dest_url.starts_with("./")
        || dest_url.starts_with("../")
        || dest_url.starts_with("\\\\")
        || matches!(
            dest_url.as_bytes(),
            [drive, b':', separator, ..]
                if drive.is_ascii_alphabetic() && matches!(separator, b'/' | b'\\')
        )
}

fn parse_hash_point(point: &str) -> Option<(&str, Option<&str>)> {
    let point = point.strip_prefix('L')?;
    Some(match point.split_once('C') {
        Some((line, col)) => (line, Some(col)),
        None => (point, None),
    })
}
