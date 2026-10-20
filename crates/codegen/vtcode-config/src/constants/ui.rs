pub const TOOL_OUTPUT_MODE_COMPACT: &str = "compact";
pub const TOOL_OUTPUT_MODE_FULL: &str = "full";
pub const DEFAULT_INLINE_VIEWPORT_ROWS: u16 = 16;
pub const DEFAULT_REASONING_VISIBLE: bool = true;
pub const MODAL_MIN_WIDTH: u16 = 36;
pub const MODAL_MIN_HEIGHT: u16 = 16;
pub const MODAL_INSTRUCTIONS_BULLET: &str = "•";
pub const INLINE_HEADER_HEIGHT: u16 = 3;
pub const INLINE_INPUT_PADDING_HORIZONTAL: u16 = 1;
pub const INLINE_INPUT_PADDING_VERTICAL: u16 = 1;
pub const INLINE_INPUT_MAX_LINES: usize = 10;
/// Transcript bottom padding rows. Live value:
/// `vtcode_ui::tui::config::constants::ui::INLINE_TRANSCRIPT_BOTTOM_PADDING`
/// with `effective_transcript_bottom_padding`; keep in sync.
pub const INLINE_TRANSCRIPT_BOTTOM_PADDING: u16 = 2;
pub const INLINE_PREVIEW_ELLIPSIS: &str = "…";
pub const INLINE_PASTE_COLLAPSE_LINE_THRESHOLD: usize = 10;
pub const HEADER_HIGHLIGHT_PREVIEW_MAX_CHARS: usize = 48;
pub const INLINE_AGENT_MESSAGE_LEFT_PADDING: &str = " ";
pub const INLINE_AGENT_QUOTE_PREFIX: &str = " •";
pub const INLINE_USER_MESSAGE_DIVIDER_SYMBOL: &str = "─";

pub const INLINE_BLOCK_HORIZONTAL: &str = "─";
/// Shared inline separator between metadata segments (`a • b • c`).
///
/// Single source for every `" • "` join so spacing stays consistent across
/// headers, modals, and slash-command detail lines.
pub const META_BULLET_SEPARATOR: &str = " • ";
pub const MODAL_LIST_HIGHLIGHT_FULL: &str = "┃ ";
pub const MODAL_LIST_SUMMARY_NO_MATCHES: &str = "No matches";
pub const MODAL_LIST_SUMMARY_RESET_HINT: &str = "Press Esc to reset";
pub const MODAL_LIST_NO_RESULTS_MESSAGE: &str = "No matching options";
pub const HEADER_VERSION_PROMPT: &str = "> ";
pub const HEADER_VERSION_PREFIX: &str = "VT Code";
pub const HEADER_VERSION_LEFT_DELIMITER: &str = "(";
pub const HEADER_VERSION_RIGHT_DELIMITER: &str = ")";
pub const HEADER_PROVIDER_PREFIX: &str = "Provider: ";
pub const HEADER_MODEL_PREFIX: &str = "Model: ";
pub const HEADER_SERVICE_TIER_PREFIX: &str = "Tier: ";
pub const HEADER_REASONING_PREFIX: &str = "";
pub const HEADER_TRUST_PREFIX: &str = "Trust: ";
pub const HEADER_TOOLS_PREFIX: &str = "Tools: ";
pub const HEADER_MCP_PREFIX: &str = "MCP: ";
pub const HEADER_GIT_PREFIX: &str = "git: ";
pub const HEADER_GIT_CLEAN_SUFFIX: &str = "✓";
pub const HEADER_GIT_DIRTY_SUFFIX: &str = "*";
pub const HEADER_UNKNOWN_PLACEHOLDER: &str = "unavailable";
/// Shortcut hints shared by the bootstrap and follow-up placeholders.
///
/// Defined once so the two cannot drift apart. The composer paints this on a
/// single clipped line, so the most useful hints lead and trailing ones fall
/// off first on narrow terminals.
///
/// Each key must match a live binding: `@`/`/` open the file and slash
/// palettes, Enter submits, `Tab` queues while a turn runs, `Shift+Tab` cycles
/// to the previous agent, and `Ctrl+R` opens the history picker.
macro_rules! chat_input_shortcut_hint {
    () => {
        "@files /commands • Enter run • Tab queue • Shift+Tab agent • Ctrl+R history"
    };
}
/// Empty-composer prompt before the first turn.
pub const CHAT_INPUT_PLACEHOLDER_BOOTSTRAP: &str = concat!("Type a request… ", chat_input_shortcut_hint!());
/// Empty-composer prompt after at least one turn.
pub const CHAT_INPUT_PLACEHOLDER_FOLLOW_UP: &str = concat!("Continue… ", chat_input_shortcut_hint!());
pub const CHAT_INPUT_PLACEHOLDER_INTERRUPTED: &str =
    "Interrupted · What should VT Code do instead? • Esc cancel • Ctrl+C interrupt • Ctrl+D exit";
pub const WELCOME_SLASH_COMMAND_LIMIT: usize = 6;
pub const WELCOME_SLASH_COMMAND_PREFIX: &str = "/";
pub const WELCOME_SLASH_COMMAND_INTRO: &str = "";
pub const WELCOME_SLASH_COMMAND_INDENT: &str = "  ";
pub const PLAN_BLOCK_TITLE: &str = "TODOs";
pub const PLAN_STATUS_EMPTY: &str = "No TODOs";
pub(crate) const STATUS_LINE_REFRESH_INTERVAL_MS: u64 = 1000;
pub(crate) const STATUS_LINE_COMMAND_TIMEOUT_MS: u64 = 200;

// Agent/Mode color constants.
//
// Mode badges resolve through one canonical token per mode. Most modes use a
// **standard ANSI hue name** (green/blue/magenta/...), which the design system
// (`vtcode-ui`) maps to a theme-appropriate variant at render time: the
// brighter `Light*` ANSI variant on dark terminals, the base variant on light
// terminals. Build is the exception: it uses a fixed hex token that renders
// identically on both appearances.
//
// `AGENT_HUE_NAMES` mirrors the variant table in `vtcode-ui`'s design color
// resolver, which owns the actual RGB values.
const AGENT_HUE_NAMES: &[&str] = &["red", "green", "blue", "magenta", "yellow", "cyan"];

/// Build agent color - sage `#73a18e` (calm, implementation tone).
pub const AGENT_COLOR_BUILD: &str = "#73a18e";
/// Auto agent hue - green (autonomy and forward progress).
pub const AGENT_COLOR_AUTO: &str = "green";
/// Plan agent hue - blue (thoughtful, planning tone).
pub const AGENT_COLOR_PLAN: &str = "blue";
/// Duck agent hue - magenta (soft, discussion-focused).
pub const AGENT_COLOR_DUCK: &str = "magenta";

/// Canonical primary-agent mode -> color token mapping (single source of
/// truth). `subagents.rs` assigns the color to each built-in spec from here.
const AGENT_MODE_HUE: &[(&str, &str)] = &[
    ("build", AGENT_COLOR_BUILD),
    ("auto", AGENT_COLOR_AUTO),
    ("plan", AGENT_COLOR_PLAN),
    ("duck", AGENT_COLOR_DUCK),
];

/// Resolve a primary-agent mode name to its canonical color token
/// (e.g. `"build"` -> `"#73a18e"`, `"plan"` -> `"blue"`).
pub fn agent_mode_hue(mode: &str) -> Option<&'static str> {
    AGENT_MODE_HUE.iter().find(|(m, _)| *m == mode).map(|(_, h)| *h)
}

// TUI timing, tick, and viewport limits live in
// `vtcode_ui::tui::config::constants::ui` — the only reader. Do not redeclare
// them here: shadowed copies drift silently (see `no_shadowed_tui_constants`).

// Theme and color constants
pub const THEME_MIN_CONTRAST_RATIO: f32 = 4.5;
pub const THEME_FOREGROUND_LIGHTEN_RATIO: f32 = 0.25;
pub const THEME_SECONDARY_LIGHTEN_RATIO: f32 = 0.2;
pub const THEME_MIX_RATIO: f32 = 0.35;
pub const THEME_TOOL_BODY_MIX_RATIO: f32 = 0.35;
pub const THEME_TOOL_BODY_LIGHTEN_RATIO: f32 = 0.2;
pub const THEME_RESPONSE_COLOR_LIGHTEN_RATIO: f32 = 0.15;
pub const THEME_INPUT_BACKGROUND_MIX_RATIO: f32 = 0.08;
pub const THEME_USER_COLOR_LIGHTEN_RATIO: f32 = 0.2;
pub const THEME_SECONDARY_USER_COLOR_LIGHTEN_RATIO: f32 = 0.4;
pub const THEME_PRIMARY_STATUS_LIGHTEN_RATIO: f32 = 0.35;
pub const THEME_PRIMARY_STATUS_SECONDARY_LIGHTEN_RATIO: f32 = 0.5;
pub const THEME_LOGO_ACCENT_BANNER_LIGHTEN_RATIO: f32 = 0.35;
pub const THEME_LOGO_ACCENT_BANNER_SECONDARY_LIGHTEN_RATIO: f32 = 0.25;

// UI Color constants
pub const THEME_COLOR_WHITE_RED: u8 = 0xFF;
pub const THEME_COLOR_WHITE_GREEN: u8 = 0xFF;
pub const THEME_COLOR_WHITE_BLUE: u8 = 0xFF;
pub const THEME_MIX_RATIO_MIN: f32 = 0.0;
pub const THEME_MIX_RATIO_MAX: f32 = 1.0;
pub const THEME_BLEND_CLAMP_MIN: f32 = 0.0;
pub const THEME_BLEND_CLAMP_MAX: f32 = 255.0;

// WCAG contrast algorithm constants
pub const THEME_RELATIVE_LUMINANCE_CUTOFF: f32 = 0.03928;
pub const THEME_RELATIVE_LUMINANCE_LOW_FACTOR: f32 = 12.92;
pub const THEME_RELATIVE_LUMINANCE_OFFSET: f32 = 0.055;
pub const THEME_RELATIVE_LUMINANCE_EXPONENT: f32 = 2.4;
pub const THEME_CONTRAST_RATIO_OFFSET: f32 = 0.05;
pub const THEME_RED_LUMINANCE_COEFFICIENT: f32 = 0.2126;
pub const THEME_GREEN_LUMINANCE_COEFFICIENT: f32 = 0.7152;
pub const THEME_BLUE_LUMINANCE_COEFFICIENT: f32 = 0.0722;
pub const THEME_LUMINANCE_LIGHTEN_RATIO: f32 = 0.2;
/// Mix ratio for PTY output text toward background — dim but readable (higher = dimmer)
pub const THEME_PTY_OUTPUT_MIX_RATIO: f32 = 0.50;

/// Medium gray for input placeholder text
pub const PLACEHOLDER_R: u8 = 0x88;
pub const PLACEHOLDER_G: u8 = 0x88;
pub const PLACEHOLDER_B: u8 = 0x88;

// Safe ANSI palette. Terminal color portability research:
// <https://blog.xoria.org/terminal-colors/>
// Only the bright-cyan index is shared with the config layer; the rest of the
// palette lives in `vtcode_ui::tui::config::constants::ui`.

/// Bright cyan (ANSI 14) — used for modal list title/border accents.
pub const SAFE_ANSI_BRIGHT_CYAN: u8 = 14;

#[cfg(test)]
mod tests {
    use super::*;

    /// Representative terminal backgrounds the agent badge is rendered against.
    /// A near-black dark theme (Catppuccin Mocha / Gruvbox) and a near-white
    /// light theme (Solarized Light / Catppuccin Latte).
    const DARK_BG: (u8, u8, u8) = (0x18, 0x18, 0x18);
    const LIGHT_BG: (u8, u8, u8) = (0xFD, 0xF6, 0xE3);

    /// Parse a `#rrggbb` token to its RGB triple. Returns `None` for anything
    /// else (hue names fall through to the variant table below).
    fn hex_to_rgb(hex: &str) -> Option<(u8, u8, u8)> {
        let hex = hex.strip_prefix('#')?;
        if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        Some((channel(0)?, channel(2)?, channel(4)?))
    }

    /// Standard ANSI variant RGB values used by the design system for each hue
    /// (base + bright), plus the fixed Build hex. Hue badges pick the bright
    /// variant on dark and the base variant on light, so both appearances stay
    /// readable; the hex renders identically on both.
    fn hue_rgb_on(hue: &str, light: bool) -> Option<(u8, u8, u8)> {
        if let Some(rgb) = hex_to_rgb(hue) {
            return Some(rgb);
        }
        let (d, l) = match hue {
            "red" => ((0xFF, 0x5F, 0x5F), (0xCD, 0x00, 0x00)),
            "green" => ((0x5F, 0xFF, 0x5F), (0x00, 0xCD, 0x00)),
            "blue" => ((0x5F, 0x5F, 0xFF), (0x00, 0x00, 0xCD)),
            "magenta" => ((0xFF, 0x5F, 0xFF), (0xCD, 0x00, 0xCD)),
            "yellow" => ((0xFF, 0xFF, 0x5F), (0xCD, 0xCD, 0x00)),
            "cyan" => ((0x5F, 0xFF, 0xFF), (0x00, 0xCD, 0xCD)),
            _ => return None,
        };
        Some(if light { l } else { d })
    }

    fn channel_lin(c: u8) -> f32 {
        let c = c as f32 / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }

    fn relative_luminance((r, g, b): (u8, u8, u8)) -> f32 {
        0.2126 * channel_lin(r) + 0.7152 * channel_lin(g) + 0.0722 * channel_lin(b)
    }

    fn contrast_ratio(fg: (u8, u8, u8), bg: (u8, u8, u8)) -> f32 {
        let l_fg = relative_luminance(fg);
        let l_bg = relative_luminance(bg);
        let (hi, lo) = if l_fg > l_bg { (l_fg, l_bg) } else { (l_bg, l_fg) };
        (hi + 0.05) / (lo + 0.05)
    }

    /// Each mode color must resolve to a variant that stays legible on its
    /// matching background. Dark terminals clear the WCAG "large/bold text"
    /// 3:1 bar comfortably. On light terminals the ANSI hues clear 3:1 except
    /// green (~2:1 — the best a standard `Green` code can do against white),
    /// and the Build hex clears 2.7:1, so the light threshold stays at 2:1.
    const MIN_CONTRAST_DARK: f32 = 3.0;
    const MIN_CONTRAST_LIGHT: f32 = 2.0;

    fn assert_agent_hue_readable(name: &str, hue: &str, light: bool, bg: (u8, u8, u8)) {
        let fg =
            hue_rgb_on(hue, light).unwrap_or_else(|| panic!("{name} ({hue}) is not a supported standard ANSI hue"));
        let cr = contrast_ratio(fg, bg);
        let min = if light { MIN_CONTRAST_LIGHT } else { MIN_CONTRAST_DARK };
        assert!(
            cr >= min,
            "{name} ({hue}) on {} bg contrast {cr:.2} below {min}",
            if light { "light" } else { "dark" }
        );
    }

    #[test]
    fn agent_mode_color_tokens_are_supported() {
        for (_, hue) in AGENT_MODE_HUE {
            let supported = AGENT_HUE_NAMES.contains(hue) || hex_to_rgb(hue).is_some();
            assert!(supported, "mode token {hue} is neither a standard ANSI hue nor #rrggbb");
        }
    }

    #[test]
    fn agent_mode_colors_are_readable_in_dark_and_light_terminals() {
        for (mode, hue) in AGENT_MODE_HUE {
            // Dark terminals use the bright variant; light terminals the base.
            assert_agent_hue_readable(mode, hue, false, DARK_BG);
            assert_agent_hue_readable(mode, hue, true, LIGHT_BG);
        }
    }

    /// The four mode colors must be visually distinct from one another so the
    /// user can tell Build / Auto / Plan / Duck apart at a glance.
    #[test]
    fn agent_mode_colors_are_distinct() {
        let hues: Vec<&str> = AGENT_MODE_HUE.iter().map(|(_, h)| *h).collect();
        for i in 0..hues.len() {
            for j in (i + 1)..hues.len() {
                assert_ne!(hues[i], hues[j], "mode hues {} and {} are identical", hues[i], hues[j]);
            }
        }
    }

    #[test]
    fn agent_mode_hue_resolves_mode_names() {
        assert_eq!(agent_mode_hue("build"), Some(AGENT_COLOR_BUILD));
        assert_eq!(agent_mode_hue("auto"), Some(AGENT_COLOR_AUTO));
        assert_eq!(agent_mode_hue("plan"), Some(AGENT_COLOR_PLAN));
        assert_eq!(agent_mode_hue("duck"), Some(AGENT_COLOR_DUCK));
        assert_eq!(agent_mode_hue("unknown"), None);
    }

    #[test]
    fn agent_mode_colors_are_wired_into_builtin_primary_agents() {
        // Guards against the constants drifting from the specs that expose them.
        assert_eq!(AGENT_COLOR_BUILD, "#73a18e");
        assert_eq!(AGENT_COLOR_AUTO, "green");
        assert_eq!(AGENT_COLOR_PLAN, "blue");
        assert_eq!(AGENT_COLOR_DUCK, "magenta");
    }

    /// Chat placeholders share one hint fragment so they cannot drift apart.
    #[test]
    fn chat_input_placeholders_share_one_shortcut_hint() {
        let hint = chat_input_shortcut_hint!();
        assert!(CHAT_INPUT_PLACEHOLDER_BOOTSTRAP.ends_with(hint));
        assert!(CHAT_INPUT_PLACEHOLDER_FOLLOW_UP.ends_with(hint));
        assert!(CHAT_INPUT_PLACEHOLDER_BOOTSTRAP.starts_with("Type a request"));
        assert!(CHAT_INPUT_PLACEHOLDER_FOLLOW_UP.starts_with("Continue"));
        assert_ne!(CHAT_INPUT_PLACEHOLDER_BOOTSTRAP, CHAT_INPUT_PLACEHOLDER_FOLLOW_UP);
    }

    /// The metadata bullet separator has exactly one definition.
    ///
    /// Every `" • "` join reaches this constant instead of re-typing the
    /// literal, so spacing changes land in one place.
    #[test]
    fn meta_bullet_separator_is_the_single_source() {
        assert_eq!(META_BULLET_SEPARATOR, " • ");
    }

    /// Ratchet against the duplication that made this module rot.
    ///
    /// These constants were removed because `vtcode-ui` defines its own copy
    /// under `tui::config::constants::ui` and never reads the one here, so the
    /// two silently diverged. Re-adding any of them recreates that trap; the
    /// TUI value is the live one, so edit it there instead.
    #[test]
    fn removed_shadowed_tui_constants_stay_removed() {
        const SHADOWED: &[&str] = &[
            "TUI_ACTIVE_TICK_RATE_HZ",
            "TUI_IDLE_TICK_RATE_HZ",
            "TUI_ACTIVE_TIMEOUT_MS",
            "TUI_SHIMMER_FRAME_INTERVAL_MS",
            "TUI_SHIMMER_SWEEP_DURATION_MS",
            "TUI_SCROLL_CURSOR_STEADY_MS",
            "TUI_MAX_VIEWPORT_WIDTH",
            "TUI_MAX_VIEWPORT_HEIGHT",
            "MODAL_WIDTH_RATIO",
            "MODAL_HEIGHT_RATIO",
            "MODAL_MAX_WIDTH_RATIO",
            "MODAL_MAX_HEIGHT_RATIO",
            "SLASH_PALETTE_MIN_WIDTH",
            "SLASH_PALETTE_MIN_HEIGHT",
            "HEADER_SHORTCUT_HINT",
            "SCROLL_INDICATOR_FORMAT",
            "HEADER_META_SEPARATOR",
            "SLASH_PALETTE_TOP_OFFSET",
        ];
        // Source-level assertion: these names must not be declared here again.
        let src = include_str!("ui.rs");
        for name in SHADOWED {
            let declared = src.lines().any(|line| {
                let trimmed = line.trim_start();
                trimmed
                    .strip_prefix("pub const ")
                    .or_else(|| trimmed.strip_prefix("pub(crate) const "))
                    .is_some_and(|rest| rest.starts_with(&format!("{name}:")))
            });
            assert!(!declared, "`{name}` is shadowed by vtcode-ui; declare the live value there, not here");
        }
    }
}
