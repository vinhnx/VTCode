use super::*;

#[cfg_attr(feature = "profiling", hotpath::measure)]
pub(crate) fn detect_rendered_transcript_links(
    line: &Line<'_>,
    workspace_root: Option<&Path>,
) -> Vec<RenderedTranscriptLink> {
    if !may_contain_link_candidate(line, &[]) {
        return Vec::new();
    }

    let text = transcript_line_text(line);
    detect_transcript_link_matches(&text, workspace_root)
        .into_iter()
        .filter_map(|DetectedLinkMatch { start, end, target }| {
            let width = UnicodeWidthStr::width(&text[start..end]);
            if width == 0 {
                return None;
            }

            Some(RenderedTranscriptLink {
                start,
                end,
                start_col: UnicodeWidthStr::width(&text[..start]),
                width,
                target: InlineLinkTarget::Url(transcript_link_target_string(&target)),
            })
        })
        .collect()
}

pub(super) fn detect_transcript_link_matches(text: &str, workspace_root: Option<&Path>) -> Vec<DetectedLinkMatch> {
    let mut matches = Vec::new();

    for quoted_match in QUOTED_PATH_PATTERN.find_iter(text) {
        if let Some(link_match) =
            build_transcript_link_match(text, quoted_match.start(), quoted_match.end(), workspace_root)
        {
            matches.push(link_match);
        }
    }

    for token_match in NON_WHITESPACE_TOKEN_PATTERN.find_iter(text) {
        if matches
            .iter()
            .any(|existing| token_match.start() < existing.end && token_match.end() > existing.start)
        {
            continue;
        }

        if let Some(link_match) =
            build_transcript_link_match(text, token_match.start(), token_match.end(), workspace_root)
        {
            matches.push(link_match);
        }
    }

    matches.sort_by_key(|link_match| link_match.start);
    matches.dedup_by(|left, right| left.start == right.start && left.end == right.end);

    matches
}

pub(super) fn transcript_link_target_string(target: &TranscriptLinkTarget) -> String {
    match target {
        TranscriptLinkTarget::File(target) => target.canonical_string(),
        TranscriptLinkTarget::Url(url) => url.clone(),
    }
}

fn build_transcript_link_match(
    text: &str,
    token_start: usize,
    token_end: usize,
    workspace_root: Option<&Path>,
) -> Option<DetectedLinkMatch> {
    let token = &text[token_start..token_end];
    let (trimmed_start, trimmed_end) = trim_transcript_token_bounds(token);
    if trimmed_start >= trimmed_end {
        return None;
    }

    let start = token_start + trimmed_start;
    let end = token_start + trimmed_end;
    let candidate = &text[start..end];
    let target = resolve_transcript_url(candidate)
        .map(TranscriptLinkTarget::Url)
        .or_else(|| resolve_transcript_file_target(candidate, workspace_root).map(TranscriptLinkTarget::File))?;

    Some(DetectedLinkMatch { start, end, target })
}

pub(super) fn resolve_transcript_file_target(token: &str, workspace_root: Option<&Path>) -> Option<EditorTarget> {
    let token = token.trim();
    // Location parsing cannot make an ordinary word into a path candidate.
    // Percent escapes can introduce path syntax, so they must still be decoded.
    if !token.bytes().any(|byte| matches!(byte, b'.' | b'/' | b'\\' | b'%')) {
        return None;
    }

    let parsed = parse_editor_target(token)?;
    let stripped = parsed.path().to_string_lossy();
    if stripped.is_empty() || !looks_like_transcript_path(&stripped) {
        return None;
    }

    let resolved = if parsed.path().is_absolute() {
        parsed
    } else {
        let root = workspace_root?;
        parsed.with_resolved_path(root)
    };

    resolved.path().is_file().then_some(resolved)
}

fn resolve_transcript_url(token: &str) -> Option<String> {
    let trimmed = token.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        Some(trimmed.to_string())
    } else {
        None
    }
}

fn looks_like_transcript_path(token: &str) -> bool {
    // Explicit prefixes, drive letters, and separators are shared with the
    // markdown detector; the extension fallback below favors recall because
    // `path.is_file()` is the final filter.
    if has_explicit_path_syntax(token) {
        return true;
    }
    // Has a file-like extension: non-empty stem + dot + 1-12 alphanumeric ext.
    // Called after suffix stripping so `a.rs` (not `a.rs:10`) is the input.
    // `path.is_file()` is the final filter, so favor recall here.
    token.rsplit_once('.').is_some_and(|(stem, ext)| {
        !stem.is_empty() && !ext.is_empty() && ext.len() <= 12 && ext.chars().all(|c| c.is_ascii_alphanumeric())
    })
}
