//! Mermaid diagram previews rendered by the vendored Pele bundle.
//!
//! The desktop has no JS runtime or webview, so a `mermaid` fence gets a
//! "View diagram" action that opens a self-contained preview page (Pele
//! inlined, no network) in the system browser. This module holds the pure
//! pieces: fence detection, theme-to-CSS mapping, and the hardened page
//! builder. Diagram source is untrusted agent output: it only ever enters
//! the page as an escaped JS string literal, links and images stay off,
//! and render errors display as inert text.

use gpui_kit::component::theme::ThemeColor;

/// Vendored Pele browser bundle (global `pele` with `mount`). Version and
/// checksum live in `resources/pele/PROVENANCE.md`.
pub(super) const PELE_BUNDLE_JS: &str = include_str!("../../../../../resources/pele/pele.global.js");

/// Largest diagram source accepted for preview, in UTF-16 code units.
/// Matches Pele's own default `limit` exactly (it enforces
/// `source.length`, which counts UTF-16 units, not scalar values);
/// larger sources fail closed instead of writing an unbounded preview
/// file.
pub(super) const MAX_DIAGRAM_SOURCE_CHARS: usize = 50_000;

/// Content Security Policy for the preview page: the page is fully
/// self-contained (inlined bundle and styles, system fonts, canvas text
/// measurement), so every network-capable directive is denied. Inline
/// scripts and styles stay allowed because the bundle and theme ship
/// inline; a renderer bug therefore cannot exfiltrate.
pub(super) const PREVIEW_CSP: &str = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src 'none'; connect-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'";

/// Rejection from [`build_preview_html`] when the source exceeds
/// [`MAX_DIAGRAM_SOURCE_CHARS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DiagramTooLarge;

/// Whether a fenced code block selects Mermaid diagrams. Matches Kit's
/// `CodeBlock::lang()` token case-insensitively; `None` (unlanguaged
/// fences) and near-misses (`mmd`, `mermaidjs`) never match.
pub(super) fn is_mermaid_fence(language: Option<&str>) -> bool {
    language.is_some_and(|lang| lang.trim().eq_ignore_ascii_case("mermaid"))
}

/// Map desktop theme tokens onto Pele's `--pele-*` CSS variables, so the
/// browser preview follows the app theme in both light and dark modes.
/// Muted text reuses the AA-adjusted transcript token; the series palette
/// reuses the eight most distinct semantic tokens (Pele's own defaults
/// assume a light background, so they cannot stand in dark mode). Vars Pele
/// already defaults correctly (`--pele-fg-on-light`, fonts, weights) are
/// left unset.
pub(super) fn pele_css_vars(colors: &ThemeColor) -> String {
    let mut out = String::new();
    for (name, color) in pele_var_colors(colors) {
        out.push_str("  ");
        out.push_str(name);
        out.push_str(": ");
        out.push_str(&css_hex(color));
        out.push_str(";\n");
    }
    out
}

/// The `--pele-*` variable names with their theme colors, shared by the
/// browser preview stylesheet and the inline renderer's substitution map so
/// both paths theme identically.
pub(super) fn pele_var_colors(colors: &ThemeColor) -> [(&'static str, gpui_kit::Hsla); 18] {
    [
        ("--pele-bg", colors.background),
        ("--pele-fg", colors.foreground),
        ("--pele-muted", super::themes::muted_text(colors)),
        ("--pele-line", colors.foreground),
        ("--pele-surface", colors.muted),
        ("--pele-surface-alt", colors.popover),
        ("--pele-border", colors.border),
        ("--pele-accent", colors.accent),
        ("--pele-critical", colors.danger),
        ("--pele-error", colors.danger),
        ("--pele-series-1", colors.link),
        ("--pele-series-2", colors.warning),
        ("--pele-series-3", colors.success),
        ("--pele-series-4", colors.danger),
        ("--pele-series-5", colors.info),
        ("--pele-series-6", colors.accent),
        ("--pele-series-7", colors.secondary),
        ("--pele-series-8", colors.primary),
    ]
}

/// Format a theme color as `#rrggbb` for Pele stylesheets and substitution.
pub(super) fn css_hex(color: gpui_kit::Hsla) -> String {
    let rgb = color.to_rgb();
    format!("#{:02x}{:02x}{:02x}", channel(rgb.r), channel(rgb.g), channel(rgb.b))
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 0..=255"
)]
fn channel(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Encode untrusted text as a JS string literal. `serde_json` quotes and
/// escapes per JSON; `<` is additionally escaped so `</script>` and `<!--`
/// inside diagram source cannot break out of the script block. `\u003c`
/// decodes back to `<`, so the encoding still JSON-round-trips.
pub(super) fn js_string_literal(source: &str) -> String {
    serde_json::to_string(source)
        .unwrap_or_else(|_| "\"\"".to_string())
        .replace('<', "\\u003c")
}

/// Escape untrusted text for an HTML text or double-quoted attribute slot
/// (the preview `<title>` and `aria-label`).
pub(super) fn html_escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            _ => out.push(c),
        }
    }
    out
}

/// Build the self-contained preview page: theme vars, the Pele bundle, and
/// the diagram source as an escaped literal. Rendering runs in the browser
/// via `pele.mount` with links/images disabled, zoom on shrink-to-fit, and
/// errors shown as inert text. The bundle is a parameter (callers pass
/// [`PELE_BUNDLE_JS`]) so tests stay hermetic.
pub(super) fn build_preview_html(
    source: &str,
    title: &str,
    lang: &str,
    vars_css: &str,
    bundle_js: &str,
) -> Result<String, DiagramTooLarge> {
    if source.encode_utf16().count() > MAX_DIAGRAM_SOURCE_CHARS {
        return Err(DiagramTooLarge);
    }
    let source_js = js_string_literal(source);
    let title_html = html_escape_text(title);
    let lang_html = html_escape_text(lang);
    Ok(format!(
        "<!DOCTYPE html>\n\
         <html lang=\"{lang_html}\">\n\
         <head>\n\
         <meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <meta http-equiv=\"Content-Security-Policy\" content=\"{csp}\">\n\
         <title>{title_html}</title>\n\
         <style>\n\
         :root {{\n\
         {vars_css}}}\n\
         html, body {{\n\
         \x20 margin: 0;\n\
         \x20 padding: 24px;\n\
         \x20 background: var(--pele-bg);\n\
         \x20 color: var(--pele-fg);\n\
         \x20 font-family: system-ui, -apple-system, sans-serif;\n\
         }}\n\
         #diagram {{\n\
         \x20 max-width: 960px;\n\
         \x20 margin: 0 auto;\n\
         }}\n\
         #error {{\n\
         \x20 display: none;\n\
         \x20 max-width: 960px;\n\
         \x20 margin: 16px auto;\n\
         \x20 padding: 12px 16px;\n\
         \x20 border: 1px solid var(--pele-error);\n\
         \x20 border-radius: 8px;\n\
         \x20 color: var(--pele-error);\n\
         \x20 white-space: pre-wrap;\n\
         \x20 font-family: ui-monospace, monospace;\n\
         }}\n\
         </style>\n\
         </head>\n\
         <body>\n\
         <div id=\"diagram\" role=\"img\" aria-label=\"{title_html}\"></div>\n\
         <pre id=\"error\"></pre>\n\
         <script>\n\
         {bundle_js}\n\
         </script>\n\
         <script>\n\
         (function () {{\n\
         \x20 var source = {source_js};\n\
         \x20 function showError(err) {{\n\
         \x20   var box = document.getElementById(\"error\");\n\
         \x20   box.style.display = \"block\";\n\
         \x20   box.textContent = err && err.message ? err.message : String(err);\n\
         \x20 }}\n\
         \x20 try {{\n\
         \x20   var diagram = pele.mount(document.getElementById(\"diagram\"), source, {{\n\
         \x20     links: false,\n\
         \x20     images: false,\n\
         \x20     zoom: \"auto\",\n\
         \x20     idPrefix: \"vtcode-diagram\",\n\
         \x20     onError: showError\n\
         \x20   }});\n\
         \x20   if (diagram.error) {{\n\
         \x20     showError(diagram.error);\n\
         \x20   }}\n\
         \x20 }} catch (err) {{\n\
         \x20   showError(err);\n\
         \x20 }}\n\
         }})();\n\
         </script>\n\
         </body>\n\
         </html>\n",
        csp = PREVIEW_CSP,
    ))
}

/// Open a Mermaid fence in the system browser as a themed Pele preview.
/// Page build plus bounded stale sweep plus owner-only write plus one
/// `posix_spawn` measure ~6ms with no waits, so this stays on the calling
/// thread like the adjacent copy action's clipboard IPC; the browser
/// launch itself is fire-and-forget with a reaper thread. Success needs
/// no toast (the browser opening is the feedback); every failure the app
/// can observe toasts (the fire-and-forget exit status is unobserved by
/// design).
pub(super) fn open_diagram_preview(source: &str, window: &mut gpui_kit::Window, cx: &mut gpui_kit::App) {
    use gpui_kit::component::theme::ActiveTheme as _;

    let vars = pele_css_vars(&cx.theme().colors);
    let title = rust_i18n::t!("app.markdown.diagram_preview_title").to_string();
    let page = match build_preview_html(source, &title, &rust_i18n::locale(), &vars, PELE_BUNDLE_JS) {
        Ok(page) => page,
        Err(DiagramTooLarge) => {
            let message = rust_i18n::t!("app.markdown.diagram_too_large").to_string();
            super::notifications::diagram_preview_failed(window, message, cx);
            return;
        }
    };
    let _swept = super::files::sweep_stale_previews();
    let path = match super::files::write_preview_html(&page) {
        Ok(path) => path,
        Err(message) => {
            super::notifications::diagram_preview_failed(window, message, cx);
            return;
        }
    };
    if let Err(message) = super::files::open_in_browser(&path) {
        super::notifications::diagram_preview_failed(window, message, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mermaid_fence_matches_only_mermaid() {
        for lang in ["mermaid", "Mermaid", "MERMAID", "  mermaid  "] {
            assert!(is_mermaid_fence(Some(lang)), "{lang:?} must match");
        }
        for lang in [
            None,
            Some(""),
            Some("  "),
            Some("mmd"),
            Some("mermaidjs"),
            Some("xmermaid"),
            Some("rust"),
            Some("```"),
        ] {
            assert!(!is_mermaid_fence(lang), "{lang:?} must not match");
        }
    }

    #[test]
    fn js_literal_escapes_script_breakout_and_round_trips() {
        let hostile = [
            "</script><script>alert(1)</script>",
            "<!-- conditional comment",
            "quote \" backslash \\ newline \n tab \t",
            "unicode café 🎨 <tag>",
            "\u{2028}\u{2029} line separators",
        ];
        for source in hostile {
            let literal = js_string_literal(source);
            assert!(literal.starts_with('"') && literal.ends_with('"'), "quoted: {literal:?}");
            assert!(!literal.contains("</script"), "no breakout: {literal:?}");
            assert!(!literal.contains("<!--"), "no comment open: {literal:?}");
            assert!(!literal.contains('<'), "no raw <: {literal:?}");
            // Independent oracle: a JSON parse inverts the encoding exactly.
            let round_tripped: String = serde_json::from_str(&literal).expect("valid JSON string");
            assert_eq!(round_tripped, source);
        }
    }

    #[test]
    fn html_escape_covers_markup_chars_without_double_escaping() {
        assert_eq!(html_escape_text("a&b<c>d\"e'f"), "a&amp;b&lt;c&gt;d&quot;e&#x27;f");
        // `&` first: an existing entity reference escapes its ampersand once.
        assert_eq!(html_escape_text("&lt;"), "&amp;lt;");
        assert_eq!(html_escape_text("plain café"), "plain café");
    }

    #[test]
    fn preview_html_rejects_oversized_source_at_the_boundary() {
        let ok = "x".repeat(MAX_DIAGRAM_SOURCE_CHARS);
        assert!(build_preview_html(&ok, "t", "en", "", "/*bundle*/").is_ok(), "exactly at the limit");
        let too_large = "x".repeat(MAX_DIAGRAM_SOURCE_CHARS + 1);
        assert_eq!(build_preview_html(&too_large, "t", "en", "", "/*bundle*/"), Err(DiagramTooLarge));
    }

    #[test]
    fn diagram_limit_matches_pele_default() {
        // Coupled to Pele's documented default `limit` option.
        assert_eq!(MAX_DIAGRAM_SOURCE_CHARS, 50_000);
    }

    #[test]
    fn preview_html_inlines_bundle_vars_source_and_title() {
        let source = "flowchart TD\n  A[\"</script>\"] --> B";
        let page = build_preview_html(source, "Title \"quoted\"", "en", "  --pele-bg: #fff;\n", "/*PELE-BUNDLE*/")
            .expect("small source builds");
        assert!(page.contains("/*PELE-BUNDLE*/"), "bundle inlined");
        assert!(page.contains("--pele-bg: #fff;"), "vars inlined");
        assert!(page.contains("links: false"), "links disabled");
        assert!(page.contains("images: false"), "images disabled");
        assert!(page.contains("textContent"), "errors as inert text");
        assert!(page.contains("pele.mount"), "renders via mount");
        assert!(page.contains("<title>Title &quot;quoted&quot;</title>"), "title escaped");
        assert!(page.contains("aria-label=\"Title &quot;quoted&quot;\""), "aria label escaped");
        assert!(!page.contains("</script>\"]"), "source cannot break out");
        assert!(page.contains("\\u003c/script>"), "source < escaped");
    }

    #[test]
    fn preview_html_template_has_no_external_references() {
        // Hermetic bundle: the template itself must never fetch. (The real
        // bundle's banner comment mentions its repo URL; that is inert text,
        // not a reference the page loads.)
        let page = build_preview_html("flowchart TD\n  A --> B", "t", "en", "", "/*bundle*/").expect("builds");
        for forbidden in ["http://", "https://", "src=", "href="] {
            assert!(!page.contains(forbidden), "template must not contain {forbidden:?}");
        }
        // The CSP pins the hermetic property at runtime: independently
        // written literal, so template drift fails loudly.
        assert!(
            page.contains(
                "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src 'none'; connect-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'\">"
            ),
            "CSP meta present"
        );
    }

    #[test]
    fn preview_html_counts_utf16_units_like_pele() {
        // Pele enforces `source.length` (UTF-16 units); astral characters
        // count double. Boundary pair around the limit.
        let at_limit = format!("{}🎨", "x".repeat(MAX_DIAGRAM_SOURCE_CHARS - 2));
        assert!(build_preview_html(&at_limit, "t", "en", "", "/*bundle*/").is_ok(), "exactly 50_000 units");
        let over_limit = format!("{}🎨", "x".repeat(MAX_DIAGRAM_SOURCE_CHARS - 1));
        assert_eq!(build_preview_html(&over_limit, "t", "en", "", "/*bundle*/"), Err(DiagramTooLarge));
    }

    #[test]
    fn real_bundle_defines_the_global_mount_the_template_calls() {
        assert!(PELE_BUNDLE_JS.contains("var pele="), "global present");
        assert!(PELE_BUNDLE_JS.contains("mount:"), "mount exported");
    }

    #[test]
    fn real_bundle_has_no_script_breakout_sequences() {
        // The template inlines the bundle into a <script> block, so a future
        // bundle containing these sequences would break page parsing.
        assert!(!PELE_BUNDLE_JS.to_lowercase().contains("</script"), "no script close");
        assert!(!PELE_BUNDLE_JS.contains("<!--"), "no comment open");
    }

    fn is_css_hex(value: &str) -> bool {
        value
            .strip_prefix('#')
            .is_some_and(|hex| hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()))
    }

    #[test]
    fn css_vars_emit_hex_for_every_pele_token() {
        let mut colors = ThemeColor::default();
        colors.background = gpui_kit::Hsla { h: 0.0, s: 0.0, l: 1.0, a: 1.0 };
        colors.foreground = gpui_kit::Hsla { h: 0.0, s: 0.0, l: 0.0, a: 1.0 };
        let vars = pele_css_vars(&colors);
        let expected = [
            "--pele-bg",
            "--pele-fg",
            "--pele-muted",
            "--pele-line",
            "--pele-surface",
            "--pele-surface-alt",
            "--pele-border",
            "--pele-accent",
            "--pele-critical",
            "--pele-error",
            "--pele-series-1",
            "--pele-series-2",
            "--pele-series-3",
            "--pele-series-4",
            "--pele-series-5",
            "--pele-series-6",
            "--pele-series-7",
            "--pele-series-8",
        ];
        assert_eq!(vars.lines().count(), expected.len(), "one var per line:\n{vars}");
        for name in expected {
            let line = vars
                .lines()
                .find(|line| line.trim_start().starts_with(name))
                .unwrap_or_else(|| panic!("{name} emitted"));
            let value = line.split(':').nth(1).expect("name: value").trim().trim_end_matches(';');
            assert!(is_css_hex(value), "{name} is #rrggbb, got {value:?}");
        }
        // Independent oracles: pure white/black fixtures convert exactly.
        assert!(vars.contains("--pele-bg: #ffffff;"), "white bg:\n{vars}");
        assert!(vars.contains("--pele-fg: #000000;"), "black fg:\n{vars}");
    }

    #[test]
    fn css_vars_follow_light_and_dark_fixtures() {
        let mut light = ThemeColor::default();
        light.background = gpui_kit::Hsla { h: 0.0, s: 0.0, l: 1.0, a: 1.0 };
        let mut dark = ThemeColor::default();
        dark.background = gpui_kit::Hsla { h: 0.0, s: 0.0, l: 0.0, a: 1.0 };
        let light_vars = pele_css_vars(&light);
        let dark_vars = pele_css_vars(&dark);
        assert!(light_vars.contains("--pele-bg: #ffffff;"), "light:\n{light_vars}");
        assert!(dark_vars.contains("--pele-bg: #000000;"), "dark:\n{dark_vars}");
        assert_ne!(light_vars, dark_vars, "theme change moves the vars");
    }
}
