//! Canonical terminal capability probe.
//!
//! VT Code paints box-drawing chrome in more than one surface (the CLI
//! transcript, the TUI panels, the modal lists) and each surface used to
//! answer "can this terminal draw Unicode?" on its own. Those answers
//! disagreed, so the same machine could render Unicode chrome in one surface
//! and ASCII chrome in another.
//!
//! This module owns the single answer: one probe, one verdict, evaluated once
//! per process. The verdict is a deterministic fallback ladder:
//!
//! 1. `VTCODE_NO_UNICODE` set to an affirmative value drops straight to ASCII.
//! 2. A terminal that advertises Unicode (`TERM`), or a UTF-8 locale, earns
//!    Unicode box drawing.
//! 3. A terminal known to render the dashed box-drawing variants correctly
//!    earns the rich tier.
//! 4. Everything else stays on ASCII, which never renders as mojibake.
//!
//! The probe never guesses from the terminal *name* alone: iTerm2, WezTerm,
//! and Hyper all advertise `TERM=xterm-256color`, so `TERM_PROGRAM` is the
//! signal that identifies them.

use std::sync::LazyLock;

use crate::utils::env_flag_value_enabled;

/// Richness of the box-drawing chrome the terminal can render.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum BoxGlyphTier {
    /// ASCII-only chrome (`+`, `-`, `|`).
    #[default]
    Ascii,
    /// Unicode box drawing (`╭ ─ ╮ │`).
    Unicode,
    /// Unicode plus the dashed variants only some terminals render correctly.
    RichUnicode,
}

impl BoxGlyphTier {
    /// Whether this tier can draw Unicode box-drawing glyphs.
    #[must_use]
    pub fn supports_unicode(self) -> bool {
        !matches!(self, Self::Ascii)
    }
}

/// Terminal environment inputs the capability probe reads.
///
/// The verdict is a pure function of this snapshot, so callers that need to
/// evaluate a hypothetical environment can do so without mutating the
/// process environment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TerminalEnv {
    /// `TERM` — the terminal's generic capability string.
    pub term: Option<String>,
    /// `TERM_PROGRAM` — the terminal program's own name.
    pub term_program: Option<String>,
    /// `LANG` — the primary locale.
    pub lang: Option<String>,
    /// `LC_ALL` — the locale override.
    pub lc_all: Option<String>,
    /// `LC_CTYPE` — the character-encoding locale.
    pub lc_ctype: Option<String>,
    /// `VTCODE_NO_UNICODE` — opt out of Unicode chrome.
    pub no_unicode: Option<String>,
    /// `VTCODE_NO_RICH_UNICODE` — opt out of the dashed box-drawing variants.
    pub no_rich_unicode: Option<String>,
}

impl TerminalEnv {
    /// Snapshot the current process environment.
    #[must_use]
    pub fn capture() -> Self {
        Self {
            term: std::env::var("TERM").ok(),
            term_program: std::env::var("TERM_PROGRAM").ok(),
            lang: std::env::var("LANG").ok(),
            lc_all: std::env::var("LC_ALL").ok(),
            lc_ctype: std::env::var("LC_CTYPE").ok(),
            no_unicode: std::env::var("VTCODE_NO_UNICODE").ok(),
            no_rich_unicode: std::env::var("VTCODE_NO_RICH_UNICODE").ok(),
        }
    }

    /// Whether the user asked for ASCII-only chrome.
    ///
    /// Uses the repository's canonical env-flag semantics: only an affirmative
    /// value (`1`, `true`, `yes`, `on`, `debug`) disables Unicode, so
    /// `VTCODE_NO_UNICODE=0` leaves Unicode chrome enabled.
    #[must_use]
    pub fn no_unicode_requested(&self) -> bool {
        env_flag_value_enabled(self.no_unicode.as_deref())
    }

    /// Whether the user asked to skip the dashed box-drawing variants.
    #[must_use]
    pub fn no_rich_unicode_requested(&self) -> bool {
        env_flag_value_enabled(self.no_rich_unicode.as_deref())
    }

    /// Whether the terminal can draw Unicode box-drawing glyphs.
    ///
    /// `TERM` is authoritative because it describes the terminal program; the
    /// locale only decides the question when `TERM` says nothing. A UTF-8
    /// locale is the only positive evidence left, so without one the answer
    /// stays at "no" and the terminal keeps ASCII chrome.
    #[must_use]
    pub fn supports_unicode_box_drawing(&self) -> bool {
        if let Some(term) = self.term.as_deref() {
            if term_supports_unicode(term) {
                return true;
            }
            if term_lacks_unicode(term) {
                return false;
            }
        }

        [self.lang.as_deref(), self.lc_all.as_deref(), self.lc_ctype.as_deref()]
            .iter()
            .flatten()
            .any(|locale| locale_is_utf8(locale))
    }

    /// Whether the terminal renders the dashed box-drawing variants correctly.
    ///
    /// `TERM_PROGRAM` identifies the terminal program; `TERM` only names it
    /// for the terminals that put their name in `TERM` as well.
    #[must_use]
    pub fn supports_rich_box_drawing(&self) -> bool {
        const RICH_TERMINALS: [&str; 6] = ["ghostty", "wezterm", "iterm", "hyper", "kitty", "alacritty"];

        let term_program = self.term_program.as_deref().unwrap_or_default();
        let term = self.term.as_deref().unwrap_or_default();
        RICH_TERMINALS.iter().any(|name| {
            crate::formatting::contains_ignore_ascii_case(term_program, name)
                || crate::formatting::contains_ignore_ascii_case(term, name)
        })
    }
}

/// Whether the environment advertises Unicode box-drawing support.
#[must_use]
pub fn box_glyph_tier() -> BoxGlyphTier {
    *BOX_GLYPH_TIER
}

/// Evaluate the capability ladder against a captured environment.
#[must_use]
pub fn box_glyph_tier_for(env: &TerminalEnv) -> BoxGlyphTier {
    if env.no_unicode_requested() {
        return BoxGlyphTier::Ascii;
    }
    if !env.supports_unicode_box_drawing() {
        return BoxGlyphTier::Ascii;
    }
    if env.no_rich_unicode_requested() {
        return BoxGlyphTier::Unicode;
    }
    if env.supports_rich_box_drawing() {
        return BoxGlyphTier::RichUnicode;
    }
    BoxGlyphTier::Unicode
}

/// The cached verdict; the environment does not change while VT Code runs.
static BOX_GLYPH_TIER: LazyLock<BoxGlyphTier> = LazyLock::new(|| box_glyph_tier_for(&TerminalEnv::capture()));

/// Terminals whose `TERM` value advertises Unicode box-drawing support.
fn term_supports_unicode(term: &str) -> bool {
    let term = term.to_ascii_lowercase();
    term.contains("utf")
        || term.contains("unicode")
        || term.contains("256color")
        || term.contains("truecolor")
        || term.contains("direct")
        || term.contains("alacritty")
        || term.contains("wezterm")
        || term.contains("kitty")
        || term.contains("iterm")
        || term.contains("hyper")
        || term.contains("ghostty")
        || term.contains("konsole")
        || term.contains("contour")
        || term.contains("foot")
}

/// Terminals that cannot render Unicode box-drawing glyphs at all.
fn term_lacks_unicode(term: &str) -> bool {
    let term = term.to_ascii_lowercase();
    term == "dumb" || term == "emacs" || term == "ansi" || term == "cons25" || term == "basic" || term.starts_with("vt")
}

/// Whether a locale value declares a UTF-8 encoding.
fn locale_is_utf8(locale: &str) -> bool {
    locale.to_ascii_lowercase().contains("utf")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(term: Option<&str>, term_program: Option<&str>, lang: Option<&str>) -> TerminalEnv {
        TerminalEnv {
            term: term.map(str::to_owned),
            term_program: term_program.map(str::to_owned),
            lang: lang.map(str::to_owned),
            ..TerminalEnv::default()
        }
    }

    #[test]
    fn ascii_is_the_fallback_without_evidence() {
        assert_eq!(box_glyph_tier_for(&TerminalEnv::default()), BoxGlyphTier::Ascii);
        assert_eq!(box_glyph_tier_for(&env(None, None, None)), BoxGlyphTier::Ascii);
    }

    #[test]
    fn term_advertisement_earns_unicode_even_with_a_c_locale() {
        let tier = box_glyph_tier_for(&env(Some("xterm-256color"), None, Some("C")));
        assert_eq!(tier, BoxGlyphTier::Unicode);
    }

    #[test]
    fn utf8_locale_earns_unicode_when_term_says_nothing() {
        assert_eq!(box_glyph_tier_for(&env(None, None, Some("en_US.UTF-8"))), BoxGlyphTier::Unicode);
        assert_eq!(
            box_glyph_tier_for(&TerminalEnv {
                lc_ctype: Some("en_US.UTF-8".to_owned()),
                ..TerminalEnv::default()
            }),
            BoxGlyphTier::Unicode
        );
    }

    #[test]
    fn explicitly_non_utf8_locale_stays_ascii() {
        assert_eq!(box_glyph_tier_for(&env(None, None, Some("C"))), BoxGlyphTier::Ascii);
    }

    #[test]
    fn dumb_terminal_stays_ascii_even_with_a_utf8_locale() {
        assert_eq!(box_glyph_tier_for(&env(Some("dumb"), None, Some("en_US.UTF-8"))), BoxGlyphTier::Ascii);
    }

    #[test]
    fn term_program_identifies_rich_terminals_that_share_a_generic_term() {
        for (program, term) in [
            ("iTerm.app", "xterm-256color"),
            ("WezTerm", "xterm-256color"),
            ("Hyper.app", "xterm-256color"),
            ("kitty", "xterm-kitty"),
            ("ghostty", "xterm-ghostty"),
            ("Alacritty", "alacritty"),
        ] {
            let tier = box_glyph_tier_for(&env(Some(term), Some(program), Some("en_US.UTF-8")));
            assert_eq!(tier, BoxGlyphTier::RichUnicode, "{program} should reach the rich tier");
        }
    }

    #[test]
    fn plain_terminals_stay_on_the_unicode_tier() {
        assert_eq!(box_glyph_tier_for(&env(Some("xterm-256color"), None, Some("en_US.UTF-8"))), BoxGlyphTier::Unicode);
        assert_eq!(
            box_glyph_tier_for(&env(Some("xterm-256color"), Some("Apple_Terminal"), Some("en_US.UTF-8"))),
            BoxGlyphTier::Unicode
        );
    }

    #[test]
    fn opt_out_flags_use_canonical_flag_semantics() {
        let base = env(Some("xterm-256color"), Some("WezTerm"), Some("en_US.UTF-8"));
        assert_eq!(box_glyph_tier_for(&base), BoxGlyphTier::RichUnicode);

        let no_unicode = TerminalEnv { no_unicode: Some("1".to_owned()), ..base.clone() };
        assert_eq!(box_glyph_tier_for(&no_unicode), BoxGlyphTier::Ascii);

        let no_rich = TerminalEnv {
            no_rich_unicode: Some("1".to_owned()),
            ..base.clone()
        };
        assert_eq!(box_glyph_tier_for(&no_rich), BoxGlyphTier::Unicode);
    }

    #[test]
    fn falsy_opt_out_values_do_not_disable_unicode() {
        for value in ["0", "false", "off", "no", ""] {
            let disabled = TerminalEnv {
                no_unicode: Some(value.to_owned()),
                no_rich_unicode: Some(value.to_owned()),
                ..env(Some("xterm-256color"), Some("WezTerm"), Some("en_US.UTF-8"))
            };
            assert_eq!(
                box_glyph_tier_for(&disabled),
                BoxGlyphTier::RichUnicode,
                "VTCODE_NO_UNICODE={value:?} must not disable Unicode"
            );
        }
    }

    #[test]
    fn tier_supports_unicode_matches_the_ladder() {
        assert!(!BoxGlyphTier::Ascii.supports_unicode());
        assert!(BoxGlyphTier::Unicode.supports_unicode());
        assert!(BoxGlyphTier::RichUnicode.supports_unicode());
    }
}
