//! Offline Mermaid diagram rendering through the vendored Pele bundle.
//!
//! This crate is the single headless render engine for diagrams: the `vtcode
//! diagram render` CLI and the desktop app's inline previews both render
//! through [`DiagramSandbox`]. The sandbox executes the vendored Pele bundle
//! (`assets/pele.global.js`, provenance in `assets/PROVENANCE.md`) inside a
//! capped QuickJS runtime (bounded heap, stack, and wall-clock budget, no
//! module loader, no I/O intrinsics), so untrusted diagram source can never
//! touch the network or the filesystem.
//!
//! Two contracts pin the Pele coupling:
//!
//! - [`MAX_DIAGRAM_SOURCE_CHARS`] matches Pele's own default `limit`
//!   (UTF-16 units, like `source.length`); larger sources fail closed before
//!   any JS executes.
//! - Render options stay fixed (`links: false`, `images: false`); the SVG is
//!   inert by construction.
//!
//! Pele emits `var(--pele-*)` references with light-theme fallbacks plus
//! document-local `--_*` aliases, so [`RenderedSvg`] renders standalone in a
//! browser. Consumers that rasterize natively resolve the variables first
//! with [`inline_css_vars`] against their own theme map.

mod inline;
mod sandbox;

pub use inline::inline_css_vars;
pub use sandbox::{DiagramSandbox, RenderError, RenderedSvg};

/// Vendored Pele browser bundle (global `pele` with `render` and `mount`).
/// Version and checksum live in `assets/PROVENANCE.md`.
pub const PELE_BUNDLE_JS: &str = include_str!("../assets/pele.global.js");

/// Largest diagram source accepted for rendering, in UTF-16 code units.
/// Matches Pele's own default `limit` exactly (it enforces
/// `source.length`, which counts UTF-16 units, not scalar values).
pub const MAX_DIAGRAM_SOURCE_CHARS: usize = 50_000;

/// First closed ```mermaid fence body, or `None`. Mirrors the WebMCP
/// extractor (`apps/webmcp/src/diagrams.ts`): the opener is ``` followed by
/// `mermaid` (ASCII case-insensitive) plus an optional info tail, the closer
/// is the first all-backtick run, and unclosed fences never match.
#[must_use]
pub fn extract_mermaid_fence(content: &str) -> Option<&str> {
    let mut start: Option<usize> = None;
    let mut offset = 0;
    for line in content.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        let body = body.strip_suffix('\r').unwrap_or(body);
        match start {
            None => {
                if is_mermaid_fence_open(body) {
                    start = Some(offset + line.len());
                }
            }
            Some(from) => {
                if is_fence_close(body) {
                    let raw = content.get(from..offset)?;
                    return Some(raw.strip_suffix("\r\n").or_else(|| raw.strip_suffix('\n')).unwrap_or(raw));
                }
            }
        }
        offset += line.len();
    }
    None
}

fn is_mermaid_fence_open(line: &str) -> bool {
    let rest = line.strip_prefix("```").unwrap_or("").trim_start_matches([' ', '\t']);
    let name = rest.get(.."mermaid".len()).unwrap_or("");
    if !name.eq_ignore_ascii_case("mermaid") {
        return false;
    }
    match rest.get("mermaid".len()..) {
        Some(tail) => tail.is_empty() || tail.starts_with([' ', '\t']),
        None => false,
    }
}

fn is_fence_close(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.len() >= 3 && trimmed.bytes().all(|byte| byte == b'`')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagram_limit_matches_pele_default() {
        // Coupled to Pele's documented default `limit` option.
        assert_eq!(MAX_DIAGRAM_SOURCE_CHARS, 50_000);
    }

    #[test]
    fn fence_extraction_takes_the_first_closed_mermaid_block_only() {
        assert_eq!(extract_mermaid_fence("```mermaid\nflowchart TD\n  A --> B\n```"), Some("flowchart TD\n  A --> B"));
        assert_eq!(extract_mermaid_fence("```MERMAID\nA\n```"), Some("A"));
        assert_eq!(extract_mermaid_fence("```mermaid title here\nA\n```"), Some("A"));
        assert_eq!(extract_mermaid_fence("```rust\ncode\n```\n```mermaid\nA\n```"), Some("A"));
        assert_eq!(extract_mermaid_fence("```mermaid\nfirst\n```\n```mermaid\nsecond\n```"), Some("first"));
        assert_eq!(extract_mermaid_fence("```mermaid\nA\n````"), Some("A"));
        // First closing run wins, even mid-block: documented, not nested.
        assert_eq!(extract_mermaid_fence("```mermaid\nA\n```\nB\n```"), Some("A"));
        // CRLF sources slice on the same boundaries.
        assert_eq!(extract_mermaid_fence("```mermaid\r\nA\r\n```\r\n"), Some("A"));
        assert_eq!(extract_mermaid_fence("```mermaidjs\nA\n```"), None);
        assert_eq!(extract_mermaid_fence("````mermaid\nA\n````"), None);
        assert_eq!(extract_mermaid_fence("```mermaid\nunclosed"), None);
        assert_eq!(extract_mermaid_fence("no fences here"), None);
    }
}
