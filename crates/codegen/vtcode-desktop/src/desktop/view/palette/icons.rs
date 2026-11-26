//! Leading palette-row glyphs: one total `PaletteAction` mapping shared by
//! the Commands palette's fixed icon slot.

use super::*;

/// Leading glyph for a palette row. The Commands palette renders every row
/// with a fixed icon slot; other palettes stay text-only except permission
/// rows (see [`permission_palette_mode`]), so `None` only affects future
/// palettes. Icons reuse siblings' choices where they exist
/// (`SettingsTab::icon`, `files_panel::file_tree_row_icon`).
pub(in crate::desktop::view) fn palette_action_icon(action: &PaletteAction) -> Option<UiIcon> {
    let icon = match action {
        PaletteAction::NewSession => IconName::Plus.into(),
        PaletteAction::CloseSession => IconName::X.into(),
        PaletteAction::CopyThreadTitle | PaletteAction::CopyLatestMessage | PaletteAction::CopyFilePath => {
            IconName::Copy.into()
        }
        PaletteAction::CloseOtherTabs => IconName::CopyX.into(),
        PaletteAction::OpenInCli | PaletteAction::ShowActivityOutput { .. } => IconName::Terminal.into(),
        PaletteAction::OpenConversationSearch { .. } | PaletteAction::OpenGoTo => IconName::Search.into(),
        // Browse Threads owns the tree glyph everywhere (rail, zen chrome,
        // palette); message and session rows use the plural conversation glyph.
        PaletteAction::BrowseThreads => IconName::ListTree.into(),
        PaletteAction::JumpToTranscriptItem { .. } | PaletteAction::LoadSession(_) => IconName::MessagesSquare.into(),
        // The AI model wears the VT Code brand V, not a robot.
        PaletteAction::ChooseModel | PaletteAction::SelectModel { .. } => UiIcon::BrandV,
        PaletteAction::ChooseWorkspace => IconName::FolderOpen.into(),
        PaletteAction::RefreshSessions | PaletteAction::RefreshFiles => IconName::RefreshCw.into(),
        PaletteAction::ToggleInspector => IconName::PanelRight.into(),
        PaletteAction::ToggleTheme => IconName::SunMoon.into(),
        PaletteAction::SelectFont { .. } => IconName::ALargeSmall.into(),
        PaletteAction::SelectSettingsTab(tab) => return tab.icon().map(UiIcon::Named),
        PaletteAction::EditConfigSetting(_) => IconName::Wrench.into(),
        PaletteAction::PreviewFile(_) => files_panel::file_tree_row_icon(false, false).into(),
        PaletteAction::SelectSessionOptions(_) => return None,
        PaletteAction::ToggleZenMode => IconName::Leaf.into(),
        PaletteAction::ToggleComputerUse => return settings::SettingsTab::ComputerUse.icon().map(UiIcon::Named),
        PaletteAction::AttachFiles | PaletteAction::AttachFile(_) => IconName::Paperclip.into(),
        PaletteAction::InsertFileMention(_) => IconName::AtSign.into(),
        PaletteAction::CompleteSlashCommand(_) | PaletteAction::InsertDraft(_) => IconName::Slash.into(),
        PaletteAction::InsertHistoryPrompt(_) => IconName::Clock.into(),
        PaletteAction::SelectWorkspaceReviewFile(_) => IconName::GitCompare.into(),
        PaletteAction::OpenDeepLink(_) => IconName::ExternalLink.into(),
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
                Some(IconName::MessagesSquare),
            ),
            (
                PaletteAction::ShowActivityOutput {
                    session_id: "session-1".to_string(),
                    item_id: "item-1".to_string(),
                },
                Some(IconName::Terminal),
            ),
            (PaletteAction::ChooseWorkspace, Some(IconName::FolderOpen)),
            (PaletteAction::RefreshSessions, Some(IconName::RefreshCw)),
            (PaletteAction::RefreshFiles, Some(IconName::RefreshCw)),
            (PaletteAction::BrowseThreads, Some(IconName::ListTree)),
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
            (PaletteAction::LoadSession("session-1".to_string()), Some(IconName::MessagesSquare)),
            (PaletteAction::PreviewFile("src/main.rs".to_string()), Some(IconName::FileText)),
            (PaletteAction::SelectSessionOptions(protocol::SessionOptionsSelectParams::default()), None),
            (PaletteAction::ToggleZenMode, Some(IconName::Leaf)),
            (PaletteAction::ToggleComputerUse, Some(IconName::Computer)),
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
            assert_eq!(palette_action_icon(&action), expected.map(UiIcon::Named), "{action:?}");
        }
        // Model rows use the brand V, never a robot glyph.
        assert_eq!(palette_action_icon(&PaletteAction::ChooseModel), Some(UiIcon::BrandV));
        assert_eq!(
            palette_action_icon(&PaletteAction::SelectModel {
                provider: "acme".to_string(),
                model: "m1".to_string(),
            }),
            Some(UiIcon::BrandV)
        );
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
            PaletteAction::ToggleComputerUse,
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
            if let Some(UiIcon::Named(icon)) = palette_action_icon(&action) {
                assert!(
                    assets.load(&icon.path()).expect("load icon asset").is_some(),
                    "desktop asset source must include {}",
                    icon.path()
                );
            }
        }
    }
}
