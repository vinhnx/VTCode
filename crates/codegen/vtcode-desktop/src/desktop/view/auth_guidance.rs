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
/// no credentials. Desktop-first ordering: the in-app path (Settings form or
/// the provider group's Run button over the built-in terminal) leads, and the
/// raw CLI command follows as the alternative. API-key providers offer an
/// in-app Settings form (no reload needed after saving); login-flow providers
/// complete setup from the Run button, then Reload CLI settings.
pub(super) fn provider_auth_guidance(configuration: Option<&ConfigurationSnapshot>) -> String {
    let setup = configuration
        .map(|snapshot| snapshot.credential_setup_command.trim())
        .filter(|command| !command.is_empty());
    if configuration.is_some_and(|snapshot| snapshot.secret_store.is_some()) {
        format!(
            "{PROVIDER_AUTH_GUIDANCE_PREFIX} Save your API key in Settings → VT Code → Provider to reconnect. Terminal alternative: run `{}`, then choose Reload CLI settings in Settings → VT Code.",
            setup.unwrap_or("vtcode auth")
        )
    } else if let Some(command) = setup {
        format!(
            "{PROVIDER_AUTH_GUIDANCE_PREFIX} Open Settings → VT Code → Provider and choose Run to complete setup in the built-in terminal, then choose Reload CLI settings to reconnect. Terminal alternative: run `{command}` in your own terminal instead."
        )
    } else {
        // No setup command advertised (only local providers, which never need
        // credentials): point at the provider group without promising a Run
        // button that is not rendered.
        format!(
            "{PROVIDER_AUTH_GUIDANCE_PREFIX} Open Settings → VT Code → Provider to continue setup, then choose Reload CLI settings to reconnect."
        )
    }
}

pub(super) fn scale_row_height_for_ui_font(row_height: f32, font_size: Pixels) -> f32 {
    row_height * font_size.as_f32() / DEFAULT_UI_FONT_SIZE
}
