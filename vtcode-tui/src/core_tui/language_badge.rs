use ratatui::style::{Color, Modifier, Style};

pub(crate) fn language_badge_style(language: &str) -> Option<Style> {
    let color = match language.trim().to_ascii_lowercase().as_str() {
        "rust" => Color::Rgb(0xCE, 0x7E, 0x47),
        "swift" => Color::Rgb(0xF0, 0x91, 0x3E),
        "ruby" => Color::Rgb(0xC7, 0x45, 0x45),
        "typescript" | "javascript" => Color::Rgb(0x4F, 0x8F, 0xD8),
        "python" => Color::Rgb(0xD8, 0xC0, 0x4F),
        "go" => Color::Rgb(0x4F, 0xC2, 0xD8),
        "java" => Color::Rgb(0xC9, 0x6B, 0x3C),
        "bash" => Color::Rgb(0x6B, 0xB8, 0x5A),
        "c" | "c++" => Color::Rgb(0x7A, 0x8F, 0xD8),
        "php" => Color::Rgb(0x8C, 0x7B, 0xD8),
        _ => return None,
    };

    Some(Style::default().fg(color).add_modifier(Modifier::BOLD))
}

#[cfg(test)]
mod tests {
    use super::language_badge_style;
    use ratatui::style::Color;

    #[test]
    fn language_badge_style_maps_expected_families() {
        assert_eq!(
            language_badge_style("Rust").and_then(|style| style.fg),
            Some(Color::Rgb(0xCE, 0x7E, 0x47))
        );
        assert_eq!(
            language_badge_style("Swift").and_then(|style| style.fg),
            Some(Color::Rgb(0xF0, 0x91, 0x3E))
        );
        assert_eq!(
            language_badge_style("Ruby").and_then(|style| style.fg),
            Some(Color::Rgb(0xC7, 0x45, 0x45))
        );
        assert_eq!(
            language_badge_style("TypeScript").and_then(|style| style.fg),
            Some(Color::Rgb(0x4F, 0x8F, 0xD8))
        );
        assert_eq!(
            language_badge_style("JavaScript").and_then(|style| style.fg),
            Some(Color::Rgb(0x4F, 0x8F, 0xD8))
        );
        assert!(language_badge_style("Unknown").is_none());
    }
}
