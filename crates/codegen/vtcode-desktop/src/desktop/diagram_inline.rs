//! Inline Mermaid diagrams: content addressing, theme mapping, and the
//! transcript image pipeline.
//!
//! Headless rendering itself lives in `vtcode-diagram` (sandboxed Pele
//! execution plus CSS-var inlining, shared with the `diagram render` CLI);
//! this module feeds the transcript's inline diagram images. Closed mermaid
//! fences register here under a content hash, the Markdown rewrite appends a
//! `diagram:<sha256>` image reference, and [`super::diagram_store`] serves
//! the rasterized bytes through its image cache.

use std::collections::{HashMap, VecDeque, hash_map::Entry};
use std::sync::{Mutex, OnceLock};

use gpui_kit::component::theme::ThemeColor;
use sha2::{Digest, Sha256};

/// Theme substitution map for [`vtcode_diagram::inline_css_vars`], derived
/// from the same variable table as the browser preview stylesheet so both
/// paths theme identically.
pub(super) fn inline_theme(colors: &ThemeColor) -> HashMap<String, String> {
    super::diagram::pele_var_colors(colors)
        .into_iter()
        .map(|(name, color)| (name.to_string(), super::diagram::css_hex(color)))
        .collect()
}

// ---------------------------------------------------------------------------
// Inline image pipeline: content addressing, caches, worker, image hook.
// ---------------------------------------------------------------------------

/// Custom image URL scheme for inline diagrams: `diagram:<sha256 hex>`.
const DIAGRAM_URL_SCHEME: &str = "diagram:";

/// SHA-256 over diagram source bytes: the stable session-wide identity of
/// one diagram, embedded in image URLs and cache keys.
pub(super) type SourceHash = [u8; 32];

/// Sources retained for diagrams the transcript prepared. 128 entries cap
/// worst-case retention near 6MB; evicted entries fail their images closed
/// (the code fence and browser preview stay available).
const SOURCE_REGISTRY_CAP: usize = 128;

/// Insertion-ordered bounded map. Entries are content-addressed immutables,
/// so plain FIFO eviction is correct: evicting early only costs a re-render.
pub(super) struct FifoMap<K, V> {
    map: HashMap<K, V>,
    order: VecDeque<K>,
    cap: usize,
}

impl<K, V> FifoMap<K, V>
where
    K: Eq + std::hash::Hash + Clone,
{
    /// Empty map evicting past `cap` entries.
    pub(super) fn new(cap: usize) -> Self {
        Self { map: HashMap::new(), order: VecDeque::new(), cap }
    }

    /// Look up without affecting eviction order.
    pub(super) fn get(&self, key: &K) -> Option<&V> {
        self.map.get(key)
    }

    /// Insert, evicting oldest entries past capacity. Returns evicted pairs
    /// so callers can adjust their own accounting.
    pub(super) fn insert(&mut self, key: K, value: V) -> Vec<(K, V)> {
        match self.map.entry(key) {
            Entry::Occupied(mut slot) => {
                let _previous = slot.insert(value);
                Vec::new()
            }
            Entry::Vacant(slot) => {
                self.order.push_back(slot.key().clone());
                let _slot = slot.insert(value);
                let mut evicted = Vec::new();
                while self.map.len() > self.cap {
                    let Some(oldest) = self.pop_oldest() else {
                        break;
                    };
                    evicted.push(oldest);
                }
                evicted
            }
        }
    }

    /// Remove and return the oldest entry, if any.
    pub(super) fn pop_oldest(&mut self) -> Option<(K, V)> {
        let key = self.order.pop_front()?;
        let value = self.map.remove(&key)?;
        Some((key, value))
    }
}

/// Content-addressed diagram sources, shared by the pure Markdown rewrite
/// (which runs without an App context) and the image hook. Values are
/// immutable: identical concurrent inserts are idempotent.
static DIAGRAM_SOURCES: OnceLock<Mutex<FifoMap<SourceHash, String>>> = OnceLock::new();

/// Register one diagram source under its content hash.
pub(super) fn register_diagram_source(source: &str) -> SourceHash {
    let hash = source_hash(source);
    let registry = DIAGRAM_SOURCES.get_or_init(|| Mutex::new(FifoMap::new(SOURCE_REGISTRY_CAP)));
    // The guarded section cannot panic (hash-map insert only), so a poisoned
    // mutex is unreachable; skipping registration then degrades to a failed
    // image instead of a poisoned panic.
    if let Ok(mut guard) = registry.lock() {
        let _evicted = guard.insert(hash, source.to_string());
    }
    hash
}

/// Look up a previously registered source by its content hash.
pub(super) fn diagram_source(hash: &SourceHash) -> Option<String> {
    DIAGRAM_SOURCES.get()?.lock().ok()?.get(hash).cloned()
}

/// Content hash of one diagram source.
pub(super) fn source_hash(source: &str) -> SourceHash {
    Sha256::digest(source.as_bytes()).into()
}

/// Lowercase hex of a content hash, for `diagram:` image URLs.
pub(super) fn source_hash_hex(hash: &SourceHash) -> String {
    let mut out = String::with_capacity(hash.len() * 2);
    for byte in hash {
        for nibble in [byte >> 4, byte & 0x0f] {
            if let Some(digit) = char::from_digit(u32::from(nibble), 16) {
                out.push(digit);
            }
        }
    }
    out
}

/// Parse a `diagram:<64 hex>` image URL back to its content hash.
/// Anything else is `None`: the image hook fails closed to blocked.
pub(super) fn parse_diagram_uri(uri: &str) -> Option<SourceHash> {
    let hex = uri.strip_prefix(DIAGRAM_URL_SCHEME)?;
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut hash = [0u8; 32];
    for (slot, chunk) in hash.iter_mut().zip(hex.as_bytes().chunks(2)) {
        let pair = std::str::from_utf8(chunk).ok()?;
        *slot = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(hash)
}

/// Image reference appended after a mermaid fence: blank-line-separated so
/// it parses as its own block. The single writer to [`parse_diagram_uri`]'s
/// reader; the alt text drops `]` so no translation can break the syntax.
pub(super) fn diagram_image_markdown(hash: &SourceHash, alt: &str) -> String {
    format!("\n\n![{}]({DIAGRAM_URL_SCHEME}{})\n", alt.replace(']', ""), source_hash_hex(hash))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagram_uri_round_trips_and_rejects_garbage() {
        let hash = register_diagram_source("flowchart TD\n  A --> B");
        let uri = format!("{DIAGRAM_URL_SCHEME}{}", source_hash_hex(&hash));
        assert_eq!(parse_diagram_uri(&uri), Some(hash));
        // Uppercase hex decodes too (encoders always emit lowercase).
        assert_eq!(parse_diagram_uri(&uri.to_uppercase().replace("DIAGRAM", "diagram")), Some(hash));
        for bad in [
            "",
            "diagram:",
            "diagram:xyz",
            "diagram:ab",                              // short
            &format!("diagram:{}00", "ab".repeat(32)), // long
            &format!("diagram:{}zz", "ab".repeat(31)), // non-hex tail
            "http://example.test/diagram",
            " diagram:00",
        ] {
            assert_eq!(parse_diagram_uri(bad), None, "{bad:?} must not parse");
        }
    }

    #[test]
    fn image_markdown_sanitizes_alt_and_matches_the_parser() {
        let hash = register_diagram_source("pie title P\n  \"A\" : 1");
        let markdown = diagram_image_markdown(&hash, "Rendered diagram");
        assert!(markdown.starts_with("\n\n!["), "block-separated: {markdown:?}");
        let uri = markdown.split('(').nth(1).expect("url part").trim_end_matches(")\n");
        assert_eq!(parse_diagram_uri(uri), Some(hash), "writer matches reader");
        assert_eq!(diagram_source(&hash).as_deref(), Some("pie title P\n  \"A\" : 1"));
        // A hostile translation cannot break the image syntax: the only
        // `]` left is the legitimate alt terminator.
        let hostile = diagram_image_markdown(&hash, "a]b](diagram:00");
        assert!(hostile.contains("![ab(diagram:00](diagram:"), "alt sanitized: {hostile:?}");
        assert_eq!(hostile.matches(']').count(), 1, "single terminator: {hostile:?}");
        assert_eq!(parse_diagram_uri(&format!("diagram:{}", source_hash_hex(&hash))), Some(hash));
    }

    #[test]
    fn fifo_map_evicts_oldest_and_replaces_in_place() {
        let mut fifo = FifoMap::new(2);
        assert!(fifo.insert("a", 1).is_empty());
        assert!(fifo.insert("b", 2).is_empty());
        assert_eq!(fifo.get(&"a"), Some(&1));
        let evicted = fifo.insert("c", 3);
        assert_eq!(evicted, vec![("a", 1)], "oldest evicted");
        assert_eq!(fifo.get(&"a"), None);
        assert_eq!(fifo.get(&"c"), Some(&3));
        // Replacing keeps the entry (and evicts nothing).
        assert!(fifo.insert("b", 20).is_empty());
        assert_eq!(fifo.get(&"b"), Some(&20));
        assert_eq!(fifo.pop_oldest(), Some(("b", 20)));
        assert_eq!(fifo.pop_oldest(), Some(("c", 3)));
        assert_eq!(fifo.pop_oldest(), None);
    }

    #[test]
    fn inline_theme_covers_every_pele_token_with_hex() {
        let mut colors = ThemeColor::default();
        colors.background = gpui_kit::Hsla { h: 0.0, s: 0.0, l: 1.0, a: 1.0 };
        colors.foreground = gpui_kit::Hsla { h: 0.0, s: 0.0, l: 0.0, a: 1.0 };
        let map = inline_theme(&colors);
        assert_eq!(map.len(), 18, "one entry per variable");
        for (name, value) in &map {
            let hex = value.strip_prefix('#').unwrap_or("");
            assert!(hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()), "{name} is #rrggbb");
        }
        assert_eq!(map["--pele-bg"], "#ffffff");
        assert_eq!(map["--pele-fg"], "#000000");
    }
}
