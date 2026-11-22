// Re-export shared types from vtcode-commons. The conversions to/from the
// canonical `KeyboardProtocolConfig` live in `vtcode-config`, which owns that
// type; a local type in `From`'s generic slot makes both directions legal
// there without this crate carrying a shadowed copy.
pub use vtcode_commons::ui_protocol::{KeyboardProtocolSettings, SessionSurface};

/// Standalone fullscreen interaction settings for alternate-screen behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FullscreenInteractionSettings {
    pub mouse_capture: bool,
    pub copy_on_select: bool,
    pub scroll_speed: u8,
}

impl Default for FullscreenInteractionSettings {
    fn default() -> Self {
        Self {
            mouse_capture: true,
            copy_on_select: true,
            scroll_speed: 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::config::KeyboardProtocolConfig;

    #[test]
    fn session_surface_conversion_roundtrip() {
        let variants = [SessionSurface::Auto, SessionSurface::Alternate, SessionSurface::Inline];

        for variant in variants {
            let converted: crate::tui::config::UiSurfacePreference = variant.into();
            let roundtrip = SessionSurface::from(converted);
            assert_eq!(variant, roundtrip);
        }
    }

    #[test]
    fn ui_surface_defaults_to_inline() {
        assert_eq!(SessionSurface::default(), SessionSurface::Inline);
        assert_eq!(crate::tui::config::UiSurfacePreference::default(), crate::tui::config::UiSurfacePreference::Inline);
    }

    #[test]
    fn keyboard_protocol_conversion_roundtrip() {
        let settings = KeyboardProtocolSettings {
            enabled: true,
            mode: "custom".to_string(),
            disambiguate_escape_codes: true,
            report_event_types: false,
            report_alternate_keys: true,
            report_all_keys: false,
        };

        let config: KeyboardProtocolConfig = settings.clone().into();
        let restored = KeyboardProtocolSettings::from(config);

        assert_eq!(settings, restored);
    }

    #[test]
    fn fullscreen_interaction_settings_default_values() {
        let settings = FullscreenInteractionSettings::default();

        assert!(settings.mouse_capture);
        assert!(settings.copy_on_select);
        assert_eq!(settings.scroll_speed, 3);
    }
}
