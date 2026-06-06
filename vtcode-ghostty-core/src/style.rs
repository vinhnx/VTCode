use crate::color::{self, Color};

/// Text rendering attributes for a terminal cell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Style {
    pub bold: bool,
    pub faint: bool,
    pub italic: bool,
    pub underline: bool,
    pub blink: bool,
    pub inverse: bool,
    pub strikethrough: bool,
    pub fg: Option<Color>,
    pub bg: Option<Color>,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            bold: false,
            faint: false,
            italic: false,
            underline: false,
            blink: false,
            inverse: false,
            strikethrough: false,
            fg: None,
            bg: None,
        }
    }
}

impl Style {
    /// Apply SGR (Select Graphic Rendition) parameters to this style.
    pub(crate) fn apply_sgr(&mut self, params: &[Option<usize>]) {
        if params.is_empty() {
            *self = Self::default();
            return;
        }

        let mut i = 0;
        while i < params.len() {
            let code = match params[i] {
                Some(v) => v,
                None => {
                    // Missing parameter treats as 0 (reset).
                    *self = Self::default();
                    i += 1;
                    continue;
                }
            };

            match code {
                0 => *self = Self::default(),
                1 => self.bold = true,
                2 => self.faint = true,
                3 => self.italic = true,
                4 => self.underline = true,
                5 | 6 => self.blink = true,
                7 => self.inverse = true,
                9 => self.strikethrough = true,
                22 => {
                    self.bold = false;
                    self.faint = false;
                }
                23 => self.italic = false,
                24 => self.underline = false,
                25 => self.blink = false,
                27 => self.inverse = false,
                29 => self.strikethrough = false,
                30..=37 | 90..=97 => {
                    self.fg = color::ansi_fg(code).map(Color::Ansi);
                }
                39 => self.fg = None,
                40..=47 | 100..=107 => {
                    self.bg = color::ansi_bg(code).map(Color::Ansi);
                }
                49 => self.bg = None,
                38 => {
                    // Extended foreground: 38;5;N or 38;2;R;G;B
                    if let Some(type_param) = params.get(i + 1).and_then(|p| *p) {
                        match type_param {
                            5 => {
                                if let Some(c) =
                                    Color::from_sgr_params(5, params.get(i + 2..).unwrap_or(&[]))
                                {
                                    self.fg = Some(c);
                                }
                                i += 3;
                                continue;
                            }
                            2 => {
                                if let Some(c) =
                                    Color::from_sgr_params(2, params.get(i + 2..).unwrap_or(&[]))
                                {
                                    self.fg = Some(c);
                                }
                                i += 5;
                                continue;
                            }
                            _ => {}
                        }
                    }
                }
                48 => {
                    // Extended background: 48;5;N or 48;2;R;G;B
                    if let Some(type_param) = params.get(i + 1).and_then(|p| *p) {
                        match type_param {
                            5 => {
                                if let Some(c) =
                                    Color::from_sgr_params(5, params.get(i + 2..).unwrap_or(&[]))
                                {
                                    self.bg = Some(c);
                                }
                                i += 3;
                                continue;
                            }
                            2 => {
                                if let Some(c) =
                                    Color::from_sgr_params(2, params.get(i + 2..).unwrap_or(&[]))
                                {
                                    self.bg = Some(c);
                                }
                                i += 5;
                                continue;
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::{AnsiColor, Color};

    #[test]
    fn sgr_reset() {
        let mut style = Style {
            bold: true,
            fg: Some(Color::Ansi(AnsiColor::Red)),
            ..Style::default()
        };
        style.apply_sgr(&[Some(0)]);
        assert_eq!(style, Style::default());
    }

    #[test]
    fn sgr_bold_and_color() {
        let mut style = Style::default();
        style.apply_sgr(&[Some(1), Some(31)]);
        assert!(style.bold);
        assert_eq!(style.fg, Some(Color::Ansi(AnsiColor::Red)));
    }

    #[test]
    fn sgr_256_color() {
        let mut style = Style::default();
        style.apply_sgr(&[Some(38), Some(5), Some(196)]);
        assert_eq!(style.fg, Some(Color::Indexed(196)));
    }

    #[test]
    fn sgr_rgb_color() {
        let mut style = Style::default();
        style.apply_sgr(&[Some(38), Some(2), Some(255), Some(128), Some(0)]);
        assert_eq!(
            style.fg,
            Some(Color::Rgb {
                r: 255,
                g: 128,
                b: 0
            })
        );
    }

    #[test]
    fn sgr_individual_resets() {
        let mut style = Style {
            bold: true,
            italic: true,
            underline: true,
            ..Style::default()
        };
        style.apply_sgr(&[Some(22), Some(23)]);
        assert!(!style.bold);
        assert!(!style.faint);
        assert!(!style.italic);
        assert!(style.underline);
    }
}
