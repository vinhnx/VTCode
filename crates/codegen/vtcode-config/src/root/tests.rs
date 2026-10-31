use super::*;
use serial_test::serial;
use vtcode_commons::ui_protocol::ThinkingBlockState;

#[test]
fn program_status_defaults_on_and_round_trip() {
    assert!(UiConfig::default().program_status.enabled);
    assert!(toml::from_str::<UiConfig>("").unwrap().program_status.enabled);
    assert!(toml::from_str::<UiConfig>("[program_status]").unwrap().program_status.enabled);
    let disabled: UiConfig = toml::from_str("[program_status]\nenabled = false").unwrap();
    assert!(!disabled.program_status.enabled);
    let serialized = toml::to_string(&disabled).unwrap();
    assert!(!toml::from_str::<UiConfig>(&serialized).unwrap().program_status.enabled);
    assert!(toml::from_str::<UiConfig>("[program_status]\nenabled = 'true'").is_err());
}

#[test]
fn program_status_layer_precedence_can_enable_and_disable() {
    use crate::loader::layers::{ConfigLayerEntry, ConfigLayerSource, ConfigLayerStack};
    for (lower, higher) in [(false, true), (true, false)] {
        let layer = |enabled| {
            ConfigLayerEntry::new(
                ConfigLayerSource::Runtime,
                toml::from_str(&format!("[ui.program_status]\nenabled = {enabled}")).unwrap(),
            )
        };
        let stack = ConfigLayerStack::new(vec![layer(lower), layer(higher)]);
        let merged: crate::loader::VTCodeConfig = stack.effective_config_without_origins().try_into().unwrap();
        assert_eq!(merged.ui.program_status.enabled, higher);
    }
}

fn with_env_var<F>(key: &str, value: Option<&str>, f: F)
where
    F: FnOnce(),
{
    let previous = crate::env_helpers::test_env_overrides::get(key);
    crate::env_helpers::test_env_overrides::set(key, value);
    f();
    crate::env_helpers::test_env_overrides::restore(key, previous);
}

#[test]
#[serial]
fn fullscreen_defaults_match_expected_values() {
    let fullscreen = UiFullscreenConfig::default();

    assert!(fullscreen.mouse_capture);
    assert!(fullscreen.copy_on_select);
    assert_eq!(fullscreen.scroll_speed, 3);
}

#[test]
fn fullscreen_copy_on_select_parses_manual_mode() {
    let manual: UiFullscreenConfig = toml::from_str("copy_on_select = false").expect("manual copy mode should parse");
    assert!(!manual.copy_on_select);
    assert!(manual.mouse_capture);

    let round_trip = toml::to_string(&manual).expect("manual copy mode serializes");
    assert!(round_trip.contains("copy_on_select = false"));
    assert!(
        !toml::from_str::<UiFullscreenConfig>(&round_trip)
            .expect("manual copy mode round trips")
            .copy_on_select
    );
}

#[test]
#[serial]
fn thinking_display_defaults_to_collapsed() {
    let ui = UiConfig::default();
    assert_eq!(ui.thinking_display, ThinkingBlockState::Collapsed);
}

#[test]
#[serial]
fn reduce_motion_uses_environment_default_and_preserves_animation_override() {
    with_env_var("VTCODE_REDUCE_MOTION", Some("1"), || {
        let ui = UiConfig::default();
        assert!(ui.reduce_motion_mode);
        assert!(!ui.reduce_motion_keep_progress_animation);

        let explicit_false: UiConfig = toml::from_str("reduce_motion_mode = false")
            .expect("explicit false should parse while the environment default is enabled");
        assert!(!explicit_false.reduce_motion_mode);
    });

    with_env_var("VTCODE_REDUCE_MOTION", Some("0"), || {
        let explicit_true: UiConfig =
            toml::from_str("reduce_motion_mode = true\nreduce_motion_keep_progress_animation = true")
                .expect("explicit true should parse while the environment default is disabled");
        assert!(explicit_true.reduce_motion_mode);
        assert!(explicit_true.reduce_motion_keep_progress_animation);
    });
}

#[test]
fn tool_display_mode_defaults_to_compact() {
    let ui = UiConfig::default();
    assert_eq!(ui.tool_display_mode, ToolDisplayMode::Compact);
    assert!(ui.keybindings.is_empty());
    assert!(ui.transcript_review.show_hints);
    assert!(ui.transcript_review.show_shortcut_guide);
    assert!(ui.transcript_review.show_close_button);
}

#[test]
fn diff_preview_mode_defaults_to_inline_and_parses_side_by_side() {
    assert_eq!(UiConfig::default().diff_preview_mode, DiffPreviewMode::Inline);
    assert_eq!(toml::from_str::<UiConfig>("").expect("empty parses").diff_preview_mode, DiffPreviewMode::Inline);

    let side: UiConfig = toml::from_str("diff_preview_mode = \"side-by-side\"").expect("side-by-side mode parses");
    assert_eq!(side.diff_preview_mode, DiffPreviewMode::SideBySide);

    let inline: UiConfig = toml::from_str("diff_preview_mode = \"inline\"").expect("inline mode parses");
    assert_eq!(inline.diff_preview_mode, DiffPreviewMode::Inline);

    let round_trip = toml::to_string(&side).expect("side-by-side serializes");
    assert!(round_trip.contains("diff_preview_mode = \"side-by-side\""));
    assert_eq!(
        toml::from_str::<UiConfig>(&round_trip)
            .expect("side-by-side round trips")
            .diff_preview_mode,
        DiffPreviewMode::SideBySide
    );
}

#[test]
fn transcript_review_controls_and_keybindings_parse() {
    let ui: UiConfig = toml::from_str(
        r#"
        [keybindings]
        open_transcript_review = ["ctrl+x"]

        [transcript_review]
        show_hints = false
        show_shortcut_guide = false
        show_close_button = false
        "#,
    )
    .expect("transcript review settings parse");

    assert_eq!(ui.keybindings.get("open_transcript_review"), Some(&vec!["ctrl+x".to_string()]));
    assert!(!ui.transcript_review.show_hints);
    assert!(!ui.transcript_review.show_shortcut_guide);
    assert!(!ui.transcript_review.show_close_button);
}

#[test]
fn task_panel_defaults_to_hidden_and_parses() {
    assert!(!UiConfig::default().show_task_panel);

    let enabled: UiConfig = toml::from_str("show_task_panel = true").expect("show_task_panel parses");
    assert!(enabled.show_task_panel);
    assert!(!toml::from_str::<UiConfig>("").expect("empty parses").show_task_panel);
}

#[test]
fn tool_display_mode_round_trips_through_toml() {
    let compact: UiConfig = toml::from_str("tool_display_mode = \"compact\"").expect("compact mode parses");
    let expanded: UiConfig = toml::from_str("tool_display_mode = \"expanded\"").expect("expanded mode parses");

    assert_eq!(compact.tool_display_mode, ToolDisplayMode::Compact);
    assert_eq!(expanded.tool_display_mode, ToolDisplayMode::Expanded);
    let compact_toml = toml::to_string(&compact).expect("compact mode serializes");
    let expanded_toml = toml::to_string(&expanded).expect("expanded mode serializes");
    assert!(compact_toml.contains("tool_display_mode = \"compact\""));
    assert!(expanded_toml.contains("tool_display_mode = \"expanded\""));
    assert_eq!(
        toml::from_str::<UiConfig>(&compact_toml)
            .expect("compact mode round trips")
            .tool_display_mode,
        ToolDisplayMode::Compact
    );
    assert_eq!(
        toml::from_str::<UiConfig>(&expanded_toml)
            .expect("expanded mode round trips")
            .tool_display_mode,
        ToolDisplayMode::Expanded
    );
}

#[test]
fn unknown_tool_display_mode_is_compatible() {
    let ui: UiConfig = toml::from_str("tool_display_mode = \"future_mode\"").expect("unknown mode parses");
    assert_eq!(ui.tool_display_mode, ToolDisplayMode::Unknown);
}

#[test]
#[serial]
fn fullscreen_env_overrides_apply_to_defaults() {
    with_env_var("VTCODE_FULLSCREEN_MOUSE_CAPTURE", Some("0"), || {
        with_env_var("VTCODE_FULLSCREEN_COPY_ON_SELECT", Some("false"), || {
            with_env_var("VTCODE_FULLSCREEN_SCROLL_SPEED", Some("7"), || {
                let fullscreen = UiFullscreenConfig::default();
                assert!(!fullscreen.mouse_capture);
                assert!(!fullscreen.copy_on_select);
                assert_eq!(fullscreen.scroll_speed, 7);
            });
        });
    });
}

#[test]
#[serial]
fn fullscreen_scroll_speed_is_clamped() {
    with_env_var("VTCODE_FULLSCREEN_SCROLL_SPEED", Some("0"), || {
        assert_eq!(UiFullscreenConfig::default().scroll_speed, 1);
    });

    with_env_var("VTCODE_FULLSCREEN_SCROLL_SPEED", Some("99"), || {
        assert_eq!(UiFullscreenConfig::default().scroll_speed, 20);
    });
}
