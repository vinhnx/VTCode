use anstyle::Style;
use vtcode_tui::{InlineTextStyle, InlineTheme, convert_style};

fn main() {
    let _theme = InlineTheme {
        primary: Some(anstyle::AnsiColor::Cyan.into()),
        secondary: Some(anstyle::AnsiColor::Blue.into()),
        ..InlineTheme::default()
    };
    let _converted: InlineTextStyle = convert_style(Style::new().bold());
}
