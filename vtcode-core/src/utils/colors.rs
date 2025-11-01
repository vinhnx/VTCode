//! Color utilities for the VTCode
//!
//! This module provides color manipulation capabilities using anstyle,
//! which offers low-level ANSI styling with RGB and 256-color support.

use anstyle::{AnsiColor, Color, Effects, RgbColor, Style};

fn styled(text: &str, style: Style) -> String {
    format!("{}{}{}", style.render(), text, style.render_reset())
}

/// Style wrapper for console::style compatibility
pub fn style(text: impl std::fmt::Display) -> StyledString {
    StyledString {
        text: text.to_string(),
        style: Style::new(),
    }
}

pub struct StyledString {
    text: String,
    style: Style,
}

impl StyledString {
    pub fn red(mut self) -> Self {
        self.style = self.style.fg_color(Some(Color::Ansi(AnsiColor::Red)));
        self
    }

    pub fn green(mut self) -> Self {
        self.style = self.style.fg_color(Some(Color::Ansi(AnsiColor::Green)));
        self
    }

    pub fn blue(mut self) -> Self {
        self.style = self.style.fg_color(Some(Color::Ansi(AnsiColor::Blue)));
        self
    }

    pub fn yellow(mut self) -> Self {
        self.style = self.style.fg_color(Some(Color::Ansi(AnsiColor::Yellow)));
        self
    }

    pub fn cyan(mut self) -> Self {
        self.style = self.style.fg_color(Some(Color::Ansi(AnsiColor::Cyan)));
        self
    }

    pub fn magenta(mut self) -> Self {
        self.style = self.style.fg_color(Some(Color::Ansi(AnsiColor::Magenta)));
        self
    }

    pub fn bold(mut self) -> Self {
        self.style = self.style.effects(self.style.get_effects() | Effects::BOLD);
        self
    }

    pub fn dimmed(mut self) -> Self {
        self.style = self
            .style
            .effects(self.style.get_effects() | Effects::DIMMED);
        self
    }

    pub fn dim(self) -> Self {
        self.dimmed()
    }

    pub fn on_black(mut self) -> Self {
        self.style = self.style.bg_color(Some(Color::Ansi(AnsiColor::Black)));
        self
    }
}

impl std::fmt::Display for StyledString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}{}{}",
            self.style.render(),
            self.text,
            self.style.render_reset()
        )
    }
}

/// Apply red color to text
pub fn red(text: &str) -> String {
    styled(
        text,
        Style::new().fg_color(Some(Color::Ansi(AnsiColor::Red))),
    )
}

/// Apply green color to text
pub fn green(text: &str) -> String {
    styled(
        text,
        Style::new().fg_color(Some(Color::Ansi(AnsiColor::Green))),
    )
}

/// Apply blue color to text
pub fn blue(text: &str) -> String {
    styled(
        text,
        Style::new().fg_color(Some(Color::Ansi(AnsiColor::Blue))),
    )
}

/// Apply yellow color to text
pub fn yellow(text: &str) -> String {
    styled(
        text,
        Style::new().fg_color(Some(Color::Ansi(AnsiColor::Yellow))),
    )
}

/// Apply purple color to text
pub fn purple(text: &str) -> String {
    styled(
        text,
        Style::new().fg_color(Some(Color::Ansi(AnsiColor::Magenta))),
    )
}

/// Apply cyan color to text
pub fn cyan(text: &str) -> String {
    styled(
        text,
        Style::new().fg_color(Some(Color::Ansi(AnsiColor::Cyan))),
    )
}

/// Apply white color to text
pub fn white(text: &str) -> String {
    styled(
        text,
        Style::new().fg_color(Some(Color::Ansi(AnsiColor::White))),
    )
}

/// Apply black color to text
pub fn black(text: &str) -> String {
    styled(
        text,
        Style::new().fg_color(Some(Color::Ansi(AnsiColor::Black))),
    )
}

/// Apply bold styling to text
pub fn bold(text: &str) -> String {
    styled(text, Style::new().effects(Effects::BOLD))
}

/// Apply italic styling to text
pub fn italic(text: &str) -> String {
    styled(text, Style::new().effects(Effects::ITALIC))
}

/// Apply underline styling to text
pub fn underline(text: &str) -> String {
    styled(text, Style::new().effects(Effects::UNDERLINE))
}

/// Apply dimmed styling to text
pub fn dimmed(text: &str) -> String {
    styled(text, Style::new().effects(Effects::DIMMED))
}

/// Apply blinking styling to text
pub fn blink(text: &str) -> String {
    styled(text, Style::new().effects(Effects::BLINK))
}

/// Apply reversed styling to text
pub fn reversed(text: &str) -> String {
    styled(text, Style::new().effects(Effects::INVERT))
}

/// Apply strikethrough styling to text
pub fn strikethrough(text: &str) -> String {
    styled(text, Style::new().effects(Effects::STRIKETHROUGH))
}

/// Apply custom RGB color to text
pub fn rgb(text: &str, r: u8, g: u8, b: u8) -> String {
    styled(
        text,
        Style::new().fg_color(Some(Color::Rgb(RgbColor(r, g, b)))),
    )
}

/// Combine multiple color and style operations
pub fn custom_style(text: &str, styles: &[&str]) -> String {
    let mut style = Style::new();

    for style_str in styles {
        match *style_str {
            "red" => style = style.fg_color(Some(Color::Ansi(AnsiColor::Red))),
            "green" => style = style.fg_color(Some(Color::Ansi(AnsiColor::Green))),
            "blue" => style = style.fg_color(Some(Color::Ansi(AnsiColor::Blue))),
            "yellow" => style = style.fg_color(Some(Color::Ansi(AnsiColor::Yellow))),
            "purple" => style = style.fg_color(Some(Color::Ansi(AnsiColor::Magenta))),
            "cyan" => style = style.fg_color(Some(Color::Ansi(AnsiColor::Cyan))),
            "white" => style = style.fg_color(Some(Color::Ansi(AnsiColor::White))),
            "black" => style = style.fg_color(Some(Color::Ansi(AnsiColor::Black))),
            "bold" => style = style.effects(style.get_effects() | Effects::BOLD),
            "italic" => style = style.effects(style.get_effects() | Effects::ITALIC),
            "underline" => style = style.effects(style.get_effects() | Effects::UNDERLINE),
            "dimmed" => style = style.effects(style.get_effects() | Effects::DIMMED),
            "blink" => style = style.effects(style.get_effects() | Effects::BLINK),
            "reversed" => style = style.effects(style.get_effects() | Effects::INVERT),
            "strikethrough" => style = style.effects(style.get_effects() | Effects::STRIKETHROUGH),
            _ => {}
        }
    }

    styled(text, style)
}
