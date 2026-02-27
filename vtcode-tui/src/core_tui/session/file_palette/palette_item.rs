use super::FileEntry;

/// Implement PaletteItem trait for FileEntry to support generic PaletteRenderer
impl super::super::palette_renderer::PaletteItem for FileEntry {
    fn display_name(&self) -> String {
        self.display_name.clone()
    }

    fn display_icon(&self) -> Option<String> {
        if self.is_dir {
            Some("↳  ".to_owned())
        } else {
            Some("  · ".to_owned())
        }
    }

    fn is_directory(&self) -> bool {
        self.is_dir
    }
}
