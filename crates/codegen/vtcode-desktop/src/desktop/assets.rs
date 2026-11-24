use std::borrow::Cow;

use gpui_kit::assets::{Assets as KitAssets, icon_assets};
use gpui_kit::{AssetSource, Result, SharedString};

// Keep the native bundle to GPUI Kit's default icons plus the extra catalog
// entries referenced by VT Code Desktop.
icon_assets!(
    ExtraIcons,
    [
        Activity,
        ALargeSmall,
        AtSign,
        Clock,
        Computer,
        CopyX,
        Download,
        FileCode,
        FileImage,
        FileUp,
        FolderGit2,
        GitBranch,
        GitCompare,
        House,
        Keyboard,
        LaptopMinimal,
        Leaf,
        ListChecks,
        ListTree,
        Maximize2,
        Minimize2,
        MessageSquare,
        ChevronsDown,
        Paperclip,
        Pencil,
        PersonStanding,
        Plug,
        Shield,
        ShieldCheck,
        ShieldAlert,
        Slash,
        Square,
        SquareTerminal,
        SunMoon,
        Target,
        Terminal,
        Trash,
        Wrench,
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
    use std::collections::{BTreeSet, HashMap};
    use std::path::{Path, PathBuf};

    use gpui_kit::AssetSource;
    use gpui_kit::SharedString;
    use gpui_kit::assets::IconName;

    use super::DesktopAssets;

    /// Collect `IconName::Variant` tokens from one line of source. Asset
    /// lookups happen at runtime, so an unregistered glyph is blank with no
    /// headless failure; the only reliable oracle is the source itself.
    /// `IconName::ALL` is an associated const enumerating every name, not a
    /// glyph the asset source must serve, so it is deliberately skipped.
    fn collect_icon_names(line: &str, names: &mut BTreeSet<String>) {
        let code = line.split("//").next().unwrap_or_default();
        let mut rest = code;
        while let Some(pos) = rest.find("IconName::") {
            let after = rest
                .get(pos + "IconName::".len()..)
                .expect("ASCII prefix ends on a char boundary");
            let name: String = after
                .chars()
                .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
                .collect();
            if !name.is_empty() && name != "ALL" {
                let _ = names.insert(name);
            }
            // The prefix is consumed; keep scanning the remainder for more.
            rest = after;
        }
    }

    /// Walk every Rust source under `src/` (the asset registry itself owns no
    /// glyph usages, so it is excluded) and return the variants the UI names.
    fn referenced_icon_names() -> BTreeSet<String> {
        fn walk(dir: &Path, files: &mut Vec<PathBuf>) {
            let mut entries = std::fs::read_dir(dir)
                .expect("read desktop source dir")
                .filter_map(Result::ok)
                .collect::<Vec<_>>();
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, files);
                } else if path.extension().is_some_and(|ext| ext == "rs")
                    && path.file_name().is_some_and(|name| name != "assets.rs")
                {
                    files.push(path);
                }
            }
        }

        let mut files = Vec::new();
        walk(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
        assert!(!files.is_empty(), "source scan must find Rust files");

        let mut names = BTreeSet::new();
        for file in files {
            let content = std::fs::read_to_string(&file).expect("read desktop source file");
            for line in content.lines() {
                collect_icon_names(line, &mut names);
            }
        }
        names
    }

    /// `IconName::ALL` is the authoritative name → enum table shipped by the
    /// pinned assets crate; `path()` is the exact key the source is asked for.
    fn referenced_icon_paths() -> Vec<(String, SharedString)> {
        let by_name: HashMap<String, IconName> =
            IconName::ALL.iter().map(|icon| (format!("{icon:?}"), *icon)).collect();
        referenced_icon_names()
            .into_iter()
            .map(|name| {
                let icon = *by_name
                    .get(&name)
                    .unwrap_or_else(|| panic!("{name} is not an IconName variant"));
                (name, icon.path())
            })
            .collect()
    }

    #[test]
    fn icon_reference_scan_reads_variants_and_ignores_comments_and_all() {
        let mut names = BTreeSet::new();
        collect_icon_names("let a = IconName::Activity; // IconName::NotRegistered", &mut names);
        collect_icon_names("for name in IconName::ALL { let _ = IconName::X; }", &mut names);
        assert_eq!(names, BTreeSet::from(["Activity".to_string(), "X".to_string()]));
    }

    #[test]
    fn every_icon_referenced_by_the_ui_resolves_through_desktop_assets() {
        let referenced = referenced_icon_paths();
        assert!(referenced.len() > 50, "source scan must discover UI glyphs, found {}", referenced.len());
        let assets = DesktopAssets;
        for (name, path) in referenced {
            let loaded = assets
                .load(&path)
                .unwrap_or_else(|err| panic!("IconName::{name} ({path}) cannot be resolved: {err}"));
            assert!(
                loaded.is_some(),
                "UI references IconName::{name}, but {path} is not registered in ExtraIcons/KitAssets"
            );
        }
    }

    #[test]
    fn desktop_asset_listing_is_sorted_and_contains_every_referenced_icon() {
        let paths = DesktopAssets.list("icons/").expect("list icon assets");
        let names: Vec<_> = paths.iter().map(|path| path.as_ref()).collect();

        assert!(names.windows(2).all(|pair| pair[0] <= pair[1]), "asset paths stay sorted");
        assert_eq!(names.len(), names.iter().copied().collect::<std::collections::HashSet<_>>().len());
        for (_, path) in referenced_icon_paths() {
            assert!(names.contains(&path.as_ref()), "asset listing must include {path}");
        }
    }

    #[test]
    fn desktop_assets_return_none_for_empty_paths_and_errors_for_missing_paths() {
        let assets = DesktopAssets;

        assert!(assets.load("").expect("load empty path").is_none());
        assert!(assets.load("icons/not-a-vtcode-icon.svg").is_err());
    }
}
