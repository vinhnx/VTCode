//! Test launch-intercept records and small view helpers extracted from `view.rs`.

use super::*;

/// A recorded external-editor launch request, captured only under test interception.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::desktop) struct ExternalLaunchCall {
    pub rel_path: String,
    pub editor: files::ExternalEditor,
}

/// A recorded CLI handoff script launch: the exact script content that would
/// have been written and opened, so GUI tests assert routing without spawning
/// `/usr/bin/open`.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::desktop) struct HandoffScriptCall {
    pub content: String,
    pub editor: files::ExternalEditor,
}

/// Test-only interception of external-editor launches so GUI tests never shell
/// out to `/usr/bin/open`. The completion path stays shared with production.
#[cfg(test)]
#[derive(Debug, Default)]
pub(in crate::desktop) struct TestLaunchIntercept {
    pub calls: Vec<ExternalLaunchCall>,
    pub handoff_scripts: Vec<HandoffScriptCall>,
    /// Simulated failure for every intercepted launch; `None` means success.
    pub fail_with: Option<String>,
}

pub(in crate::desktop) fn send_request(commands: &mpsc::Sender<BackendCommand>, method: &str, params: Value) -> bool {
    commands
        .try_send(BackendCommand::Request { method: method.to_string(), params })
        .is_ok()
}

pub(in crate::desktop) fn valid_slash_command_catalog(commands: &[protocol::SlashCommandInfo]) -> bool {
    commands.len() <= 256
        && commands.iter().all(|command| {
            !command.name.is_empty()
                && command.name.len() <= 80
                && command
                    .name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
                && command.description.chars().count() <= 1_000
                && !command.description.chars().any(char::is_control)
        })
}

pub(in crate::desktop) fn workspace_picker_options() -> PathPromptOptions {
    PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some(rust_i18n::t!(i18n::WORKSPACE_CHOOSE).to_string().into()),
    }
}

pub(in crate::desktop) fn chrome_background(colors: &gpui_kit::component::theme::ThemeColor) -> gpui_kit::Hsla {
    themes::navigation_surfaces(colors.background, colors.secondary, colors.foreground).rail
}
