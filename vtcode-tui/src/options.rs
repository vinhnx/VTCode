use crate::config::KeyboardProtocolConfig;

// Re-export shared types from vtcode-commons.
pub use vtcode_commons::ui_protocol::{KeyboardProtocolSettings, SessionSurface};

impl From<SessionSurface> for crate::config::UiSurfacePreference {
    fn from(value: SessionSurface) -> Self {
        match value {
            SessionSurface::Auto => Self::Auto,
            SessionSurface::Alternate => Self::Alternate,
            SessionSurface::Inline => Self::Inline,
        }
    }
}

impl From<crate::config::UiSurfacePreference> for SessionSurface {
    fn from(value: crate::config::UiSurfacePreference) -> Self {
        match value {
            crate::config::UiSurfacePreference::Auto => Self::Auto,
            crate::config::UiSurfacePreference::Alternate => Self::Alternate,
            crate::config::UiSurfacePreference::Inline => Self::Inline,
        }
    }
}

impl From<KeyboardProtocolConfig> for KeyboardProtocolSettings {
    fn from(value: KeyboardProtocolConfig) -> Self {
        Self {
            enabled: value.enabled,
            mode: value.mode,
            disambiguate_escape_codes: value.disambiguate_escape_codes,
            report_event_types: value.report_event_types,
            report_alternate_keys: value.report_alternate_keys,
            report_all_keys: value.report_all_keys,
        }
    }
}

impl From<KeyboardProtocolSettings> for KeyboardProtocolConfig {
    fn from(value: KeyboardProtocolSettings) -> Self {
        Self {
            enabled: value.enabled,
            mode: value.mode,
            disambiguate_escape_codes: value.disambiguate_escape_codes,
            report_event_types: value.report_event_types,
            report_alternate_keys: value.report_alternate_keys,
            report_all_keys: value.report_all_keys,
        }
    }
}

/// Standalone fullscreen interaction settings for alternate-screen behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
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

    #[test]
    fn session_surface_conversion_roundtrip() {
        let variants = [
            SessionSurface::Auto,
            SessionSurface::Alternate,
            SessionSurface::Inline,
        ];

        for variant in variants {
            let converted: crate::config::UiSurfacePreference = variant.into();
            let roundtrip = SessionSurface::from(converted);
            assert_eq!(variant, roundtrip);
        }
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
