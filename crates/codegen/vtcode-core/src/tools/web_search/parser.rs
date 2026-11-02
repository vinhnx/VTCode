//! Pure DuckDuckGo HTML extraction and result URL validation.

use anyhow::{Result, anyhow};
use astral_html::Token;
use url::Url;

use super::{SearchResult, truncate_chars};

const MAX_TITLE_CHARS: usize = 200;
const MAX_SNIPPET_CHARS: usize = 400;

/// Parse one DDG response and reject known anti-bot pages before they can be
/// mistaken for a legitimate empty result set. A normal no-hit page has no
/// result anchors and no challenge markers, so it remains a valid empty
/// response and can be cached briefly.
pub(super) fn parse_duckduckgo_response(body: &str, max_results: usize) -> Result<Vec<SearchResult>> {
    let results = parse_duckduckgo_html(body, max_results);
    if results.is_empty() && is_duckduckgo_challenge_page(body) {
        return Err(anyhow!(
            "DuckDuckGo returned an anti-bot challenge page instead of search results; wait before retrying from this network"
        ));
    }
    Ok(results)
}

fn is_duckduckgo_challenge_page(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    [
        "anomaly detected",
        "captcha",
        "challenge-form",
        "unusual traffic",
        "verify you are human",
        "are you a robot",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

/// Extract ranked results from a DuckDuckGo HTML body. Pure function so it can
/// be exercised in unit tests against a local fixture (no live network).
///
/// Single streaming pass: a result anchor opens title collection, its first
/// `</a>` completes it, and the following `result__snippet` element (before
/// the next result anchor) supplies the snippet. First-close-wins keeps
/// parity with the previous extractor on well-formed markup.
pub fn parse_duckduckgo_html(body: &str, max_results: usize) -> Vec<SearchResult> {
    let mut results = Vec::new();
    let mut state = DdgSection::Idle;
    for token in astral_html::Reader::new(body) {
        if results.len() >= max_results {
            break;
        }
        match token {
            Token::StartTag(tag) => {
                if tag.name == "a" && has_ddg_class(&tag, "result__a") {
                    // A new result anchor ends the pending one (keeping an
                    // empty snippet) and drops any partial buffers.
                    if let DdgSection::AfterTitle { url, title } = std::mem::replace(&mut state, DdgSection::Idle) {
                        push_ddg_result(&mut results, url, title, String::new(), max_results);
                        if results.len() >= max_results {
                            break;
                        }
                    }
                    if !tag.self_closing {
                        if let Some(href) = ddg_attr(&tag, "href") {
                            state = DdgSection::InTitle { href: href.to_string(), buf: String::new() };
                        }
                    }
                } else if matches!(state, DdgSection::AfterTitle { .. }) && has_ddg_class(&tag, "result__snippet") {
                    if let DdgSection::AfterTitle { url, title } = std::mem::replace(&mut state, DdgSection::Idle) {
                        if tag.self_closing {
                            push_ddg_result(&mut results, url, title, String::new(), max_results);
                        } else {
                            state = DdgSection::InSnippet {
                                url,
                                title,
                                name: tag.name.to_string(),
                                buf: String::new(),
                            };
                        }
                    }
                }
                // All other tags are content or noise: nested formatting
                // text arrives as Text tokens; nothing to do here.
            }
            Token::Text(text) => match &mut state {
                DdgSection::InTitle { buf, .. } | DdgSection::InSnippet { buf, .. } => {
                    buf.push_str(&text);
                }
                DdgSection::Idle | DdgSection::AfterTitle { .. } => {}
            },
            Token::EndTag(tag) => {
                if tag.name == "a" && matches!(state, DdgSection::InTitle { .. }) {
                    if let DdgSection::InTitle { href, buf } = std::mem::replace(&mut state, DdgSection::Idle) {
                        let title = buf.trim().to_string();
                        if !title.is_empty() {
                            if let Some(url) = normalize_ddg_url(&href) {
                                state = DdgSection::AfterTitle { url, title };
                            }
                        }
                    }
                } else if matches!(state, DdgSection::InSnippet { .. }) {
                    let done = if let DdgSection::InSnippet { name, .. } = &state {
                        tag.name == *name
                    } else {
                        false
                    };
                    if done {
                        if let DdgSection::InSnippet { url, title, buf, .. } =
                            std::mem::replace(&mut state, DdgSection::Idle)
                        {
                            push_ddg_result(&mut results, url, title, buf, max_results);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    // Trailing result without a snippet element keeps an empty snippet.
    if let DdgSection::AfterTitle { url, title } = state {
        push_ddg_result(&mut results, url, title, String::new(), max_results);
    }
    results
}

/// Tokenizer position while extracting one result: idle, collecting the
/// anchor title, holding a completed title, or collecting its snippet.
enum DdgSection {
    Idle,
    InTitle {
        href: String,
        buf: String,
    },
    AfterTitle {
        url: String,
        title: String,
    },
    InSnippet {
        url: String,
        title: String,
        name: String,
        buf: String,
    },
}

/// Decoded value of attribute `name` on `tag`, if present. Attribute names
/// are ASCII-lowercased by the tokenizer, so plain equality applies.
fn ddg_attr(tag: &astral_html::Tag<'_>, name: &str) -> Option<String> {
    tag.attributes
        .iter()
        .find(|attr| attr.name == name)
        .map(|attr| attr.value().to_string())
}

/// Whether the tag's `class` attribute carries `class` as a whitespace
/// separated token (ASCII case-insensitive, matching DDG's lowercase).
fn has_ddg_class(tag: &astral_html::Tag<'_>, class: &str) -> bool {
    tag.attributes
        .iter()
        .find(|attr| attr.name == "class")
        .is_some_and(|attr| attr.value().split_whitespace().any(|token| token.eq_ignore_ascii_case(class)))
}

/// Append one validated result unless the cap is reached. Callers validate
/// the URL and require a non-empty title before reaching `AfterTitle`, so
/// this only truncates and pushes.
fn push_ddg_result(results: &mut Vec<SearchResult>, url: String, title: String, snippet: String, max_results: usize) {
    if results.len() >= max_results {
        return;
    }
    results.push(SearchResult {
        title: truncate_chars(&title, MAX_TITLE_CHARS),
        url,
        snippet: truncate_chars(&snippet, MAX_SNIPPET_CHARS),
    });
}

/// Resolve a DuckDuckGo result href into a real https/http URL.
///
/// DDG wraps targets in a redirector like `//duckduckgo.com/l/?uddg=<encoded>`.
/// Returns `None` for non-http(s) schemes (defends against `javascript:` etc).
fn normalize_ddg_url(href: &str) -> Option<String> {
    let absolute = if let Some(stripped) = href.strip_prefix("//") {
        format!("https://{stripped}")
    } else {
        href.to_string()
    };

    let parsed = Url::parse(&absolute).ok()?;
    if let Some((_, target)) = parsed.query_pairs().find(|(k, _)| k == "uddg") {
        let target = target.into_owned();
        return validate_result_url(&target);
    }
    validate_result_url(&absolute)
}

/// Only allow http(s) result URLs with a host.
fn validate_result_url(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    match parsed.scheme() {
        "http" | "https" => {}
        _ => return None,
    }
    if parsed.host_str().is_none_or(str::is_empty) {
        return None;
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return None;
    }
    Some(url.to_string())
}

#[cfg(test)]
mod tests;
