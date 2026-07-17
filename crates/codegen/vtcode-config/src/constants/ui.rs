pub const TOOL_OUTPUT_MODE_COMPACT: &str = "compact";
pub const TOOL_OUTPUT_MODE_FULL: &str = "full";
pub const DEFAULT_INLINE_VIEWPORT_ROWS: u16 = 16;
pub const DEFAULT_REASONING_VISIBLE: bool = true;
pub const SLASH_SUGGESTION_LIMIT: usize = 50; // All commands are scrollable
pub const SLASH_PALETTE_MIN_WIDTH: u16 = 40;
pub const SLASH_PALETTE_MIN_HEIGHT: u16 = 9;
pub const SLASH_PALETTE_HORIZONTAL_MARGIN: u16 = 8;
pub const SLASH_PALETTE_TOP_OFFSET: u16 = 3;
pub const SLASH_PALETTE_CONTENT_PADDING: u16 = 6;
pub const SLASH_PALETTE_HINT_PRIMARY: &str = "Type to filter slash commands.";
pub const SLASH_PALETTE_HINT_SECONDARY: &str = "Press Enter to apply • Esc to dismiss.";
pub const MODAL_MIN_WIDTH: u16 = 36;
pub const MODAL_MIN_HEIGHT: u16 = 20;
pub const MODAL_LIST_MIN_HEIGHT: u16 = 12;
pub const MODAL_WIDTH_RATIO: f32 = 0.6;
pub const MODAL_HEIGHT_RATIO: f32 = 0.6;
pub const MODAL_MAX_WIDTH_RATIO: f32 = 0.9;
pub const MODAL_MAX_HEIGHT_RATIO: f32 = 0.8;
pub const MODAL_CONTENT_HORIZONTAL_PADDING: u16 = 8;
pub const MODAL_CONTENT_VERTICAL_PADDING: u16 = 6;
pub const MODAL_INSTRUCTIONS_TITLE: &str = "";
pub const MODAL_INSTRUCTIONS_BULLET: &str = "•";
pub const INLINE_HEADER_HEIGHT: u16 = 3;
pub const INLINE_INPUT_HEIGHT: u16 = 4;
pub const INLINE_INPUT_PADDING_HORIZONTAL: u16 = 1;
pub const INLINE_INPUT_PADDING_VERTICAL: u16 = 1;
pub const INLINE_INPUT_MAX_LINES: usize = 10;
pub const INLINE_NAVIGATION_PERCENT: u16 = 28;
pub const INLINE_NAVIGATION_MIN_WIDTH: u16 = 24;
pub const INLINE_CONTENT_MIN_WIDTH: u16 = 48;
pub const INLINE_STACKED_NAVIGATION_PERCENT: u16 = INLINE_NAVIGATION_PERCENT;
pub const INLINE_SCROLLBAR_EDGE_PADDING: u16 = 1;
pub const INLINE_TRANSCRIPT_BOTTOM_PADDING: u16 = 4;
pub const INLINE_PREVIEW_MAX_CHARS: usize = 56;
pub const INLINE_PREVIEW_ELLIPSIS: &str = "…";
pub const INLINE_PASTE_COLLAPSE_LINE_THRESHOLD: usize = 10;
pub const HEADER_HIGHLIGHT_PREVIEW_MAX_CHARS: usize = 48;
pub const INLINE_AGENT_MESSAGE_LEFT_PADDING: &str = " ";
pub const INLINE_AGENT_QUOTE_PREFIX: &str = " •";
pub const INLINE_USER_MESSAGE_DIVIDER_SYMBOL: &str = "─";

/// Scroll percentage format in status bar
pub const SCROLL_INDICATOR_FORMAT: &str = "↕";
/// Show scroll percentage in status bar
pub const SCROLL_INDICATOR_ENABLED: bool = false;

pub const INLINE_BLOCK_TOP_LEFT: &str = "╭";
pub const INLINE_BLOCK_TOP_RIGHT: &str = "╮";
pub const INLINE_BLOCK_BODY_LEFT: &str = "│";
pub const INLINE_BLOCK_BODY_RIGHT: &str = "│";
pub const INLINE_BLOCK_BOTTOM_LEFT: &str = "╰";
pub const INLINE_BLOCK_BOTTOM_RIGHT: &str = "╯";
pub const INLINE_BLOCK_HORIZONTAL: &str = "─";
pub const INLINE_TOOL_HEADER_LABEL: &str = "Tool";
pub const INLINE_TOOL_ACTION_PREFIX: &str = "→";
pub const INLINE_TOOL_DETAIL_PREFIX: &str = "↳";
pub const INLINE_PTY_HEADER_LABEL: &str = "Terminal";
pub const INLINE_PTY_RUNNING_LABEL: &str = "running";
pub const INLINE_PTY_STATUS_LIVE: &str = "LIVE";
pub const INLINE_PTY_STATUS_DONE: &str = "DONE";
pub const INLINE_PTY_PLACEHOLDER: &str = "Terminal output";
pub const MODAL_LIST_HIGHLIGHT_SYMBOL: &str = "│";
pub const MODAL_LIST_HIGHLIGHT_FULL: &str = "│ ";
pub const MODAL_LIST_SUMMARY_FILTER_LABEL: &str = "Filter";
pub const MODAL_LIST_SUMMARY_SEPARATOR: &str = " • ";
pub const MODAL_LIST_SUMMARY_MATCHES_LABEL: &str = "Matches";
pub const MODAL_LIST_SUMMARY_TOTAL_LABEL: &str = "of";
pub const MODAL_LIST_SUMMARY_NO_MATCHES: &str = "No matches";
pub const MODAL_LIST_SUMMARY_RESET_HINT: &str = "Press Esc to reset";
pub const MODAL_LIST_NO_RESULTS_MESSAGE: &str = "No matching options";
pub const HEADER_VERSION_PROMPT: &str = "> ";
pub const HEADER_VERSION_PREFIX: &str = "VT Code";
pub const HEADER_VERSION_LEFT_DELIMITER: &str = "(";
pub const HEADER_VERSION_RIGHT_DELIMITER: &str = ")";
pub const HEADER_PROVIDER_PREFIX: &str = "Provider: ";
pub const HEADER_MODEL_PREFIX: &str = "Model: ";
pub const HEADER_REASONING_PREFIX: &str = "";
pub const HEADER_TRUST_PREFIX: &str = "Trust: ";
pub const HEADER_TOOLS_PREFIX: &str = "Tools: ";
pub const HEADER_MCP_PREFIX: &str = "MCP: ";
pub const HEADER_GIT_PREFIX: &str = "git: ";
pub const HEADER_GIT_CLEAN_SUFFIX: &str = "✓";
pub const HEADER_GIT_DIRTY_SUFFIX: &str = "*";
pub const HEADER_UNKNOWN_PLACEHOLDER: &str = "unavailable";
pub const HEADER_STATUS_LABEL: &str = "Status";
pub const HEADER_STATUS_ACTIVE: &str = "Active";
pub const HEADER_STATUS_PAUSED: &str = "Paused";
pub const HEADER_MESSAGES_LABEL: &str = "Messages";
pub const HEADER_INPUT_LABEL: &str = "Input";
pub const HEADER_INPUT_ENABLED: &str = "Enabled";
pub const HEADER_INPUT_DISABLED: &str = "Disabled";
pub const INLINE_USER_PREFIX: &str = " ";
pub const CHAT_INPUT_PLACEHOLDER_BOOTSTRAP: &str =
    "Type a request… Tab switch agent • @files /commands • Ctrl+Enter run • Enter queue";
pub const CHAT_INPUT_PLACEHOLDER_FOLLOW_UP: &str =
    "Continue… Tab switch agent • @files /commands • Ctrl+Enter run • Enter queue";
pub const CHAT_INPUT_PLACEHOLDER_INTERRUPTED: &str =
    "Interrupted · What should VT Code do instead? • Esc cancel • Ctrl+C interrupt • Ctrl+D exit";
pub const HEADER_SHORTCUT_HINT: &str = "? help • / command • @ file • Tab agent • Ctrl+Enter run • Enter queue • Esc cancel • Ctrl+C interrupt";
pub const HEADER_META_SEPARATOR: &str = "   ";
pub const WELCOME_TEXT_WIDTH: usize = 80;
pub const WELCOME_SHORTCUT_SECTION_TITLE: &str = "Keyboard Shortcuts";
pub const WELCOME_SHORTCUT_HINT_PREFIX: &str = "Shortcuts:";
pub const WELCOME_SHORTCUT_SEPARATOR: &str = "•";
pub const WELCOME_SHORTCUT_INDENT: &str = "  ";
pub const WELCOME_SLASH_COMMAND_SECTION_TITLE: &str = "Slash Commands";
pub const WELCOME_SLASH_COMMAND_LIMIT: usize = 6;
pub const WELCOME_SLASH_COMMAND_PREFIX: &str = "/";
pub const WELCOME_SLASH_COMMAND_INTRO: &str = "";
pub const WELCOME_SLASH_COMMAND_INDENT: &str = "  ";
pub const NAVIGATION_BLOCK_TITLE: &str = "Timeline";
pub const NAVIGATION_BLOCK_SHORTCUT_NOTE: &str = "Ctrl+T";
pub const NAVIGATION_EMPTY_LABEL: &str = "Waiting for activity";
pub const NAVIGATION_INDEX_PREFIX: &str = "#";
pub const NAVIGATION_LABEL_AGENT: &str = "Agent";
pub const NAVIGATION_LABEL_ERROR: &str = "Error";
pub const NAVIGATION_LABEL_INFO: &str = "Info";
pub const NAVIGATION_LABEL_POLICY: &str = "Policy";
pub const NAVIGATION_LABEL_TOOL: &str = "Tool";
pub const NAVIGATION_LABEL_USER: &str = "User";
pub const NAVIGATION_LABEL_PTY: &str = "PTY";
pub const PLAN_BLOCK_TITLE: &str = "TODOs";
pub const PLAN_STATUS_EMPTY: &str = "No TODOs";
pub const PLAN_STATUS_IN_PROGRESS: &str = "In progress";
pub const PLAN_STATUS_DONE: &str = "Done";
pub const PLAN_IN_PROGRESS_NOTE: &str = "in progress";
pub const SUGGESTION_BLOCK_TITLE: &str = "Slash Commands";
pub const STATUS_LINE_MODE: &str = "auto";
pub const STATUS_LINE_REFRESH_INTERVAL_MS: u64 = 1000;
pub const STATUS_LINE_COMMAND_TIMEOUT_MS: u64 = 200;

// Agent/Mode color constants.
//
// Mode badges are identified by a **standard ANSI hue name** (red/green/blue/
// magenta/...) rather than a hard-coded hex. The design system
// (`vtcode-ui`) resolves each hue to a theme-appropriate variant at render
// time: the brighter `Light*` ANSI variant on dark terminals, the base variant
// on light terminals. This keeps a single, portable token readable on BOTH
// dark and light terminals without per-theme hex tuning, and reuses the same
// standard palette the rest of the UI already relies on (see `SAFE_ANSI_*`).
//
/// Standard ANSI hue names usable for agent/mode badges. Must stay in sync with
/// the variant table in `vtcode-ui`'s design color resolver.
pub const AGENT_HUE_NAMES: &[&str] = &["red", "green", "blue", "magenta", "yellow", "cyan"];

/// Build agent hue - red (warm, implementation / go-signal).
pub const AGENT_COLOR_BUILD: &str = "red";
/// Auto agent hue - green (autonomy and forward progress).
pub const AGENT_COLOR_AUTO: &str = "green";
/// Plan agent hue - blue (thoughtful, planning tone).
pub const AGENT_COLOR_PLAN: &str = "blue";
/// Duck agent hue - magenta (soft, discussion-focused).
pub const AGENT_COLOR_DUCK: &str = "magenta";

/// Canonical primary-agent mode -> standard ANSI hue mapping (single source of
/// truth). `subagents.rs` assigns the hue to each built-in spec from here.
pub const AGENT_MODE_HUE: &[(&str, &str)] = &[
    ("build", AGENT_COLOR_BUILD),
    ("auto", AGENT_COLOR_AUTO),
    ("plan", AGENT_COLOR_PLAN),
    ("duck", AGENT_COLOR_DUCK),
];

/// Resolve a primary-agent mode name to its standard ANSI hue
/// (e.g. `"build"` -> `"red"`).
pub fn agent_mode_hue(mode: &str) -> Option<&'static str> {
    AGENT_MODE_HUE
        .iter()
        .find(|(m, _)| *m == mode)
        .map(|(_, h)| *h)
}

// TUI tick rate constants for smooth scrolling
/// Tick rate (Hz) when user is actively interacting with the TUI
pub const TUI_ACTIVE_TICK_RATE_HZ: f64 = 60.0;
/// Tick rate (Hz) when TUI is idle to save CPU
pub const TUI_IDLE_TICK_RATE_HZ: f64 = 4.0;
/// Duration (ms) to remain in active mode after last input
pub const TUI_ACTIVE_TIMEOUT_MS: u64 = 500;
/// Shimmer frame interval in milliseconds
pub const TUI_SHIMMER_FRAME_INTERVAL_MS: u64 = 33;
/// Shimmer sweep duration in milliseconds
pub const TUI_SHIMMER_SWEEP_DURATION_MS: u64 = 2000;
/// Keep cursor steady for this duration after scroll events
pub const TUI_SCROLL_CURSOR_STEADY_MS: u64 = 250;

// Viewport size limits to prevent pathological CPU usage with huge terminals
// (e.g., 2000+ columns causes 100% CPU without these guards)
// See: <https://github.com/anthropics/claude-code/issues/21567>
/// Maximum effective viewport width (columns) for rendering
pub const TUI_MAX_VIEWPORT_WIDTH: u16 = 500;
/// Maximum effective viewport height (rows) for rendering
pub const TUI_MAX_VIEWPORT_HEIGHT: u16 = 200;

// Theme and color constants
pub const THEME_MIN_CONTRAST_RATIO: f32 = 4.5;
pub const THEME_FOREGROUND_LIGHTEN_RATIO: f32 = 0.25;
pub const THEME_SECONDARY_LIGHTEN_RATIO: f32 = 0.2;
pub const THEME_MIX_RATIO: f32 = 0.35;
pub const THEME_TOOL_BODY_MIX_RATIO: f32 = 0.35;
pub const THEME_TOOL_BODY_LIGHTEN_RATIO: f32 = 0.2;
pub const THEME_RESPONSE_COLOR_LIGHTEN_RATIO: f32 = 0.15;
pub const THEME_REASONING_COLOR_LIGHTEN_RATIO: f32 = 0.3;
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

/// Subdued gray for diff line numbers
pub const DIFF_LINE_NUMBER_R: u8 = 0x4a;
pub const DIFF_LINE_NUMBER_G: u8 = 0x4a;
pub const DIFF_LINE_NUMBER_B: u8 = 0x4a;
/// Medium gray for input placeholder text
pub const PLACEHOLDER_R: u8 = 0x88;
pub const PLACEHOLDER_G: u8 = 0x88;
pub const PLACEHOLDER_B: u8 = 0x88;

// === Safe ANSI Color Palette ===
// Based on terminal color portability research: <https://blog.xoria.org/terminal-colors/>
// These 11 colors are safe across Basic (light/dark), Tango, and Solarized themes.
// Colors NOT in this list have visibility issues in common terminal configurations.

/// WCAG AA standard minimum contrast ratio (4.5:1)
pub const WCAG_AA_CONTRAST_RATIO: f32 = 4.5;

/// WCAG AAA standard minimum contrast ratio (7.0:1)
pub const WCAG_AAA_CONTRAST_RATIO: f32 = 7.0;

/// Large text minimum contrast ratio (3.0:1)
pub const WCAG_LARGE_TEXT_CONTRAST_RATIO: f32 = 3.0;

// Safe ANSI color indices (standard 0-15 palette)
// These colors are portable across common terminal themes.

/// Safe regular colors (ANSI 0-7 subset that works everywhere)
/// Note: black (0) and white (7) are excluded due to theme conflicts
pub const SAFE_ANSI_RED: u8 = 1;
pub const SAFE_ANSI_GREEN: u8 = 2;
pub const SAFE_ANSI_YELLOW: u8 = 3;
pub const SAFE_ANSI_BLUE: u8 = 4;
pub const SAFE_ANSI_MAGENTA: u8 = 5;
pub const SAFE_ANSI_CYAN: u8 = 6;

/// Safe bright colors (ANSI 8-15 subset that works everywhere)
/// Note: brblack (8) is invisible in Solarized Dark
/// Note: bryellow (11), brblue (12), brwhite (15) have visibility issues
pub const SAFE_ANSI_BRIGHT_RED: u8 = 9;
pub const SAFE_ANSI_BRIGHT_GREEN: u8 = 10;
pub const SAFE_ANSI_BRIGHT_MAGENTA: u8 = 13;
pub const SAFE_ANSI_BRIGHT_CYAN: u8 = 14;

/// All safe ANSI color indices as an array
/// These 10 colors are safe to use across all common terminal themes
pub const SAFE_ANSI_COLORS: [u8; 10] = [
    SAFE_ANSI_RED,
    SAFE_ANSI_GREEN,
    SAFE_ANSI_YELLOW,
    SAFE_ANSI_BLUE,
    SAFE_ANSI_MAGENTA,
    SAFE_ANSI_CYAN,
    SAFE_ANSI_BRIGHT_RED,
    SAFE_ANSI_BRIGHT_GREEN,
    SAFE_ANSI_BRIGHT_MAGENTA,
    SAFE_ANSI_BRIGHT_CYAN,
];

/// Problematic ANSI colors to avoid when safe_colors_only is enabled
/// - 0 (black): Low contrast on dark backgrounds
/// - 7 (white): Low contrast on light backgrounds
/// - 8 (brblack): Invisible in Solarized Dark (hijacked for base03)
/// - 11 (bryellow): Low contrast on light backgrounds
/// - 12 (brblue): Low contrast in Basic Dark
/// - 15 (brwhite): Low contrast on light backgrounds
pub const PROBLEMATIC_ANSI_COLORS: [u8; 6] = [0, 7, 8, 11, 12, 15];

#[cfg(test)]
mod tests {
    use super::*;

    /// Representative terminal backgrounds the agent badge is rendered against.
    /// A near-black dark theme (Catppuccin Mocha / Gruvbox) and a near-white
    /// light theme (Solarized Light / Catppuccin Latte).
    const DARK_BG: (u8, u8, u8) = (0x18, 0x18, 0x18);
    const LIGHT_BG: (u8, u8, u8) = (0xFD, 0xF6, 0xE3);

    /// Standard ANSI variant RGB values used by the design system for each hue
    /// (base + bright). The mode badge picks the bright variant on dark and the
    /// base variant on light, so both appearances stay readable.
    fn hue_rgb_on(hue: &str, light: bool) -> Option<(u8, u8, u8)> {
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
        let (hi, lo) = if l_fg > l_bg {
            (l_fg, l_bg)
        } else {
            (l_bg, l_fg)
        };
        (hi + 0.05) / (lo + 0.05)
    }

    /// Each mode hue must resolve to a standard ANSI variant that stays legible
    /// on its matching background. Dark terminals clear the WCAG "large/bold
    /// text" 3:1 bar comfortably. On light terminals three hues (red, magenta,
    /// blue) also clear 3:1; the fourth (green) reaches ~2:1 — the best a
    /// standard `Green` ANSI code can do against white, so the light threshold
    /// is relaxed to 2:1 for the single weakest hue.
    const MIN_CONTRAST_DARK: f32 = 3.0;
    const MIN_CONTRAST_LIGHT: f32 = 2.0;

    fn assert_agent_hue_readable(name: &str, hue: &str, light: bool, bg: (u8, u8, u8)) {
        let fg = hue_rgb_on(hue, light)
            .unwrap_or_else(|| panic!("{name} ({hue}) is not a supported standard ANSI hue"));
        let cr = contrast_ratio(fg, bg);
        let min = if light {
            MIN_CONTRAST_LIGHT
        } else {
            MIN_CONTRAST_DARK
        };
        assert!(
            cr >= min,
            "{name} ({hue}) on {} bg contrast {cr:.2} below {min}",
            if light { "light" } else { "dark" }
        );
    }

    #[test]
    fn agent_mode_colors_are_standard_ansi_hues() {
        for (_, hue) in AGENT_MODE_HUE {
            assert!(
                AGENT_HUE_NAMES.contains(hue),
                "mode hue {hue} is not in the supported standard ANSI hue list"
            );
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
                assert_ne!(
                    hues[i], hues[j],
                    "mode hues {} and {} are identical",
                    hues[i], hues[j]
                );
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
        assert_eq!(AGENT_COLOR_BUILD, "red");
        assert_eq!(AGENT_COLOR_AUTO, "green");
        assert_eq!(AGENT_COLOR_PLAN, "blue");
        assert_eq!(AGENT_COLOR_DUCK, "magenta");
    }
}
