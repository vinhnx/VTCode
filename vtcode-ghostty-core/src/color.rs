/// Named ANSI terminal colors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AnsiColor {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
}

/// Terminal color representation supporting ANSI, 256-color, and RGB.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Color {
    Ansi(AnsiColor),
    Indexed(u8),
    Rgb { r: u8, g: u8, b: u8 },
}

impl Color {
    /// Parse a color from SGR extended color parameters.
    ///
    /// `type_param` is the color type (5 for indexed, 2 for RGB).
    /// `params` are the remaining parameters after the type.
    pub(crate) fn from_sgr_params(type_param: usize, params: &[Option<usize>]) -> Option<Self> {
        match type_param {
            5 => {
                let idx = params.first().copied().flatten()?;
                if idx <= 255 {
                    Some(Color::Indexed(idx as u8))
                } else {
                    None
                }
            }
            2 => {
                let r = params.first().copied().flatten()?;
                let g = params.get(1).copied().flatten()?;
                let b = params.get(2).copied().flatten()?;
                if r <= 255 && g <= 255 && b <= 255 {
                    Some(Color::Rgb {
                        r: r as u8,
                        g: g as u8,
                        b: b as u8,
                    })
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

/// Map an ANSI color code (30-37, 90-97) to an `AnsiColor`.
pub(crate) fn ansi_fg(code: usize) -> Option<AnsiColor> {
    match code {
        30 => Some(AnsiColor::Black),
        31 => Some(AnsiColor::Red),
        32 => Some(AnsiColor::Green),
        33 => Some(AnsiColor::Yellow),
        34 => Some(AnsiColor::Blue),
        35 => Some(AnsiColor::Magenta),
        36 => Some(AnsiColor::Cyan),
        37 => Some(AnsiColor::White),
        90 => Some(AnsiColor::BrightBlack),
        91 => Some(AnsiColor::BrightRed),
        92 => Some(AnsiColor::BrightGreen),
        93 => Some(AnsiColor::BrightYellow),
        94 => Some(AnsiColor::BrightBlue),
        95 => Some(AnsiColor::BrightMagenta),
        96 => Some(AnsiColor::BrightCyan),
        97 => Some(AnsiColor::BrightWhite),
        _ => None,
    }
}

/// Map an ANSI background color code (40-47, 100-107) to an `AnsiColor`.
pub(crate) fn ansi_bg(code: usize) -> Option<AnsiColor> {
    ansi_fg(code - 10)
}
