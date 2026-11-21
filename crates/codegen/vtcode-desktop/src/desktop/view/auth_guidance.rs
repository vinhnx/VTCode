use super::*;

/// Leading sentence shared by both credential-guidance variants. The full
/// sentences stay intact below (one localizable unit each); only this prefix
/// is shared so [`is_provider_auth_guidance`] cannot drift from the generator.
const PROVIDER_AUTH_GUIDANCE_PREFIX: &str = "No provider credentials are configured.";

/// Whether an error banner shows credential guidance rather than an
/// unrelated failure. A prefix match avoids rebuilding the guidance string
/// just to compare; no other error uses this prefix.
pub(super) fn is_provider_auth_guidance(error: &str) -> bool {
    error.starts_with(PROVIDER_AUTH_GUIDANCE_PREFIX)
}

/// Guidance shown while the service is connected but the active provider has
/// no credentials. API-key providers offer an in-app Settings form (no reload
/// needed after saving); login-flow providers still go through the Terminal.
pub(super) fn provider_auth_guidance(configuration: Option<&ConfigurationSnapshot>) -> String {
    let setup = configuration
        .map(|snapshot| snapshot.credential_setup_command.trim())
        .filter(|command| !command.is_empty())
        .unwrap_or("vtcode auth");
    if configuration.is_some_and(|snapshot| snapshot.secret_store.is_some()) {
        format!(
            "{PROVIDER_AUTH_GUIDANCE_PREFIX} Save your API key in Settings → VT Code → Provider to reconnect. Terminal alternative: run `{setup}`, then choose Reload CLI settings in Settings → VT Code."
        )
    } else {
        format!(
            "{PROVIDER_AUTH_GUIDANCE_PREFIX} In Terminal, run `{setup}`, then choose Reload CLI settings in Settings → VT Code to reconnect."
        )
    }
}

pub(super) fn scale_row_height_for_ui_font(row_height: f32, font_size: Pixels) -> f32 {
    row_height * font_size.as_f32() / DEFAULT_UI_FONT_SIZE
}
