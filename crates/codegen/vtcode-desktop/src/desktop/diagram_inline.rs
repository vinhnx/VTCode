//! Inline Mermaid rendering: sandboxed Pele execution, CSS-var inlining,
//! and content addressing.
//!
//! The browser preview ([`super::diagram`]) stays the zoomable escape hatch;
//! this module feeds the transcript's inline diagram images. Untrusted
//! diagram source executes inside a capped QuickJS runtime (bounded heap,
//! stack, and wall-clock budget, no module loader, no I/O intrinsics), and
//! the returned SVG's `var(--pele-*)` references resolve against the app
//! theme, because the native SVG rasterizer cannot interpret CSS variables.
//! The sandbox is `!Send` by design: it lives on the worker thread owned by
//! [`super::diagram_store`], and the transcript only ever touches rendered
//! bytes through that module's image cache.

use std::collections::{HashMap, HashSet, VecDeque, hash_map::Entry};
use std::fmt;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use gpui_kit::component::theme::ThemeColor;
use rquickjs::{Context, Object, Runtime};
use sha2::{Digest, Sha256};

/// Heap cap for one diagram render. Typical renders allocate kilobytes; the
/// cap only binds pathological inputs, which then fail instead of growing.
const JS_HEAP_LIMIT_BYTES: usize = 64 * 1024 * 1024;

/// Stack cap for one diagram render. Pele's generated parsers recurse over
/// diagram nesting, so deeply nested hostile input hits this instead of the
/// host stack.
const JS_STACK_LIMIT_BYTES: usize = 1024 * 1024;

/// Budget for evaluating the vendored bundle at sandbox construction. The
/// bundle is trusted first-party-vendored code, so this only guards against
/// a corrupt copy hanging the worker at startup.
const BUNDLE_LOAD_BUDGET: Duration = Duration::from_secs(60);

/// Options pinned for every headless render. Links and images stay off, like
/// the browser preview: the SVG is rasterized, so anchors would be dead
/// weight and remote images a fetch primitive.
const RENDER_EXPRESSION: &str =
    "pele.render(vtcode_diagram_source, {links:false, images:false, idPrefix:'vtcode-inline'})";

/// Failure from [`DiagramSandbox::render`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum InlineRenderError {
    /// The source exceeds [`super::diagram::MAX_DIAGRAM_SOURCE_CHARS`]
    /// UTF-16 units; rejected before any JS executes.
    TooLarge,
    /// The sandbox or Pele itself failed: bundle load, JS exception, or an
    /// exhausted time budget. Carries the JS-side message for inert display.
    Js(String),
}

impl fmt::Display for InlineRenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => write!(f, "diagram source exceeds the size limit"),
            Self::Js(message) => write!(f, "diagram render failed: {message}"),
        }
    }
}

impl std::error::Error for InlineRenderError {}

/// Headless Pele output: themed-later SVG plus its intrinsic size.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct RenderedSvg {
    /// Raw SVG; `var(--pele-*)` references still unresolved.
    pub svg: String,
    /// Intrinsic width in SVG units, always finite and positive.
    pub width: f64,
    /// Intrinsic height in SVG units, always finite and positive.
    pub height: f64,
}

/// Pinned QuickJS runtime evaluating the vendored Pele bundle.
///
/// One sandbox lives on the diagram worker thread for the app's lifetime:
/// the bundle loads once, and every render reuses the runtime with a fresh
/// time budget. Neither the runtime nor its context may move threads.
pub(super) struct DiagramSandbox {
    context: Context,
    deadline: Arc<Mutex<Instant>>,
    // Declared last so it drops last: the context must be freed before the
    // runtime it was created from.
    _runtime: Runtime,
}

impl DiagramSandbox {
    /// Load the bundle into a fresh capped runtime. Callers pass
    /// [`super::diagram::PELE_BUNDLE_JS`]; the parameter keeps tests hermetic.
    pub(super) fn new(bundle_js: &str) -> Result<Self, InlineRenderError> {
        let runtime = Runtime::new().map_err(|error| InlineRenderError::Js(format!("{error:?}")))?;
        runtime.set_memory_limit(JS_HEAP_LIMIT_BYTES);
        runtime.set_max_stack_size(JS_STACK_LIMIT_BYTES);
        let deadline = Arc::new(Mutex::new(Instant::now() + BUNDLE_LOAD_BUDGET));
        let handler_deadline = Arc::clone(&deadline);
        // A poisoned mutex means a previous holder panicked mid-update; the
        // only update is an infallible timestamp store, so poisoning is
        // unreachable — fail closed by treating it as expired anyway.
        runtime.set_interrupt_handler(Some(Box::new(move || {
            handler_deadline.lock().map(|due| Instant::now() > *due).unwrap_or(true)
        })));
        let context = Context::full(&runtime).map_err(|error| InlineRenderError::Js(format!("{error:?}")))?;
        context
            .with(|ctx| {
                ctx.eval::<(), _>(bundle_js.as_bytes())
                    .map_err(|error| format_js_error(&ctx, error))
                    .and_then(|()| {
                        ctx.eval::<(), _>(
                            "if (typeof pele === 'undefined' || typeof pele.render !== 'function') \
                         throw new Error('diagram bundle does not define pele.render');",
                        )
                        .map_err(|error| format_js_error(&ctx, error))
                    })
            })
            .map_err(InlineRenderError::Js)?;
        Ok(Self { context, deadline, _runtime: runtime })
    }

    /// Render one diagram source to SVG, enforcing the size gate before any
    /// JS executes and the wall-clock `budget` during execution.
    pub(super) fn render(&self, source: &str, budget: Duration) -> Result<RenderedSvg, InlineRenderError> {
        if source.encode_utf16().count() > super::diagram::MAX_DIAGRAM_SOURCE_CHARS {
            return Err(InlineRenderError::TooLarge);
        }
        *self.deadline.lock().unwrap_or_else(|poison| poison.into_inner()) = Instant::now() + budget;
        self.context.with(|ctx| {
            ctx.globals()
                .set("vtcode_diagram_source", source)
                .map_err(|error| InlineRenderError::Js(format!("{error:?}")))?;
            let result: Object = ctx
                .eval(RENDER_EXPRESSION)
                .map_err(|error| InlineRenderError::Js(format_js_error(&ctx, error)))?;
            let svg: String = result.get("svg").map_err(|error| InlineRenderError::Js(format!("{error:?}")))?;
            let width: f64 = result
                .get("width")
                .map_err(|error| InlineRenderError::Js(format!("{error:?}")))?;
            let height: f64 = result
                .get("height")
                .map_err(|error| InlineRenderError::Js(format!("{error:?}")))?;
            if !(width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0) {
                return Err(InlineRenderError::Js(format!("invalid diagram size {width}x{height}")));
            }
            Ok(RenderedSvg { svg, width, height })
        })
    }
}

fn format_js_error(ctx: &rquickjs::Ctx<'_>, error: rquickjs::Error) -> String {
    if matches!(error, rquickjs::Error::Exception) {
        let caught: rquickjs::Value = ctx.catch();
        if let Some(object) = caught.as_object() {
            let name: String = object.get("name").unwrap_or_else(|_| String::from("Error"));
            let message: String = object.get("message").unwrap_or_else(|_| format!("{caught:?}"));
            return format!("{name}: {message}");
        }
        return format!("exception: {caught:?}");
    }
    format!("{error:?}")
}

/// Theme substitution map for [`inline_css_vars`], derived from the same
/// variable table as the browser preview stylesheet so both paths theme
/// identically.
pub(super) fn inline_theme(colors: &ThemeColor) -> HashMap<String, String> {
    super::diagram::pele_var_colors(colors)
        .into_iter()
        .map(|(name, color)| (name.to_string(), super::diagram::css_hex(color)))
        .collect()
}

/// Substitute `var(--name[, fallback])` inside tag markup only (never in text
/// nodes, where a hostile label could carry a literal `var(`). Theme wins;
/// document `--*` definitions resolve as aliases with a fixpoint cap.
/// Returns the rewritten SVG plus the count of references left unresolved
/// (unknown names without fallbacks, kept literally).
pub(super) fn inline_css_vars(svg: &str, theme: &HashMap<String, String>) -> (String, usize) {
    let defs = collect_custom_props(svg);
    let mut unresolved = 0;
    let mut out = String::with_capacity(svg.len());
    for (in_tag, segment) in split_markup(svg) {
        if in_tag {
            out.push_str(&substitute_vars(segment, theme, &defs, &mut unresolved));
        } else {
            out.push_str(segment);
        }
    }
    (out, unresolved)
}

/// Split SVG into `(in_tag, segment)` runs. Quote-aware: `<`/`>` inside
/// single or double quotes never toggle the mode, so attribute values like
/// `title="a>b"` cannot desynchronize the scanner. Every cut sits on an
/// ASCII delimiter just matched, so `get` only fails if the offset math
/// drifts; breaking early then truncates the scan instead of panicking, and
/// the downstream SVG parse fails closed on the partial document.
fn split_markup(svg: &str) -> Vec<(bool, &str)> {
    let bytes = svg.as_bytes();
    let mut segments = Vec::new();
    let mut index = 0;
    let mut segment_start = 0;
    let mut in_tag = false;
    let mut quote: Option<u8> = None;
    while let Some(byte) = bytes.get(index).copied() {
        if in_tag {
            match byte {
                b'\'' | b'"' if quote.is_none() => quote = Some(byte),
                q if Some(q) == quote => quote = None,
                b'>' if quote.is_none() => {
                    let Some(segment) = svg.get(segment_start..=index) else {
                        break;
                    };
                    segments.push((true, segment));
                    index += 1;
                    segment_start = index;
                    in_tag = false;
                    continue;
                }
                _ => {}
            }
        } else if byte == b'<' {
            if segment_start < index {
                let Some(segment) = svg.get(segment_start..index) else {
                    break;
                };
                segments.push((false, segment));
            }
            segment_start = index;
            in_tag = true;
        }
        index += 1;
    }
    if segment_start < bytes.len()
        && let Some(rest) = svg.get(segment_start..)
    {
        segments.push((in_tag, rest));
    }
    segments
}

/// Collect `--name: value;` definitions (paren-aware) from tag markup only.
/// Text nodes are excluded: a hostile label containing `--x: y;` must not
/// poison the definition map.
fn collect_custom_props(svg: &str) -> HashMap<String, String> {
    let mut defs = HashMap::new();
    for (in_tag, segment) in split_markup(svg) {
        if in_tag {
            collect_from_tag(segment, &mut defs);
        }
    }
    defs
}

fn collect_from_tag(tag: &str, defs: &mut HashMap<String, String>) {
    let bytes = tag.as_bytes();
    let mut index = 0;
    while index + 2 < bytes.len() {
        if bytes.get(index..index + 2) != Some(b"--".as_slice()) {
            index += 1;
            continue;
        }
        let name_start = index;
        let mut cursor = index + 2;
        while let Some(byte) = bytes.get(cursor)
            && (byte.is_ascii_alphanumeric() || *byte == b'-' || *byte == b'_')
        {
            cursor += 1;
        }
        let (Some(name), is_definition) = (tag.get(name_start..cursor), bytes.get(cursor) == Some(&b':')) else {
            // Truly unreachable: both cuts sit on ASCII just matched. Skip
            // forward instead of panicking so malformed input degrades.
            index += 1;
            continue;
        };
        if !is_definition {
            index += 1;
            continue;
        }
        cursor += 1;
        let value_start = cursor;
        let mut depth = 0;
        while let Some(byte) = bytes.get(cursor).copied() {
            match byte {
                b'(' => depth += 1,
                // An unbalanced `)` in malformed input is kept
                // literally instead of underflowing the counter.
                b')' if depth > 0 => depth -= 1,
                b';' if depth == 0 => break,
                _ => {}
            }
            cursor += 1;
        }
        if let Some(value) = tag.get(value_start..cursor) {
            let _previous = defs.insert(name.to_string(), value.trim().to_string());
        }
        // `cursor` always advanced past the name and colon, so this makes
        // progress every iteration.
        index = cursor;
    }
}

/// Maximum alias-chase rounds. Pele emits one alias layer (`--_*` over
/// `--pele-*`); the cap only binds adversarial cycles, which then surface
/// as unresolved instead of hanging the worker.
const INLINE_ROUNDS: usize = 10;

/// One substitution round over tag text, repeated to a fixpoint cap.
fn substitute_vars(
    text: &str,
    theme: &HashMap<String, String>,
    defs: &HashMap<String, String>,
    unresolved: &mut usize,
) -> String {
    // Names, not occurrences: a reference unresolvable in one round stays
    // unresolvable (the theme and definitions never change mid-fixpoint), so
    // recounting it every round would inflate the total.
    let mut unseen = HashSet::new();
    let mut current = text.to_string();
    for _ in 0..INLINE_ROUNDS {
        if !current.contains("var(") {
            break;
        }
        let mut next = String::with_capacity(current.len());
        let mut rest = current.as_str();
        let mut changed = false;
        while let Some(start) = rest.find("var(") {
            // Both cuts sit on the `find` match (ASCII), so `get` only fails
            // on impossible offset drift; stopping the round then keeps the
            // remaining text literal instead of panicking.
            let (Some(before), Some(after)) =
                (rest.get(..start), rest.get(start..).and_then(|tail| tail.strip_prefix("var")))
            else {
                break;
            };
            next.push_str(before);
            match parse_var_call(after) {
                Some((name, fallback, len)) => {
                    let replacement = theme.get(name).or_else(|| defs.get(name)).map(String::as_str).or(fallback);
                    match replacement {
                        Some(value) => {
                            next.push_str(value);
                            changed = true;
                        }
                        None => {
                            if unseen.insert(name.to_string()) {
                                *unresolved += 1;
                            }
                            next.push_str("var");
                            if let Some(call) = after.get(..len) {
                                next.push_str(call);
                            }
                        }
                    }
                    let Some(tail) = after.get(len..) else {
                        break;
                    };
                    rest = tail;
                }
                None => {
                    next.push_str("var");
                    rest = after;
                }
            }
        }
        next.push_str(rest);
        current = next;
        if !changed {
            break;
        }
    }
    current
}

/// Parse `(name[, fallback])` with paren matching; returns the name, the
/// optional fallback, and the consumed length. Commas and parens nest, so
/// only a top-level comma splits and only the matching close ends the call.
fn parse_var_call(after_var: &str) -> Option<(&str, Option<&str>, usize)> {
    let inner = after_var.strip_prefix('(')?;
    let mut depth = 1;
    let mut split = None;
    for (offset, byte) in inner.bytes().enumerate() {
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    let name = inner.get(..split.unwrap_or(offset))?;
                    let fallback = match split {
                        Some(comma) => Some(inner.get(comma + 1..offset)?.trim()),
                        None => None,
                    };
                    return Some((name.trim(), fallback, offset + 2));
                }
            }
            b',' if depth == 1 && split.is_none() => split = Some(offset),
            _ => {}
        }
    }
    None
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

    fn theme(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect()
    }

    fn sandbox() -> DiagramSandbox {
        DiagramSandbox::new(super::super::diagram::PELE_BUNDLE_JS).expect("real bundle loads")
    }

    #[test]
    fn sandbox_renders_flowchart_geometry() {
        let rendered = sandbox()
            .render("flowchart TD\n  A[Mermaid] --> B[Pele]", Duration::from_secs(10))
            .expect("valid source renders");
        assert!(rendered.svg.contains("<svg"), "svg element present");
        assert!(rendered.width > 0.0 && rendered.height > 0.0, "nonzero size");
        assert!(!rendered.svg.contains("<a "), "links:false drops anchors");
    }

    #[test]
    fn sandbox_reports_pele_errors_as_js() {
        let error = sandbox()
            .render("flowchart TD\n  A[unclosed", Duration::from_secs(10))
            .expect_err("invalid source fails");
        assert!(matches!(error, InlineRenderError::Js(_)), "typed as JS: {error:?}");
        assert!(error.to_string().contains("Parse error"), "message preserved: {error}");
    }

    #[test]
    fn sandbox_rejects_oversized_source_at_the_utf16_boundary() {
        let sandbox = sandbox();
        let too_large = "x".repeat(super::super::diagram::MAX_DIAGRAM_SOURCE_CHARS + 1);
        assert_eq!(sandbox.render(&too_large, Duration::from_secs(10)), Err(InlineRenderError::TooLarge));
        // Exactly at the limit the JS engine runs: garbage fails fast in the
        // parser instead of tripping the size gate.
        let at_limit = "x".repeat(super::super::diagram::MAX_DIAGRAM_SOURCE_CHARS);
        assert!(matches!(sandbox.render(&at_limit, Duration::from_secs(10)), Err(InlineRenderError::Js(_))));
    }

    /// 1200-node chain ≈ 40KB: under the source gate, slow enough (tens
    /// of milliseconds even in release) that a 1ms budget always trips, and
    /// wide enough (~180k units) to trip the raster gate.
    fn big_chain() -> String {
        let mut source = String::from("flowchart LR\n");
        for index in 0..1200 {
            use std::fmt::Write;
            writeln!(source, "  N{index}[node {index}] --> N{}[node {}]", index + 1, index + 1)
                .expect("fixture builds");
        }
        source
    }

    #[test]
    fn sandbox_enforces_the_time_budget() {
        let source = big_chain();
        let sandbox = sandbox();
        // Control first: the same source renders under a generous budget.
        let _control = sandbox.render(&source, Duration::from_secs(30)).expect("control renders");
        assert!(matches!(sandbox.render(&source, Duration::from_millis(1)), Err(InlineRenderError::Js(_))));
    }

    #[test]
    fn sandbox_rejects_a_bundle_without_pele() {
        let error = match DiagramSandbox::new("var notPele = 1;") {
            Ok(_) => panic!("bundle without pele must fail"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("pele.render"), "names the contract: {error}");
    }

    #[test]
    fn nested_fallback_resolves_inside_out() {
        let svg = r##"<rect fill="var(--a, var(--b, #fff))"/>"##;
        // Inner defined: inner wins over the outer fallback literal.
        let (out, unresolved) = inline_css_vars(svg, &theme(&[("--b", "#111")]));
        assert_eq!(out, r##"<rect fill="#111"/>"##, "{out}");
        assert_eq!(unresolved, 0);
        // Neither defined: innermost literal survives.
        let (out, unresolved) = inline_css_vars(svg, &theme(&[]));
        assert_eq!(out, r##"<rect fill="#fff"/>"##, "{out}");
        assert_eq!(unresolved, 0);
        // Outer defined: fallbacks never consulted.
        let (out, _) = inline_css_vars(svg, &theme(&[("--a", "#222"), ("--b", "#111")]));
        assert_eq!(out, r##"<rect fill="#222"/>"##, "{out}");
    }

    #[test]
    fn text_node_definition_does_not_poison_map() {
        let svg = concat!(
            r##"<svg style="--_fg: blue;">"##,
            r##"<text>--_fg: red; var(--_fg)</text>"##,
            r##"<rect fill="var(--_fg)"/>"##,
            r##"</svg>"##,
        );
        let (out, unresolved) = inline_css_vars(svg, &theme(&[]));
        assert!(out.contains(r##"<text>--_fg: red; var(--_fg)</text>"##), "text byte-identical: {out}");
        assert!(out.contains(r##"<rect fill="blue"/>"##), "tag uses real def: {out}");
        assert!(!out.contains("red\""), "poison never substituted: {out}");
        assert_eq!(unresolved, 0);
    }

    #[test]
    fn quoted_gt_keeps_tag_mode() {
        let svg = r##"<rect title="a>b" fill="var(--c, #123)"/>"##;
        let (out, unresolved) = inline_css_vars(svg, &theme(&[]));
        assert_eq!(out, r##"<rect title="a>b" fill="#123"/>"##, "{out}");
        assert_eq!(unresolved, 0);
        // Single quotes behave the same.
        let svg = r##"<rect title='a>b' fill="var(--c, #123)"/>"##;
        let (out, _) = inline_css_vars(svg, &theme(&[]));
        assert!(out.contains("fill=\"#123\""), "{out}");
    }

    #[test]
    fn unbalanced_paren_does_not_panic_and_keeps_literal() {
        // Debug overflow checks are on for tests: an underflow would panic.
        let svg = r##"<svg style="--x: a)b;"><rect fill="var(--x, #000)"/></svg>"##;
        let (out, unresolved) = inline_css_vars(svg, &theme(&[]));
        assert!(out.contains("fill=\"a)b\""), "{out}");
        assert_eq!(unresolved, 0);
    }

    #[test]
    fn pele_alias_chain_resolves_to_theme() {
        let svg = concat!(
            r##"<svg style="--_fg:var(--pele-fg,#1f1f1f);" fill="var(--_fg)">"##,
            r##"<rect fill="var(--_fg)"/>"##,
            r##"</svg>"##,
        );
        let (out, unresolved) = inline_css_vars(svg, &theme(&[("--pele-fg", "#e8e8e8")]));
        assert!(!out.contains("var("), "fully resolved: {out}");
        assert!(out.contains("fill=\"#e8e8e8\""), "{out}");
        assert_eq!(unresolved, 0);
    }

    #[test]
    fn unknown_var_without_fallback_counts_unresolved_and_stays_literal() {
        let svg = r##"<rect fill="var(--nope)" stroke="var(--flat, none)"/>"##;
        let (out, unresolved) = inline_css_vars(svg, &theme(&[]));
        assert_eq!(unresolved, 1, "{out}");
        assert!(out.contains("var(--nope)"), "kept literally: {out}");
        assert!(out.contains("stroke=\"none\""), "sibling still resolves: {out}");
    }

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
