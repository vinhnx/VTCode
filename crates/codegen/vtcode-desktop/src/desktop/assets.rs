use std::borrow::Cow;

use gpui_kit::assets::{Assets as KitAssets, icon_assets};
use gpui_kit::{AssetSource, Result, SharedString};

// Keep the native bundle to GPUI Kit's default icons plus the extra catalog
// entries referenced by VT Code Desktop.
icon_assets!(
    ExtraIcons,
    [
        Activity,
        FileCode,
        FolderGit2,
        GitCompare,
        Maximize2,
        MessageSquare,
        Shield,
        Terminal,
        X
    ]
);

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct DesktopAssets;

impl AssetSource for DesktopAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        KitAssets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = KitAssets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.sort_by(|left, right| left.as_ref().cmp(right.as_ref()));
        paths.dedup();
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::AssetSource;
    use gpui_kit::assets::IconName;

    use super::DesktopAssets;

    #[test]
    fn desktop_assets_include_every_icon_referenced_by_the_ui() {
        let assets = DesktopAssets;
        let icons = [
            IconName::Activity,
            IconName::ArrowDown,
            IconName::ArrowUp,
            IconName::Bot,
            IconName::Check,
            IconName::ChevronDown,
            IconName::ChevronLeft,
            IconName::ChevronRight,
            IconName::Copy,
            IconName::Ellipsis,
            IconName::ExternalLink,
            IconName::File,
            IconName::FileCode,
            IconName::FileText,
            IconName::Folder,
            IconName::FolderClosed,
            IconName::FolderGit2,
            IconName::FolderOpen,
            IconName::GitCompare,
            IconName::Maximize2,
            IconName::MessageSquare,
            IconName::Minus,
            IconName::PanelLeft,
            IconName::PanelRight,
            IconName::Plus,
            IconName::RefreshCw,
            IconName::Search,
            IconName::Settings2,
            IconName::Shield,
            IconName::Square,
            IconName::Terminal,
            IconName::X,
        ];

        for icon in icons {
            assert!(
                assets.load(&icon.path()).expect("load icon asset").is_some(),
                "desktop asset source must include {}",
                icon.path()
            );
        }
    }

    #[test]
    fn desktop_asset_listing_is_sorted_and_contains_default_and_extra_icons() {
        let paths = DesktopAssets.list("icons/").expect("list icon assets");
        let names: Vec<_> = paths.iter().map(|path| path.as_ref()).collect();

        assert!(names.windows(2).all(|pair| pair[0] <= pair[1]), "asset paths stay sorted");
        assert!(names.contains(&"icons/bot.svg"));
        assert!(names.contains(&"icons/file-code.svg"));
    }
}
