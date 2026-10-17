use anstyle::{Color, RgbColor, Style};

use crate::theme::color_math::contrast_ratio;
use crate::theme::registry::all_theme_definitions;
use crate::*;

#[test]
fn test_mono_theme_exists() {
    let result = ensure_theme("mono");
    assert!(result.is_ok(), "Mono theme should be registered");
    assert_eq!(result.unwrap(), "Mono");
}

#[test]
fn test_mono_theme_contrast() {
    let result = validate_theme_contrast("mono");
    assert!(result.errors.is_empty(), "Mono theme should have no errors");
    assert!(result.is_valid);
}

#[test]
fn test_ansi_classic_theme_exists() {
    let result = ensure_theme("ansi-classic");
    assert!(result.is_ok(), "ANSI Classic theme should be registered");
    assert_eq!(result.unwrap(), "ANSI Classic");
}

#[test]
fn test_all_themes_resolvable() {
    for id in available_themes() {
        assert!(ensure_theme(id).is_ok(), "Theme {id} should be resolvable");
    }
}

#[test]
fn test_available_theme_suites_contains_expected_groups() {
    let suites = available_theme_suites();
    let suite_ids: Vec<&str> = suites.iter().map(|suite| suite.id).collect();
    assert!(suite_ids.contains(&"ciapre"));
    assert!(suite_ids.contains(&"vitesse"));
    assert!(suite_ids.contains(&"catppuccin"));
    assert!(suite_ids.contains(&"mono"));
}

#[test]
fn test_theme_suite_resolution() {
    assert_eq!(theme_suite_id("catppuccin-mocha"), Some("catppuccin"));
    assert_eq!(theme_suite_id("vitesse-light"), Some("vitesse"));
    assert_eq!(theme_suite_id("ciapre-dark"), Some("ciapre"));
    assert_eq!(theme_suite_id("mono"), Some("mono"));
    assert_eq!(theme_suite_id("unknown-theme"), None);
}

#[test]
fn test_all_themes_have_readable_foreground_and_accents() {
    let accessibility = ColorAccessibilityConfig::default();
    let min_contrast = accessibility.minimum_contrast;
    for definition in all_theme_definitions().values() {
        let styles = definition.palette.build_styles_with_accessibility(&accessibility);
        let bg = definition.palette.background;

        for (name, color) in [
            ("foreground", style_rgb(styles.output)),
            ("primary", style_rgb(styles.primary)),
            ("secondary", style_rgb(styles.secondary)),
            ("user", style_rgb(styles.user)),
            ("response", style_rgb(styles.response)),
            ("info", style_rgb(styles.info)),
            ("error", style_rgb(styles.error)),
            ("warning", style_rgb(styles.warning)),
            ("reasoning", style_rgb(styles.reasoning)),
            ("tool", style_rgb(styles.tool)),
            ("tool_detail", style_rgb(styles.tool_detail)),
            ("tool_output", style_rgb(styles.tool_output).or(style_rgb(styles.output))),
            ("pty_output", style_rgb(styles.pty_output)),
            ("status", style_rgb(styles.status)),
            ("mcp", style_rgb(styles.mcp)),
        ] {
            let color = color.unwrap_or_else(|| panic!("{} missing fg color for {}", name, definition.id));
            let ratio = contrast_ratio(color, bg);
            assert!(
                ratio >= min_contrast,
                "theme={} style={} contrast {:.2} < {:.1}",
                definition.id,
                name,
                ratio,
                min_contrast
            );
        }
    }
}

#[test]
#[serial_test::serial(theme_runtime)]
fn style_contrast_ratio_reports_the_theme_minimum_for_banner_copy() {
    let original_theme = active_theme_id();
    for id in available_themes() {
        set_active_theme(id).expect("built-in theme");
        let minimum = get_minimum_contrast();
        let styles = active_styles();

        for (name, style) in [
            ("banner", banner_style()),
            ("info", styles.info),
            ("tool_detail", styles.tool_detail),
            ("primary", styles.primary),
        ] {
            let ratio = style_contrast_ratio(&style).unwrap_or_else(|| panic!("{id}/{name} needs an RGB fg"));
            assert!(ratio >= minimum, "theme={id} token={name} contrast {ratio:.2} below {minimum:.1}");
        }

        // A style without an explicit foreground has no theme-relative ratio.
        assert_eq!(style_contrast_ratio(&Style::new()), None);
    }
    set_active_theme(&original_theme).expect("restore the previously active theme");
}

#[test]
#[serial_test::serial(theme_runtime)]
fn committed_and_cancelled_theme_changes_do_not_leave_a_preview() {
    let original_theme = active_theme_id();
    let committed_theme = if original_theme == "ciapre" { "mono" } else { "ciapre" };
    let preview_theme = if committed_theme == "mono" {
        "ciapre-blue"
    } else {
        "mono"
    };

    set_active_theme(committed_theme).expect("built-in committed theme");
    set_preview_theme(preview_theme).expect("built-in preview theme");
    assert!(has_preview_theme(), "selection movement should install a temporary preview");

    set_active_theme(preview_theme).expect("built-in committed preview theme");
    assert_eq!(active_theme_id(), preview_theme);
    assert!(!has_preview_theme(), "committing a theme must clear its temporary preview");

    set_preview_theme(committed_theme).expect("built-in cancelled preview theme");
    assert!(has_preview_theme(), "cancel setup needs an active preview");
    clear_preview_theme();
    assert_eq!(active_theme_id(), preview_theme, "cancelling must retain the last committed theme");
    assert!(!has_preview_theme(), "cancelling must clear the temporary preview");

    set_active_theme(&original_theme).expect("restore original theme after test");
}

#[test]
fn reasoning_style_is_dimmed_and_italicized() {
    let accessibility = ColorAccessibilityConfig::default();
    for definition in all_theme_definitions().values() {
        let effects = definition
            .palette
            .build_styles_with_accessibility(&accessibility)
            .reasoning
            .get_effects();
        assert!(effects.contains(anstyle::Effects::DIMMED), "theme={} reasoning should be dimmed", definition.id);
        assert!(effects.contains(anstyle::Effects::ITALIC), "theme={} reasoning should be italic", definition.id);
    }
}

#[test]
fn test_all_themes_error_accent_meets_contrast() {
    // The error/alert token backs the Blocked header badge and error copy, so
    // it must meet the WCAG AA contrast floor against the background in every
    // built-in theme. Unlike body-text tokens it is an accent, so the
    // readability luminance window does not apply.
    let accessibility = ColorAccessibilityConfig::default();
    let min_contrast = accessibility.minimum_contrast;
    for definition in all_theme_definitions().values() {
        let styles = definition.palette.build_styles_with_accessibility(&accessibility);
        let color = style_rgb(styles.error).unwrap_or_else(|| panic!("error token missing fg for {}", definition.id));
        let ratio = contrast_ratio(color, definition.palette.background);
        assert!(
            ratio >= min_contrast,
            "theme={} error accent contrast {:.2} < {:.1}",
            definition.id,
            ratio,
            min_contrast
        );
    }
}

#[test]
fn test_all_themes_warning_accent_meets_contrast() {
    // The warning token backs warning transcript copy, so it must meet the
    // WCAG AA contrast floor against the background in every built-in theme.
    let accessibility = ColorAccessibilityConfig::default();
    let min_contrast = accessibility.minimum_contrast;
    for definition in all_theme_definitions().values() {
        let styles = definition.palette.build_styles_with_accessibility(&accessibility);
        let color =
            style_rgb(styles.warning).unwrap_or_else(|| panic!("warning token missing fg for {}", definition.id));
        let ratio = contrast_ratio(color, definition.palette.background);
        assert!(
            ratio >= min_contrast,
            "theme={} warning accent contrast {:.2} < {:.1}",
            definition.id,
            ratio,
            min_contrast
        );
    }
}

#[test]
fn test_syntax_theme_mapping_dark_themes() {
    assert_eq!(get_syntax_theme_for_ui_theme("dracula"), "Dracula");
    assert_eq!(get_syntax_theme_for_ui_theme("monokai-classic"), "monokai-classic");
    assert_eq!(get_syntax_theme_for_ui_theme("github-dark"), "GitHub Dark");
    assert_eq!(get_syntax_theme_for_ui_theme("atom-one-dark"), "OneDark");
    assert_eq!(get_syntax_theme_for_ui_theme("ayu"), "ayu-dark");
    assert_eq!(get_syntax_theme_for_ui_theme("ayu-mirage"), "ayu-mirage");
}

#[test]
fn test_syntax_theme_mapping_light_themes() {
    assert_eq!(get_syntax_theme_for_ui_theme("solarized-light"), "Solarized (light)");
    assert_eq!(get_syntax_theme_for_ui_theme("vitesse-light"), "base16-ocean.light");
    assert_eq!(get_syntax_theme_for_ui_theme("apple-system-colors-light"), "base16-ocean.light");
}

#[test]
fn test_syntax_theme_mapping_solarized() {
    assert_eq!(get_syntax_theme_for_ui_theme("solarized-dark"), "Solarized (dark)");
    assert_eq!(get_syntax_theme_for_ui_theme("solarized-dark-hc"), "Solarized (dark)");
}

#[test]
fn test_syntax_theme_mapping_gruvbox() {
    assert_eq!(get_syntax_theme_for_ui_theme("gruvbox-dark"), "gruvbox-dark");
    assert_eq!(get_syntax_theme_for_ui_theme("gruvbox-light"), "gruvbox-light");
    assert_eq!(get_syntax_theme_for_ui_theme("gruvbox-material"), "gruvbox-dark");
    assert_eq!(get_syntax_theme_for_ui_theme("gruvbox-material-light"), "gruvbox-light");
}

#[test]
fn test_theme_for_terminal_scheme_change_prefers_suite_twin() {
    // Catppuccin spans both schemes: a report must land on the suite twin.
    let light_twin = theme_for_terminal_scheme_change("catppuccin-mocha", false);
    assert_eq!(theme_suite_id(light_twin), Some("catppuccin"));
    assert!(is_light_theme(light_twin));

    let dark_twin = theme_for_terminal_scheme_change("catppuccin-latte", true);
    assert_eq!(theme_suite_id(dark_twin), Some("catppuccin"));
    assert!(!is_light_theme(dark_twin));
}

#[test]
fn test_theme_for_terminal_scheme_change_falls_back_without_twin() {
    // mono is a single dark theme with no light twin; a light report falls
    // back to the default suggestion for the reported scheme.
    vtcode_commons::ansi_capabilities::set_color_scheme_override(Some(
        vtcode_commons::ansi_capabilities::ColorScheme::Light,
    ));
    assert_eq!(theme_for_terminal_scheme_change("mono", false), "vitesse-light");
    vtcode_commons::ansi_capabilities::set_color_scheme_override(None);
}

#[test]
fn test_theme_for_terminal_scheme_change_falls_back_for_unknown_theme() {
    vtcode_commons::ansi_capabilities::set_color_scheme_override(Some(
        vtcode_commons::ansi_capabilities::ColorScheme::Dark,
    ));
    assert_eq!(theme_for_terminal_scheme_change("not-a-theme", true), DEFAULT_THEME_ID);
    vtcode_commons::ansi_capabilities::set_color_scheme_override(None);
}

fn style_rgb(style: Style) -> Option<RgbColor> {
    match style.get_fg_color() {
        Some(Color::Rgb(rgb)) => Some(rgb),
        _ => None,
    }
}

#[test]
fn test_all_themes_sticky_prompt_meets_contrast() {
    use crate::tui::core_tui::{
        session::Session,
        style::theme_from_styles,
        types::{InlineMessageKind, InlineSegment, InlineTextStyle},
        widgets::TranscriptWidget,
    };
    use ratatui::{buffer::Buffer, layout::Rect, style::Color as TuiColor, widgets::Widget};
    use std::sync::Arc;

    let accessibility = ColorAccessibilityConfig::default();
    for definition in all_theme_definitions().values() {
        let styles = definition.palette.build_styles_with_accessibility(&accessibility);
        let mut session = Session::new(theme_from_styles(&styles), None, 16);
        for index in 0..16 {
            session.push_line(
                if index == 0 {
                    InlineMessageKind::User
                } else {
                    InlineMessageKind::Agent
                },
                vec![InlineSegment {
                    text: "prompt or answer".into(),
                    style: Arc::new(InlineTextStyle::default()),
                }],
            );
        }
        let area = Rect::new(0, 0, 40, 8);
        let mut buffer = Buffer::empty(area);
        TranscriptWidget::new(&mut session).render(area, &mut buffer);
        assert_eq!(session.transcript_area().unwrap().y, 1, "header is visible for {}", definition.id);
        let (TuiColor::Rgb(fr, fg, fb), TuiColor::Rgb(br, bg, bb)) = (buffer[(0, 0)].fg, buffer[(0, 0)].bg) else {
            panic!("RGB theme {} must render RGB header colors", definition.id)
        };
        let ratio = contrast_ratio(RgbColor(fr, fg, fb), RgbColor(br, bg, bb));
        assert!(ratio >= accessibility.minimum_contrast, "theme={} sticky prompt contrast {ratio:.2}", definition.id);
    }
}

#[test]
fn test_all_themes_ultrafast_header_badge_meets_contrast() {
    use crate::tui::core_tui::{
        session::Session,
        types::{InlineCommand, InlineHeaderContext},
    };
    use ratatui::style::{Color as TerminalColor, Modifier};
    use vtcode_commons::ui_protocol::theme_from_color_fields;
    use vtcode_config::constants::ui;

    for definition in all_theme_definitions().values() {
        let styles = definition
            .palette
            .build_styles_with_accessibility(&ColorAccessibilityConfig::default());
        let theme = theme_from_color_fields(
            styles.foreground,
            styles.background,
            styles.primary,
            styles.secondary,
            styles.tool,
            styles.tool_detail,
            styles.pty_output,
            styles.error,
            styles.warning,
        );
        let mut session = Session::new(theme, None, 24);
        session.handle_command(InlineCommand::SetHeaderContext {
            context: Box::new(InlineHeaderContext {
                service_tier: Some(format!("{}ultrafast", ui::HEADER_SERVICE_TIER_PREFIX)),
                ..Default::default()
            }),
        });
        let line = session.header_title_line();
        let badge = line
            .spans
            .iter()
            .find(|span| span.content == " ULTRAFAST ")
            .expect("premium badge");
        if vtcode_commons::color_policy::color_output_enabled() {
            let rgb = |color| match color {
                Some(TerminalColor::Rgb(r, g, b)) => RgbColor(r, g, b),
                other => panic!("expected badge RGB colors, got {other:?}"),
            };
            let ratio = contrast_ratio(rgb(badge.style.bg), rgb(badge.style.fg));
            assert!(ratio >= 7.0, "{} badge contrast {ratio:.2} is below WCAG AAA", definition.id);
            assert_eq!(badge.style.bg, Some(TerminalColor::Rgb(0xD9, 0x9A, 0x4E)));
            assert!(!badge.style.add_modifier.contains(Modifier::REVERSED));
        } else {
            assert_eq!(badge.style.fg, Some(TerminalColor::Reset));
            assert_eq!(badge.style.bg, Some(TerminalColor::Reset));
            assert!(badge.style.add_modifier.contains(Modifier::REVERSED));
        }
        assert!(badge.style.sub_modifier.contains(Modifier::DIM));
    }
}
