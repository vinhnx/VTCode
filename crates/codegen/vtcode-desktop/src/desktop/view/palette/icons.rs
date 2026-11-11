//! Leading palette-row glyphs: one total `PaletteAction` mapping shared by
//! the Commands palette's fixed icon slot.

use super::*;

/// Leading glyph for a palette row. The Commands palette renders every row
/// with a fixed icon slot; other palettes stay text-only except permission
/// rows (see [`permission_palette_mode`]), so `None` only affects future
/// palettes. Icons reuse siblings' choices where they exist
/// (`SettingsTab::icon`, `files_panel::file_tree_row_icon`).
pub(in crate::desktop::view) fn palette_action_icon(action: &PaletteAction) -> Option<IconName> {
    let icon = match action {
        PaletteAction::NewSession => IconName::Plus,
        PaletteAction::CloseSession => IconName::X,
        PaletteAction::CopyThreadTitle | PaletteAction::CopyLatestMessage | PaletteAction::CopyFilePath => {
            IconName::Copy
        }
        PaletteAction::CloseOtherTabs => IconName::CopyX,
        PaletteAction::OpenInCli | PaletteAction::ShowActivityOutput { .. } => IconName::Terminal,
        PaletteAction::OpenConversationSearch { .. } | PaletteAction::OpenGoTo => IconName::Search,
        PaletteAction::JumpToTranscriptItem { .. } | PaletteAction::BrowseThreads | PaletteAction::LoadSession(_) => {
            IconName::MessageSquare
        }
        PaletteAction::ChooseModel | PaletteAction::SelectModel { .. } => IconName::Bot,
        PaletteAction::ChooseWorkspace => IconName::FolderOpen,
        PaletteAction::RefreshSessions | PaletteAction::RefreshFiles => IconName::RefreshCw,
        PaletteAction::ToggleInspector => IconName::PanelRight,
        PaletteAction::ToggleTheme => IconName::SunMoon,
        PaletteAction::SelectFont { .. } => IconName::ALargeSmall,
        PaletteAction::SelectSettingsTab(tab) => return tab.icon(),
        PaletteAction::EditConfigSetting(_) => IconName::Wrench,
        PaletteAction::PreviewFile(_) => files_panel::file_tree_row_icon(false, false),
        PaletteAction::SelectSessionOptions(_) => return None,
        PaletteAction::ToggleZenMode => IconName::Leaf,
        PaletteAction::AttachFiles | PaletteAction::AttachFile(_) => IconName::Paperclip,
        PaletteAction::InsertFileMention(_) => IconName::AtSign,
        PaletteAction::CompleteSlashCommand(_) | PaletteAction::InsertDraft(_) => IconName::Slash,
        PaletteAction::InsertHistoryPrompt(_) => IconName::Clock,
        PaletteAction::SelectWorkspaceReviewFile(_) => IconName::GitCompare,
        PaletteAction::OpenDeepLink(_) => IconName::ExternalLink,
    };
    Some(icon)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::assets::DesktopAssets;
    use gpui_kit::AssetSource;

    /// Every resolver arm with realistic payloads (session ids, file paths,
    /// drafts) rather than empty strings.
    fn icon_cases() -> Vec<(PaletteAction, Option<IconName>)> {
        vec![
            (PaletteAction::NewSession, Some(IconName::Plus)),
            (PaletteAction::CloseSession, Some(IconName::X)),
            (PaletteAction::CopyThreadTitle, Some(IconName::Copy)),
            (PaletteAction::CopyFilePath, Some(IconName::Copy)),
            (PaletteAction::CloseOtherTabs, Some(IconName::CopyX)),
            (PaletteAction::OpenInCli, Some(IconName::Terminal)),
            (
                PaletteAction::OpenConversationSearch { session_id: "session-1".to_string() },
                Some(IconName::Search),
            ),
            (PaletteAction::CopyLatestMessage, Some(IconName::Copy)),
            (
                PaletteAction::JumpToTranscriptItem {
                    session_id: "session-1".to_string(),
                    item_id: "item-1".to_string(),
                    from_search: true,
                },
                Some(IconName::MessageSquare),
            ),
            (
                PaletteAction::ShowActivityOutput {
                    session_id: "session-1".to_string(),
                    item_id: "item-1".to_string(),
                },
                Some(IconName::Terminal),
            ),
            (PaletteAction::ChooseModel, Some(IconName::Bot)),
            (PaletteAction::ChooseWorkspace, Some(IconName::FolderOpen)),
            (PaletteAction::RefreshSessions, Some(IconName::RefreshCw)),
            (PaletteAction::RefreshFiles, Some(IconName::RefreshCw)),
            (PaletteAction::BrowseThreads, Some(IconName::MessageSquare)),
            (PaletteAction::ToggleInspector, Some(IconName::PanelRight)),
            (PaletteAction::ToggleTheme, Some(IconName::SunMoon)),
            (PaletteAction::SelectFont { target: FontTarget::Ui, family: None }, Some(IconName::ALargeSmall)),
            (
                PaletteAction::SelectFont {
                    target: FontTarget::Code,
                    family: Some("SF Mono".to_string()),
                },
                Some(IconName::ALargeSmall),
            ),
            (PaletteAction::SelectSettingsTab(settings::SettingsTab::Shortcuts), Some(IconName::Keyboard)),
            (PaletteAction::SelectSettingsTab(settings::SettingsTab::About), Some(IconName::Info)),
            (PaletteAction::SelectSettingsTab(settings::SettingsTab::VtCode), None),
            (PaletteAction::EditConfigSetting("tab".to_string()), Some(IconName::Wrench)),
            (PaletteAction::LoadSession("session-1".to_string()), Some(IconName::MessageSquare)),
            (PaletteAction::PreviewFile("src/main.rs".to_string()), Some(IconName::FileText)),
            (
                PaletteAction::SelectModel {
                    provider: "acme".to_string(),
                    model: "m1".to_string(),
                },
                Some(IconName::Bot),
            ),
            (PaletteAction::SelectSessionOptions(protocol::SessionOptionsSelectParams::default()), None),
            (PaletteAction::ToggleZenMode, Some(IconName::Leaf)),
            (PaletteAction::AttachFiles, Some(IconName::Paperclip)),
            (PaletteAction::AttachFile("a/b.txt".to_string()), Some(IconName::Paperclip)),
            (PaletteAction::InsertFileMention("a/b.txt".to_string()), Some(IconName::AtSign)),
            (PaletteAction::CompleteSlashCommand("/stop".to_string()), Some(IconName::Slash)),
            (PaletteAction::InsertHistoryPrompt("old prompt".to_string()), Some(IconName::Clock)),
            (PaletteAction::InsertDraft("/plan ".to_string()), Some(IconName::Slash)),
            (PaletteAction::SelectWorkspaceReviewFile("x.rs".to_string()), Some(IconName::GitCompare)),
            (PaletteAction::OpenDeepLink(Box::new(deeplinks::DeepLink::ConfigPage)), Some(IconName::ExternalLink)),
            (PaletteAction::OpenGoTo, Some(IconName::Search)),
        ]
    }

    #[test]
    fn palette_action_icon_uses_expected_glyphs() {
        for (action, expected) in icon_cases() {
            assert_eq!(palette_action_icon(&action), expected, "{action:?}");
        }
        // Same-verb rows share one glyph; close/new verbs stay distinct.
        assert_eq!(
            palette_action_icon(&PaletteAction::RefreshSessions),
            palette_action_icon(&PaletteAction::RefreshFiles)
        );
        assert_ne!(palette_action_icon(&PaletteAction::NewSession), palette_action_icon(&PaletteAction::CloseSession));
        assert_ne!(
            palette_action_icon(&PaletteAction::CloseSession),
            palette_action_icon(&PaletteAction::CloseOtherTabs)
        );
    }

    #[test]
    fn commands_palette_rows_all_have_icons() {
        // Every action `open_command_palette` emits maps to `Some`; the two
        // `None` arms (session options, VtCode tab) never appear there, so
        // the reserved slot is never empty in ⌘K.
        let actions = [
            PaletteAction::NewSession,
            PaletteAction::CloseSession,
            PaletteAction::CopyThreadTitle,
            PaletteAction::CopyFilePath,
            PaletteAction::CloseOtherTabs,
            PaletteAction::OpenInCli,
            PaletteAction::ChooseWorkspace,
            PaletteAction::RefreshSessions,
            PaletteAction::RefreshFiles,
            PaletteAction::ChooseModel,
            PaletteAction::BrowseThreads,
            PaletteAction::ToggleInspector,
            PaletteAction::ToggleZenMode,
            PaletteAction::ToggleTheme,
            PaletteAction::OpenGoTo,
            PaletteAction::SelectSettingsTab(settings::SettingsTab::Shortcuts),
            PaletteAction::OpenConversationSearch { session_id: "session-1".to_string() },
            PaletteAction::CopyLatestMessage,
            PaletteAction::InsertDraft("/plan ".to_string()),
            PaletteAction::LoadSession("session-1".to_string()),
            PaletteAction::PreviewFile("src/main.rs".to_string()),
        ];
        for action in actions {
            assert!(palette_action_icon(&action).is_some(), "{action:?} must map to an icon");
        }
    }

    #[test]
    fn palette_action_icons_resolve_through_desktop_assets() {
        // Unregistered glyphs render blank with no headless failure, so every
        // mapped icon must load through the desktop asset source.
        let assets = DesktopAssets;
        for (action, _) in icon_cases() {
            if let Some(icon) = palette_action_icon(&action) {
                assert!(
                    assets.load(&icon.path()).expect("load icon asset").is_some(),
                    "desktop asset source must include {}",
                    icon.path()
                );
            }
        }
    }
}
